use crate::utils::is_bit_set;
use crate::serializable::Serializable;
use crate::address_bus::MemoryMapped;
use getset::{CopyGetters, Setters};
use serializable_derive::Serializable;

#[derive(Serializable, CopyGetters, Setters)]
pub struct Serial {
    #[getset(get_copy = "pub(super)", set = "pub(super)")]
    data: u8,
    clock: u16,
    is_high_speed_clock: bool,
    is_master: bool,
    transfer_enabled: bool,
    bits_transferred: u8,
    cgb_mode: bool,
    cgb_double_speed: bool,
    #[getset(get_copy = "pub(super)", set = "pub(super)")]
    interrupt: bool
}

fn serial_disconnected_exchange(_: bool) -> bool {
    // When no device is connected, it will always read 1 for each bit transfer,
    // which means it will receive 0xFF bytes.
   true
}

impl Serial {
    pub(super) fn new() -> Self {
        Serial {
            data: 0,
            clock: 0,
            is_high_speed_clock: false,
            is_master: false,
            transfer_enabled: false,
            bits_transferred: 0,
            cgb_mode: false,
            cgb_double_speed: false,
            interrupt: false
        }
    }

    pub(super) fn set_cgb_mode(&mut self, cgb_mode: bool) {
        self.cgb_mode = cgb_mode;
    }

    pub(super) fn set_cgb_double_speed(&mut self, cgb_double_speed: bool) {
        self.cgb_double_speed = cgb_double_speed;
    }

    fn get_m_cycle_clock_rate(&self) -> u16 {
        if self.is_high_speed_clock && self.cgb_mode {
            if self.cgb_double_speed { 8 } else { 16 }
        } else {
            if self.cgb_double_speed { 256 } else { 512 }
        }
    }

    fn exchange_bits(&mut self) {
        let outgoing_bit = is_bit_set(self.data, 7);
        self.data <<= 1;
        let incoming_bit = serial_disconnected_exchange(outgoing_bit);
        if incoming_bit {
            self.data |= 1;
        }
    }

    /*
        This is a very bare bones serial implementation that always assumes there is no
        serial device connected and doesn't know how to operate in slave mode.
    */
    pub(super) fn step(&mut self) {
        if self.transfer_enabled && self.is_master {
            self.clock += 1;

            let clock_rate = self.get_m_cycle_clock_rate();
            if self.clock >= clock_rate {
                self.clock = 0;

                self.exchange_bits();

                self.bits_transferred += 1;
                if self.bits_transferred >= 8 {
                    self.transfer_enabled = false;
                    self.bits_transferred = 0;
                    self.interrupt = true;
                }
            }
        }
    }

    fn control(&self) -> u8 {
        let transfer_enabled_bit = if self.transfer_enabled { 1 } else { 0 };
        let high_speed_clock_bit = if self.is_high_speed_clock { 1 } else { 0 };
        let master_bit = if self.is_master { 1 } else { 0 };
        (transfer_enabled_bit << 7)
            | (high_speed_clock_bit << 1)
            | master_bit
    }

    fn set_control(&mut self, value: u8) {
        self.transfer_enabled = is_bit_set(value, 7);
        if self.cgb_mode {
            self.is_high_speed_clock = is_bit_set(value, 1);
        }
        self.is_master = is_bit_set(value, 0);
    }
}

impl MemoryMapped for Serial {
    fn read_byte(&self, address: u16) -> u8 {
        match address {
            0xFF01 => self.data,
            0xFF02 => self.control(),
            _ => panic!("Invalid Serial address: 0x{:04X}", address)
        }
    }

    fn write_byte(&mut self, address: u16, value: u8) {
        match address {
            0xFF01 => { self.data = value; },
            0xFF02 => self.set_control(value),
            _ => panic!("Invalid Serial address: 0x{:04X}", address)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_get_control_byte() {
        let mut serial = Serial::new();
        serial.cgb_mode = true;
        serial.is_high_speed_clock = true;
        serial.is_master = true;
        serial.transfer_enabled = true;
        assert_eq!(serial.control(), 0x83);
    }

    #[test]
    fn should_get_control_byte_when_high_speed_clock_disabled() {
        let mut serial = Serial::new();
        serial.cgb_mode = true;
        serial.is_high_speed_clock = false;
        serial.is_master = true;
        serial.transfer_enabled = true;
        assert_eq!(serial.control(), 0x81);
    }

    #[test]
    fn should_set_control_byte() {
        let mut serial = Serial::new();
        serial.cgb_mode = true;
        serial.set_control(0x83);
        assert_eq!(serial.is_high_speed_clock, true);
        assert_eq!(serial.is_master, true);
        assert_eq!(serial.transfer_enabled, true);
    }

    #[test]
    fn should_not_be_able_to_set_high_speed_clock_in_monochrome_mode() {
        let mut serial = Serial::new();
        serial.cgb_mode = false;
        serial.set_control(0x83);
        assert_eq!(serial.is_high_speed_clock, false);
    }

    #[test]
    fn should_increment_clock_while_transfer_in_progress() {
        let mut serial = Serial::new();
        serial.transfer_enabled = true;
        serial.is_master = true;
        serial.clock = 0;
        serial.step();
        assert_eq!(serial.clock, 1);
    }

    #[test]
    fn should_transfer_bit_when_512_cycles_have_passed() {
        let mut serial = Serial::new();
        serial.transfer_enabled = true;
        serial.is_master = true;
        serial.data = 0b10011010;
        serial.clock = 511;
        serial.step();
        assert_eq!(serial.clock, 0);
        assert_eq!(serial.data, 0b00110101);
    }

    #[test]
    fn should_transfer_bit_when_16_cycles_have_passed_if_high_speed_clock_enabled() {
        let mut serial = Serial::new();
        serial.cgb_mode = true;
        serial.is_high_speed_clock = true;
        serial.transfer_enabled = true;
        serial.is_master = true;
        serial.data = 0b10011010;
        serial.clock = 15;
        serial.step();
        assert_eq!(serial.clock, 0);
        assert_eq!(serial.data, 0b00110101);
    }

    #[test]
    fn should_transfer_bit_when_8_cycles_have_passed_if_high_speed_clock_and_double_speed_mode_are_enabled() {
        let mut serial = Serial::new();
        serial.cgb_mode = true;
        serial.cgb_double_speed = true;
        serial.is_high_speed_clock = true;
        serial.transfer_enabled = true;
        serial.is_master = true;
        serial.data = 0b10011010;
        serial.clock = 7;
        serial.step();
        assert_eq!(serial.clock, 0);
        assert_eq!(serial.data, 0b00110101);
    }

    #[test]
    fn should_complete_transfer_and_fire_interrupt_when_8_bits_have_been_transferred() {
        let mut serial = Serial::new();
        serial.transfer_enabled = true;
        serial.is_master = true;
        serial.bits_transferred = 7;
        serial.clock = 511;
        serial.step();
        assert_eq!(serial.transfer_enabled, false);
        assert_eq!(serial.interrupt, true);
    }
}
