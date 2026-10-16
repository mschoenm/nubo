//! Interrupts and the HALT state.
//!
//! Hardware requests an interrupt by setting a bit in IF (the interrupt flag
//! register at 0xFF0F). If the same bit is set in IE (the interrupt enable
//! register at 0xFFFF) and IME (the CPU's interrupt master enable) is on, the
//! CPU pushes the program counter and jumps to that interrupt's handler.
//!
//! When the CPU looks for a pending interrupt:
//!
//! - While running, after it prefetches the next opcode
//!   (`dispatch_interrupt_if_pending`).
//! - HALT first spends one more M-cycle fetching the next opcode and only
//!   then checks, which is also when the HALT bug can happen (`halt` in
//!   `opcodes.rs`).
//! - While halted, once per M-cycle like SameBoy: before the M-cycle on the
//!   CGB and on the first halted M-cycle, halfway through it on the DMG. The
//!   CPU finishes that M-cycle before it wakes up or jumps to the handler
//!   (`halted_step`).

use crate::clock::dots_per_m_cycle;
use crate::cpu::{Cpu, InterruptMasterEnable, NextInstruction, RunState};

/// The five interrupt sources, in priority order (highest first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InterruptType {
    VBlank,
    LcdStatus,
    TimerOverflow,
    SerialLink,
    JoypadPress,
}

/// IF, the register of requested interrupts.
const INTERRUPT_FLAGS_ADDRESS: u16 = 0xFF0F;
const INTERRUPTS_BY_PRIORITY: [InterruptType; 5] = [
    InterruptType::VBlank,
    InterruptType::LcdStatus,
    InterruptType::TimerOverflow,
    InterruptType::SerialLink,
    InterruptType::JoypadPress,
];

impl InterruptType {
    /// The interrupt's bit in IE and IF.
    fn bit(self) -> u8 {
        match self {
            InterruptType::VBlank => 0x01,
            InterruptType::LcdStatus => 0x02,
            InterruptType::TimerOverflow => 0x04,
            InterruptType::SerialLink => 0x08,
            InterruptType::JoypadPress => 0x10,
        }
    }

    /// The address the CPU jumps to when it handles the interrupt.
    fn handler_address(self) -> u16 {
        match self {
            InterruptType::VBlank => 0x40,
            InterruptType::LcdStatus => 0x48,
            InterruptType::TimerOverflow => 0x50,
            InterruptType::SerialLink => 0x58,
            InterruptType::JoypadPress => 0x60,
        }
    }
}

/// The highest priority interrupt that is both enabled in `enabled` and
/// requested in `flags`.
fn pick_interrupt(enabled: u8, flags: u8) -> Option<InterruptType> {
    let pending_bits = enabled & flags & 0x1F;
    INTERRUPTS_BY_PRIORITY
        .into_iter()
        .find(|interrupt| pending_bits & interrupt.bit() != 0)
}

impl Cpu {
    /// The interrupts that are both requested (IF) and enabled (IE).
    fn pending_interrupt_bits(&self) -> u8 {
        self.address_bus.interrupt_enable() & self.address_bus.interrupt_flags() & 0x1F
    }

    /// Whether an interrupt is requested and enabled in IE, whatever IME says.
    pub(super) fn has_pending_interrupt(&self) -> bool {
        self.pending_interrupt_bits() != 0
    }

    /// Turns IME off, pushes the program counter and jumps to the handler of
    /// the highest priority pending interrupt, fetching its first opcode.
    /// Spends five M-cycles: two idle ones, two to push the program counter
    /// and the fetch.
    ///
    /// An HBlank DMA block that is due when the dispatch starts is copied
    /// first (Gambatte's irq_precedence and hdma_transition_ei_halt_late_unhalt
    /// tests).
    ///
    /// IE is read after the high byte is pushed, so a push that overwrites IE
    /// (0xFFFF) can change it or cancel the dispatch, which then jumps to
    /// 0x0000 (Mooneye's ie_push test). As in SameBoy, IF is read after the
    /// low byte is pushed (or before, if that push writes IF itself), but the
    /// chosen request is only cleared halfway through the fetch, so the same
    /// request arriving in between is cleared with it.
    fn dispatch_interrupt(&mut self) {
        self.ime = InterruptMasterEnable::Off;
        if self.address_bus.hdma_block_pending() {
            self.address_bus.hdma_step();
        }
        self.step_one_machine_cycle();
        self.step_one_machine_cycle();

        let program_counter = self.registers.program_counter;
        let stack_pointer = self.registers.stack_pointer;
        self.check_oam_bug_write(stack_pointer);

        let high_byte_address = stack_pointer.wrapping_sub(1);
        self.store_byte_in_memory(high_byte_address, (program_counter >> 8) as u8);
        self.check_oam_bug_write(high_byte_address);

        let enabled = self.address_bus.interrupt_enable();
        let low_byte_address = high_byte_address.wrapping_sub(1);
        let flags_before_low_push = self.address_bus.interrupt_flags();
        self.store_byte_in_memory(low_byte_address, (program_counter & 0xFF) as u8);
        self.check_oam_bug_write(low_byte_address);
        self.registers.stack_pointer = low_byte_address;

        let flags = if low_byte_address == INTERRUPT_FLAGS_ADDRESS {
            flags_before_low_push
        } else {
            self.address_bus.interrupt_flags()
        };
        let interrupt = pick_interrupt(enabled, flags);

        // The request is cleared halfway through the fetch. A DMG's timer
        // requests an interrupt late in the M-cycle, after that (Gambatte's tima
        // and serial late_retrigger tests), like when it samples interrupts in
        // HALT.
        let dmg = !self.address_bus.cgb_mode();
        let half_cycle_dots = dots_per_m_cycle(self.address_bus.cgb_double_speed()) / 2;
        if dmg {
            self.address_bus.sync_until_mid_cycle();
        } else {
            self.address_bus.sync_except_last_dots(half_cycle_dots);
        }
        interrupt.inspect(|&interrupt| {
            self.address_bus.clear_interrupt_request(interrupt.bit());
        });
        if dmg {
            self.address_bus.finish_mid_cycle_sync();
        } else {
            self.address_bus.finish_sync(half_cycle_dots);
        }

        self.registers.program_counter = interrupt.map_or(0x0000, InterruptType::handler_address);
        self.registers.opcode = self.read_byte_in_synced_cycle(self.registers.program_counter);
        self.registers.program_counter = self.registers.program_counter.wrapping_add(1);
    }

    /// Whether an interrupt can be handled now: IME is on and an enabled
    /// interrupt is requested.
    fn can_dispatch_interrupt(&self) -> bool {
        self.ime == InterruptMasterEnable::On && self.has_pending_interrupt()
    }

    /// Runs one M-cycle of the HALT state and wakes the CPU if an interrupt is pending. When no
    /// interrupt is pending and none can come for a while, runs that whole stretch at once.
    pub(super) fn halted_step(&mut self) {
        self.address_bus.run_deferred_cycles_before_change();
        let just_halted = self.run_state == RunState::JustHalted;
        if !just_halted && !self.has_pending_interrupt() {
            let cycles = self.address_bus.bulk_cycles_available();
            if cycles > 0 {
                let sampled_mid_cycle = !self.address_bus.cgb_mode();
                self.address_bus.run_bulk_cycles(cycles, sampled_mid_cycle);
                return;
            }
        }
        let has_pending_interrupt = if just_halted || self.address_bus.cgb_mode() {
            let has_pending_interrupt = self.has_pending_interrupt();
            self.step_one_machine_cycle();
            has_pending_interrupt
        } else {
            self.step_one_machine_cycle_sampling_mid_cycle(Self::has_pending_interrupt)
        };
        if just_halted {
            // After a speed switch, OAM DMA still runs during the first halted
            // M-cycle and then stands still, like it does after HALT
            // (Gambatte's oamdmasrc80_halt tests).
            self.address_bus.dma_halt_started();
        }
        self.run_state = RunState::Halted;

        if has_pending_interrupt {
            self.address_bus.cancel_speed_switch_halt();
            self.wake_from_halt();
        } else if self.address_bus.take_speed_switch_halt_end() {
            self.leave_halt();
            if self.take_stop_opcode_kept() {
                self.registers.program_counter = self.registers.program_counter.wrapping_add(1);
            } else {
                self.fetch_opcode_after_halt();
            }
        }
    }

    /// Leaves the HALT state. With IME on the CPU handles the interrupt,
    /// otherwise it carries on with the instruction after HALT. An HBlank DMA
    /// block that is due when the CPU wakes up is copied first (Gambatte's
    /// hdma_transition_ei_halt_late_unhalt and late_hdma_vs_tima_halt tests),
    /// and the dispatch copies another one if it falls due meanwhile.
    fn wake_from_halt(&mut self) {
        self.leave_halt();
        let stop_opcode_kept = self.take_stop_opcode_kept();
        if self.ime == InterruptMasterEnable::On {
            if self.address_bus.hdma_block_pending() {
                self.address_bus.hdma_step();
            }
            self.dispatch_interrupt();
        } else if stop_opcode_kept || self.next_instruction == NextInstruction::HaltBugDuringHdma {
            // STOP or HALT already fetched this opcode.
            self.registers.program_counter = self.registers.program_counter.wrapping_add(1);
        } else {
            self.fetch_opcode_after_halt();
        }
    }

    /// Whether STOP kept the opcode to run once the CPU wakes up
    /// (`NextInstruction::StopOpcodeKept`), which this clears.
    fn take_stop_opcode_kept(&mut self) -> bool {
        let kept = self.next_instruction == NextInstruction::StopOpcodeKept;
        if kept {
            self.next_instruction = NextInstruction::Normal;
        }
        kept
    }

    /// Leaves HALT: the CPU runs again, OAM DMA paused by the HALT goes on,
    /// and `hdma_halt_ended` decides whether a pending HBlank DMA block is
    /// kept.
    fn leave_halt(&mut self) {
        self.run_state = RunState::Running;
        self.address_bus.hdma_halt_ended();
        self.address_bus.dma_halt_ended();
    }

    /// Reads the next opcode when the CPU leaves HALT without handling an
    /// interrupt, or when HALT doesn't halt. The M-cycle before already
    /// fetched it. Also used for the free second fetch of the byte after
    /// HALT when an HBlank DMA block fell due during HALT.
    pub(super) fn fetch_opcode_after_halt(&mut self) {
        let opcode = self
            .address_bus
            .cpu_read_byte(self.registers.program_counter);
        self.registers.opcode = opcode;
        self.registers.program_counter = self.registers.program_counter.wrapping_add(1);
    }

    /// Handles a pending interrupt after an opcode fetch, if IME allows it:
    /// the fetched opcode is discarded and the handler's first one fetched.
    pub(super) fn dispatch_interrupt_if_pending(&mut self) {
        if self.can_dispatch_interrupt() {
            self.registers.program_counter = self.registers.program_counter.wrapping_sub(1);
            self.dispatch_interrupt();
        }
    }
}
