/*
    The serial clock (a flip-flop toggled by a system counter bit, which SC writes pull low) is
    ported from SameBoy's timing.c and memory.c (https://github.com/LIJI32/SameBoy, Expat License,
    see the notice in apu.rs).
*/

//! The serial port, which the link cable plugs into.
//!
//! Two Game Boys exchange a byte by shifting it out one bit at a time while
//! shifting in the other side's byte. The side that drives the clock is the
//! "master"; it starts a transfer by writing SC. Nubo always behaves as
//! if no cable is connected: as master it receives 0xFF, and as the
//! externally clocked side its transfer never progresses.
//!
//! Registers: SB (0xFF01) holds the byte being shifted, SC (0xFF02) controls
//! the transfer.

use crate::address_bus::MemoryMapped;
use crate::clock::SYSTEM_COUNTER_TICKS_PER_M_CYCLE;
use crate::utils::{falling_edges, is_bit_set};
use getset::{CopyGetters, Setters};
use nubo_serializable_derive::Serializable;

/// The serial port's state.
#[derive(Debug, Clone, Default, Serializable, CopyGetters, Setters)]
pub struct Serial {
    /// SB: the byte being sent, which is replaced bit by bit by the byte received.
    #[getset(get_copy = "pub(super)", set = "pub(super)")]
    data: u8,
    /// SC bit 1: on the Game Boy Color, shift bits 32 times faster.
    is_high_speed_clock: bool,
    /// SC bit 0: this Game Boy drives the clock instead of the other side.
    uses_internal_clock: bool,
    /// SC bit 7: a transfer is in progress.
    transfer_enabled: bool,
    bits_transferred: u8,
    /// Whether the console is a CGB, where SC's clock speed bit exists. A copy
    /// of the address bus's, which only `AddressBus::set_model` changes.
    cgb_mode: bool,
    /// The system counter at the previous step, to find the falling edges of the bit that drives
    /// the serial clock. The address bus passes the counter after every M-cycle and every DIV
    /// reset (`step`, `step_many`).
    last_system_counter: u16,
    /// The internal serial clock. It toggles on each falling edge of the system counter bit
    /// selected by the clock speed and shifts a bit each time it falls, so a bit takes two of
    /// those edges. Writing SC pulls it low. Because it is a flip-flop of its own rather than a
    /// counter bit read directly, a transfer keeps its phase across DIV and SC writes.
    clock_high: bool,
    /// Set when a transfer completes, to request the serial interrupt.
    #[getset(get_copy = "pub(super)", set = "pub(super)")]
    interrupt: bool,
}

/// The bits of the system counter (the 16-bit counter whose top byte is DIV)
/// whose falling edges toggle the serial clock. A bit is shifted every other
/// edge: every 512 ticks of the system counter normally, every 16 with the
/// high speed clock.
const NORMAL_CLOCK_COUNTER_BIT: u16 = 1 << 7;
const HIGH_SPEED_CLOCK_COUNTER_BIT: u16 = 1 << 2;

/// The bit received for each bit sent when no cable is connected. The input
/// line floats high, so the other side always seems to send 1s.
fn exchange_bit_with_disconnected_cable(_outgoing_bit: bool) -> bool {
    true
}

impl Serial {
    pub(super) fn set_cgb_mode(&mut self, cgb_mode: bool) {
        self.cgb_mode = cgb_mode;
    }

    fn clock_counter_bit(&self) -> u16 {
        // Only a CGB can set the high speed bit (`set_control`).
        if self.is_high_speed_clock {
            HIGH_SPEED_CLOCK_COUNTER_BIT
        } else {
            NORMAL_CLOCK_COUNTER_BIT
        }
    }

    /// Shifts the top bit of SB out and the received bit in at the bottom.
    fn exchange_bit(&mut self) {
        let outgoing_bit = is_bit_set(self.data, 7);
        self.data <<= 1;
        let incoming_bit = exchange_bit_with_disconnected_cable(outgoing_bit);
        if incoming_bit {
            self.data |= 1;
        }
    }

    /// Lets the serial port see a new value of the system counter: after each
    /// M-cycle, and when a DIV write clears it. The internal clock is divided
    /// from that counter, and after eight bits the transfer ends and requests
    /// the serial interrupt.
    #[inline]
    pub(super) fn step(&mut self, system_counter: u16) {
        let falling_edges = self.last_system_counter & !system_counter;
        self.last_system_counter = system_counter;
        if falling_edges & self.clock_counter_bit() != 0 {
            self.toggle_clock();
        }
    }

    /// Whether a transfer is running that this side clocks, which shifts bits as the clock runs.
    pub(super) fn clocks_transfer(&self) -> bool {
        self.transfer_enabled && self.uses_internal_clock
    }

    /// Does what `steps` calls of `step` would, with the system counter counting up by one
    /// M-cycle's ticks from `first` between them. Only for when no transfer this side clocks is
    /// running, so the clock just toggles.
    pub(super) fn step_many(&mut self, first: u16, steps: u32) {
        if steps == 0 {
            return;
        }
        debug_assert!(!self.clocks_transfer());
        self.step(first);
        let toggles = falling_edges(
            first,
            SYSTEM_COUNTER_TICKS_PER_M_CYCLE,
            steps - 1,
            self.clock_counter_bit(),
        );
        if toggles % 2 == 1 {
            self.clock_high = !self.clock_high;
        }
        self.last_system_counter =
            first.wrapping_add(((steps - 1) as u16).wrapping_mul(SYSTEM_COUNTER_TICKS_PER_M_CYCLE));
    }

    /// Toggles the internal serial clock, shifting a bit when it falls during
    /// a transfer this side clocks.
    fn toggle_clock(&mut self) {
        self.clock_high = !self.clock_high;
        if !self.clock_high && self.transfer_enabled && self.uses_internal_clock {
            self.bits_transferred += 1;
            if self.bits_transferred == 8 {
                self.transfer_enabled = false;
                self.bits_transferred = 0;
                self.interrupt = true;
            }
            self.exchange_bit();
        }
    }

    /// The value of SC. The address bus fills in the unused bits.
    fn control(&self) -> u8 {
        let transfer_enabled_bit = u8::from(self.transfer_enabled);
        let high_speed_clock_bit = u8::from(self.is_high_speed_clock);
        let internal_clock_bit = u8::from(self.uses_internal_clock);
        (transfer_enabled_bit << 7) | (high_speed_clock_bit << 1) | internal_clock_bit
    }

    /// Handles a write to SC, which restarts the bit count and pulls the
    /// serial clock low (shifting a bit if the old SC was clocking a
    /// transfer). The high speed clock bit only exists on the Game Boy Color.
    fn set_control(&mut self, value: u8) {
        self.bits_transferred = 0;
        if self.clock_high {
            self.toggle_clock();
        }
        self.transfer_enabled = is_bit_set(value, 7);
        if self.cgb_mode {
            self.is_high_speed_clock = is_bit_set(value, 1);
        }
        self.uses_internal_clock = is_bit_set(value, 0);
    }
}

impl MemoryMapped for Serial {
    fn read_byte(&self, address: u16) -> u8 {
        match address {
            0xFF01 => self.data,
            0xFF02 => self.control(),
            _ => unreachable!(
                "the address bus only routes Serial addresses here, got 0x{address:04X}"
            ),
        }
    }

    fn write_byte(&mut self, address: u16, value: u8) {
        match address {
            0xFF01 => {
                self.data = value;
            }
            0xFF02 => self.set_control(value),
            _ => unreachable!(
                "the address bus only routes Serial addresses here, got 0x{address:04X}"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_get_control_byte() {
        let serial = Serial {
            cgb_mode: true,
            is_high_speed_clock: true,
            uses_internal_clock: true,
            transfer_enabled: true,
            ..Serial::default()
        };
        assert_eq!(serial.control(), 0x83);
    }

    #[test]
    fn should_get_control_byte_when_high_speed_clock_disabled() {
        let serial = Serial {
            cgb_mode: true,
            is_high_speed_clock: false,
            uses_internal_clock: true,
            transfer_enabled: true,
            ..Serial::default()
        };
        assert_eq!(serial.control(), 0x81);
    }

    #[test]
    fn should_set_control_byte() {
        let mut serial = Serial {
            cgb_mode: true,
            ..Serial::default()
        };
        serial.set_control(0x83);
        assert!(serial.is_high_speed_clock);
        assert!(serial.uses_internal_clock);
        assert!(serial.transfer_enabled);
    }

    #[test]
    fn should_not_be_able_to_set_high_speed_clock_in_monochrome_mode() {
        let mut serial = Serial::default();
        serial.set_control(0x83);
        assert!(!serial.is_high_speed_clock);
    }

    /// Counts the system counter up by `ticks`, one M-cycle at a time.
    fn step_counter(serial: &mut Serial, system_counter: &mut u16, ticks: u16) {
        for _ in 0..ticks / SYSTEM_COUNTER_TICKS_PER_M_CYCLE {
            *system_counter = system_counter.wrapping_add(SYSTEM_COUNTER_TICKS_PER_M_CYCLE);
            serial.step(*system_counter);
        }
    }

    #[test]
    fn should_transfer_bit_on_each_falling_edge_of_the_normal_clock() {
        let mut serial = Serial::default();
        let mut system_counter = 0;
        serial.set_control(0x81);
        serial.data = 0b1001_1010;

        step_counter(&mut serial, &mut system_counter, 508);
        assert_eq!(serial.data, 0b1001_1010);

        step_counter(&mut serial, &mut system_counter, 4);
        assert_eq!(serial.data, 0b0011_0101);
    }

    #[test]
    fn should_transfer_bit_every_sixteen_ticks_with_the_high_speed_clock() {
        let mut serial = Serial::default();
        let mut system_counter = 0;
        serial.cgb_mode = true;
        serial.set_control(0x83);
        serial.data = 0b1001_1010;

        step_counter(&mut serial, &mut system_counter, 16);
        assert_eq!(serial.data, 0b0011_0101);
    }

    #[test]
    fn should_complete_transfer_and_fire_interrupt_when_8_bits_have_been_transferred() {
        let mut serial = Serial::default();
        let mut system_counter = 0;
        serial.set_control(0x81);

        step_counter(&mut serial, &mut system_counter, 512 * 8);

        assert!(!serial.transfer_enabled);
        assert!(serial.interrupt);
        assert_eq!(serial.data, 0xFF);
    }

    #[test]
    fn writing_control_pulls_the_serial_clock_low() {
        let mut serial = Serial::default();
        let mut system_counter = 0;
        step_counter(&mut serial, &mut system_counter, 256);
        serial.set_control(0x81);
        serial.data = 0b1001_1010;

        step_counter(&mut serial, &mut system_counter, 508);
        assert_eq!(serial.data, 0b1001_1010);

        step_counter(&mut serial, &mut system_counter, 4);
        assert_eq!(serial.data, 0b0011_0101);
    }

    #[test]
    fn should_not_transfer_without_the_internal_clock() {
        let mut serial = Serial::default();
        let mut system_counter = 0;
        serial.set_control(0x80);
        serial.data = 0x12;

        step_counter(&mut serial, &mut system_counter, 512 * 8);

        assert_eq!(serial.data, 0x12);
    }
}
