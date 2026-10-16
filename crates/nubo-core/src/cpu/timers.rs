//! The timer: DIV (0xFF04), TIMA (0xFF05), TMA (0xFF06) and TAC (0xFF07).
//!
//! A 16-bit system counter counts the ticks of the CPU's clock, four per
//! M-cycle (see `clock.rs`); DIV shows its upper byte. TIMA counts up when the system counter bit selected by TAC falls from
//! 1 to 0, and when it overflows it is reloaded from TMA and requests the timer
//! interrupt. Because TIMA reacts to falling edges, writes to DIV or TAC that
//! drop the selected bit also count it up, as on hardware.

use crate::address_bus::MemoryMapped;
use crate::clock::SYSTEM_COUNTER_TICKS_PER_M_CYCLE;
use crate::model::Model;
use crate::utils::falling_edges;
use nubo_serializable_derive::Serializable;

const TIMER_ENABLE_BIT: u8 = 0x04;
const UNUSED_CONTROL_BITS: u8 = 0xF8;

/// The timer's registers and internal state. TIMA, TMA and TAC change through
/// `write_byte`, which handles the reload and the falling edges a write can
/// cause.
#[derive(Clone, Default, Serializable, Debug)]
pub struct TimerRegisters {
    /// The internal counter that DIV is the upper byte of. The APU and the
    /// serial port keep copies to find the edges of its bits; the address bus
    /// tells them about every change (`AddressBus::step_timers`,
    /// `AddressBus::reset_divider`, `AddressBus::run_bulk_cycles`,
    /// `AddressBus::set_post_boot_system_counter`).
    system_counter: u16,
    /// TIMA, the timer counter.
    counter: u8,
    /// TMA, the value TIMA is reloaded with when it overflows.
    modulo: u8,
    /// TAC, which turns the timer on (bit 2) and selects its speed (bits 0-1).
    control: u8,
    /// Whether the timer interrupt is requested (bit 2 of IF).
    pub(crate) interrupt: bool,
    /// TIMA overflowed during the last M-cycle and reads 0 until it is reloaded
    /// in the next one.
    reload_pending: bool,
    /// TIMA is reloaded from TMA during the current M-cycle.
    reloading: bool,
}

impl TimerRegisters {
    pub(crate) fn system_counter(&self) -> u16 {
        self.system_counter
    }

    /// DIV, the upper byte of the system counter.
    pub(crate) fn divider(&self) -> u8 {
        (self.system_counter >> 8) as u8
    }

    /// The system counter bit that TAC's speed setting selects.
    fn selected_counter_bit(&self) -> u16 {
        match self.control & 0x03 {
            0x00 => 1 << 9,
            0x01 => 1 << 3,
            0x02 => 1 << 5,
            _ => 1 << 7,
        }
    }

    /// The signal whose falling edge counts TIMA up: the selected system
    /// counter bit, while the timer is enabled.
    fn timer_signal(&self) -> bool {
        self.control & TIMER_ENABLE_BIT != 0
            && self.system_counter & self.selected_counter_bit() != 0
    }

    /// Counts TIMA up when the timer signal fell from `previous_signal` to
    /// low; an overflow reloads TIMA from TMA on the next M-cycle.
    fn increment_counter_on_falling_edge(&mut self, previous_signal: bool) {
        if previous_signal && !self.timer_signal() {
            let (counter, overflowed) = self.counter.overflowing_add(1);
            self.counter = counter;
            self.reload_pending = overflowed;
        }
    }

    /// Advances the timer by one M-cycle.
    #[inline]
    pub(crate) fn step(&mut self) {
        self.reloading = false;
        if self.reload_pending {
            self.reload_pending = false;
            self.reloading = true;
            self.counter = self.modulo;
            self.interrupt = true;
        }

        let previous_signal = self.timer_signal();
        self.system_counter = self
            .system_counter
            .wrapping_add(SYSTEM_COUNTER_TICKS_PER_M_CYCLE);
        self.increment_counter_on_falling_edge(previous_signal);
    }

    /// How many M-cycles the timer can run before TIMA overflows: `u32::MAX` while it is stopped,
    /// 0 while a reload is due.
    pub(crate) fn cycles_before_overflow(&self) -> u32 {
        if self.reload_pending {
            return 0;
        }
        if self.control & TIMER_ENABLE_BIT == 0 {
            return u32::MAX;
        }
        // The selected bit falls each time the counter passes a multiple of twice the bit.
        let period_shift = self.selected_counter_bit().trailing_zeros() + 1;
        let counter = u32::from(self.system_counter);
        let edges_left = 255 - u32::from(self.counter);
        let overflow_at = ((counter >> period_shift) + edges_left + 1) << period_shift;
        (overflow_at - counter).div_ceil(u32::from(SYSTEM_COUNTER_TICKS_PER_M_CYCLE)) - 1
    }

    /// Runs `cycles` M-cycles at once, which must not make TIMA overflow (see
    /// `cycles_before_overflow`).
    pub(crate) fn run_cycles(&mut self, cycles: u32) {
        if cycles == 0 {
            return;
        }
        debug_assert!(cycles <= self.cycles_before_overflow());
        self.reloading = false;
        if self.control & TIMER_ENABLE_BIT != 0 {
            let edges = falling_edges(
                self.system_counter,
                SYSTEM_COUNTER_TICKS_PER_M_CYCLE,
                cycles,
                self.selected_counter_bit(),
            );
            self.counter += u8::try_from(edges).expect("TIMA doesn't overflow");
        }
        self.system_counter = self
            .system_counter
            .wrapping_add((cycles as u16).wrapping_mul(SYSTEM_COUNTER_TICKS_PER_M_CYCLE));
    }

    /// Moves the system counter without counting, which only the boot ROM
    /// exit does (see `AddressBus::set_post_boot_system_counter` for who sees
    /// it).
    pub(crate) fn set_system_counter(&mut self, value: u16) {
        self.system_counter = value;
    }

    /// Clears the system counter as STOP does when it switches the CGB's
    /// speed. At the fast TAC speeds the timer first sees the counter four
    /// clock units ahead, so it may count once more: CGB-B and CGB-C do this at
    /// 262, 65 and 16 kHz (Gambatte's speedchange_tima tests), CGB-E only at
    /// 262 kHz (AGE's spsw-tima tests). Only TIMA sees that jump: for DIV, the
    /// APU and the serial port the counter goes straight to 0.
    pub(crate) fn reset_divider_for_speed_switch(&mut self, model: Model) {
        let speed = self.control & 0x03;
        let jumps_ahead = match speed {
            0 => false,
            1 => true,
            _ => model < Model::CgbE,
        };
        if self.control & TIMER_ENABLE_BIT != 0 && jumps_ahead {
            let previous_signal = self.timer_signal();
            self.system_counter = self
                .system_counter
                .wrapping_add(SYSTEM_COUNTER_TICKS_PER_M_CYCLE);
            self.increment_counter_on_falling_edge(previous_signal);
        }
        self.reset_divider();
    }

    /// Clears the system counter, as any write to DIV does.
    pub(crate) fn reset_divider(&mut self) {
        let previous_signal = self.timer_signal();
        self.system_counter = 0;
        self.increment_counter_on_falling_edge(previous_signal);
    }

    /// Writes TIMA. A write in the M-cycle after an overflow cancels the reload,
    /// and a write in the M-cycle of the reload is ignored.
    fn write_counter(&mut self, value: u8) {
        if !self.reloading {
            self.counter = value;
            self.reload_pending = false;
        }
    }

    /// Writes TMA. During the M-cycle of a reload the new value also goes into TIMA.
    fn write_modulo(&mut self, value: u8) {
        self.modulo = value;
        if self.reloading {
            self.counter = value;
        }
    }

    /// Writes TAC. Turning the timer off or picking another counter bit can
    /// make the timer signal fall, which counts TIMA up like a tick would.
    fn write_control(&mut self, value: u8) {
        let previous_signal = self.timer_signal();
        self.control = value & !UNUSED_CONTROL_BITS;
        self.increment_counter_on_falling_edge(previous_signal);
    }
}

impl MemoryMapped for TimerRegisters {
    fn read_byte(&self, address: u16) -> u8 {
        match address {
            0xFF04 => self.divider(),
            0xFF05 => self.counter,
            0xFF06 => self.modulo,
            0xFF07 => self.control | UNUSED_CONTROL_BITS,
            _ => unreachable!(
                "the address bus only routes Timer addresses here, got 0x{address:04X}"
            ),
        }
    }

    fn write_byte(&mut self, address: u16, value: u8) {
        match address {
            0xFF04 => self.reset_divider(),
            0xFF05 => self.write_counter(value),
            0xFF06 => self.write_modulo(value),
            0xFF07 => self.write_control(value),
            _ => unreachable!(
                "the address bus only routes Timer addresses here, got 0x{address:04X}"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step_times(timers: &mut TimerRegisters, m_cycles: usize) {
        for _ in 0..m_cycles {
            timers.step();
        }
    }

    #[test]
    fn increments_divider_every_sixty_four_m_cycles() {
        let mut timers = TimerRegisters::default();
        step_times(&mut timers, 63);
        assert_eq!(timers.divider(), 0);
        timers.step();
        assert_eq!(timers.divider(), 1);
    }

    #[test]
    fn wraps_when_divider_overflows() {
        let mut timers = TimerRegisters::default();
        step_times(&mut timers, 64 * 256);
        assert_eq!(timers.divider(), 0);
    }

    #[test]
    fn writing_divider_resets_it() {
        let mut timers = TimerRegisters::default();
        step_times(&mut timers, 200);
        timers.write_byte(0xFF04, 0x45);
        assert_eq!(timers.divider(), 0);
        assert_eq!(timers.system_counter, 0);
    }

    #[test]
    fn increments_counter_at_each_configured_rate() {
        for (control, m_cycles_per_increment) in [(0x04, 256), (0x05, 4), (0x06, 16), (0x07, 64)] {
            let mut timers = TimerRegisters::default();
            timers.write_byte(0xFF07, control);
            step_times(&mut timers, m_cycles_per_increment - 1);
            assert_eq!(timers.counter, 0);
            timers.step();
            assert_eq!(timers.counter, 1);
        }
    }

    #[test]
    fn does_not_increment_counter_while_disabled() {
        let mut timers = TimerRegisters::default();
        timers.write_byte(0xFF07, 0x01);
        step_times(&mut timers, 100);
        assert_eq!(timers.counter, 0);
    }

    #[test]
    fn reloads_modulo_and_requests_interrupt_one_m_cycle_after_overflow() {
        let mut timers = TimerRegisters::default();
        timers.write_byte(0xFF06, 0x04);
        timers.write_byte(0xFF05, 0xFF);
        timers.write_byte(0xFF07, 0x05);
        step_times(&mut timers, 4);
        assert_eq!(timers.counter, 0);
        assert!(!timers.interrupt);

        timers.step();
        assert_eq!(timers.counter, 0x04);
        assert!(timers.interrupt);
    }

    #[test]
    fn writing_counter_before_the_reload_cancels_it() {
        let mut timers = TimerRegisters::default();
        timers.write_byte(0xFF06, 0x04);
        timers.write_byte(0xFF05, 0xFF);
        timers.write_byte(0xFF07, 0x05);
        step_times(&mut timers, 4);
        timers.write_byte(0xFF05, 0x80);

        timers.step();
        assert_eq!(timers.counter, 0x80);
        assert!(!timers.interrupt);
    }

    #[test]
    fn writing_counter_during_the_reload_is_ignored() {
        let mut timers = TimerRegisters::default();
        timers.write_byte(0xFF06, 0x04);
        timers.write_byte(0xFF05, 0xFF);
        timers.write_byte(0xFF07, 0x05);
        step_times(&mut timers, 5);
        timers.write_byte(0xFF05, 0x80);
        assert_eq!(timers.counter, 0x04);
    }

    #[test]
    fn writing_modulo_during_the_reload_also_updates_the_counter() {
        let mut timers = TimerRegisters::default();
        timers.write_byte(0xFF06, 0x04);
        timers.write_byte(0xFF05, 0xFF);
        timers.write_byte(0xFF07, 0x05);
        step_times(&mut timers, 5);
        timers.write_byte(0xFF06, 0x80);
        assert_eq!(timers.counter, 0x80);
    }

    #[test]
    fn resetting_divider_on_a_high_signal_increments_the_counter() {
        let mut timers = TimerRegisters::default();
        timers.write_byte(0xFF07, 0x05);
        step_times(&mut timers, 2);
        timers.write_byte(0xFF04, 0);
        assert_eq!(timers.counter, 1);
    }

    #[test]
    fn disabling_the_timer_on_a_high_signal_increments_the_counter() {
        let mut timers = TimerRegisters::default();
        timers.write_byte(0xFF07, 0x05);
        step_times(&mut timers, 2);
        timers.write_byte(0xFF07, 0x01);
        assert_eq!(timers.counter, 1);
    }

    #[test]
    fn reads_unused_control_bits_as_set() {
        let mut timers = TimerRegisters::default();
        timers.write_byte(0xFF07, 0x05);
        assert_eq!(timers.read_byte(0xFF07), 0xFD);
    }
}
