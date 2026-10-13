//! Running many M-cycles at once.
//!
//! Every M-cycle moves the timers, the serial clock, the PPU and the APU on,
//! but as long as none of them can request an interrupt and nothing looks at
//! them, running a whole stretch of M-cycles in one go has exactly the result
//! of running them one by one, at a fraction of the cost. Two kinds of stretch
//! are run this way:
//!
//! - While the CPU is halted, waiting for an interrupt, the CPU has nothing to
//!   do until one can come (`bulk_cycles_available`, `run_bulk_cycles`).
//! - While the CPU runs, the M-cycles in which it only reads ROM or reads and
//!   writes work RAM and HRAM are only counted (`AddressBus::sync`). They run
//!   before the CPU touches anything else, before any M-cycle that needs more
//!   than a plain step (HALT and STOP among them), and at the end of each call
//!   into the emulator, so nothing outside the address bus sees them pending.

use crate::address_bus::AddressBus;
use crate::clock::{SYSTEM_COUNTER_TICKS_PER_M_CYCLE, dots_per_m_cycle};

impl AddressBus {
    /// How many M-cycles can run in one go, or 0 when they have to go one at a
    /// time. None of them may request an interrupt: the PPU
    /// stays within the dots it may be left behind (what `Gpu::run_dots` returns),
    /// TIMA doesn't overflow and no serial transfer that this side clocks is
    /// running. Nothing may need attention every M-cycle: OAM DMA, a speed
    /// switch, the PPU dots a register write owes, an APU that has to run
    /// M-cycle by M-cycle, or the APU's frame sequencer, which steps when a DIV
    /// bit changes. The audio buffer may only fill up in the last of them, so
    /// the emulator stops where it would have.
    pub(crate) fn bulk_cycles_available(&self) -> u32 {
        let ppu_cycles =
            self.ppu_quiet_dots.saturating_sub(self.ppu_owed_dots) / self.dots_per_cycle();
        if ppu_cycles == 0
            || super::EAGER
            || self.processor_test_mode()
            || !self.speed_switch.is_idle()
            || !self.dma.is_idle()
            || !self.gpu.oam_dma_access_settled()
            || self.ppu_dot_debt != 0
            || self.ppu_eager
            || self.pause_oam_dma_after_cycle
            || self.serial.clocks_transfer()
            || !self.apu.can_advance_in_bulk()
        {
            return 0;
        }
        let cycles_before_div_apu_edge = self.apu.cycles_until_div_apu_edge() - 1;
        ppu_cycles
            .min(cycles_before_div_apu_edge)
            .min(self.apu.cycles_until_catch_up())
            .min(self.timers.cycles_before_overflow())
    }

    /// Runs `cycles` M-cycles at once, which `bulk_cycles_available` allows.
    /// `sampled_mid_cycle` is set when the CPU
    /// samples interrupts halfway through each M-cycle (a halted DMG), where the
    /// timers only run in the second half, so the serial port sees the system
    /// counter as it was before each M-cycle.
    ///
    /// The timers and the serial clock count the edges of their system
    /// counter bits over the whole stretch, and the APU moves on with
    /// `Apu::advance_cycles`, since no DIV-APU edge (the system counter bit
    /// that clocks its frame sequencer) falls inside it.
    pub(crate) fn run_bulk_cycles(&mut self, cycles: u32, sampled_mid_cycle: bool) {
        if cycles == 0 {
            return;
        }
        let start = self.timers.system_counter();
        let end =
            start.wrapping_add((cycles as u16).wrapping_mul(SYSTEM_COUNTER_TICKS_PER_M_CYCLE));
        // No DIV-APU edge falls in these M-cycles, so the APU only needs to see
        // where the system counter ends up.
        self.apu.advance_cycles(cycles);
        self.apu.system_counter_changed(
            end.wrapping_sub(SYSTEM_COUNTER_TICKS_PER_M_CYCLE),
            end,
            false,
        );
        self.timers.run_cycles(cycles);
        let serial_first = if sampled_mid_cycle {
            start
        } else {
            start.wrapping_add(SYSTEM_COUNTER_TICKS_PER_M_CYCLE)
        };
        self.serial.step_many(serial_first, cycles);
        let dots = cycles * self.dots_per_cycle();
        self.elapsed_dots += u64::from(dots);
        self.ppu_owed_dots += dots;
    }

    /// How many of the coming plain M-cycles (see `sync`) may wait to run in one
    /// go: as many as `bulk_cycles_available` allows, except the M-cycle that
    /// fills the audio buffer, after which the emulator has to stop.
    pub(super) fn deferrable_cycles(&self) -> u32 {
        let cycles = self.bulk_cycles_available();
        if cycles == 0 {
            return 0;
        }
        cycles.min(self.apu.cycles_until_catch_up().saturating_sub(1))
    }

    /// Runs the plain M-cycles that were waiting, so the hardware is up to date.
    pub(crate) fn run_deferred_cycles(&mut self) {
        if self.deferred_cycles > 0 {
            let cycles = std::mem::take(&mut self.deferred_cycles);
            self.run_bulk_cycles(cycles, false);
        }
    }

    /// Runs the plain M-cycles that were waiting before something changes the
    /// hardware's state or an M-cycle runs on its own, after which no more may
    /// wait until `sync` works out again how many can.
    pub(crate) fn run_deferred_cycles_before_change(&mut self) {
        self.run_deferred_cycles();
        self.deferral_budget = 0;
    }

    /// PPU dots per M-cycle at the current speed.
    fn dots_per_cycle(&self) -> u32 {
        u32::from(dots_per_m_cycle(self.speed_switch.cgb_double_speed()))
    }
}
