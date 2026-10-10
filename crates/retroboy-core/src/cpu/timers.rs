use crate::address_bus::MemoryMapped;
use crate::serializable::Serializable;
use getset::{CopyGetters, Setters};
use serializable_derive::Serializable;

const BASE_SPEED_RATE: u8 = 4;
const DIVIDER_RATE: u8 = 16;

#[derive(Serializable, Debug, CopyGetters, Setters)]
pub struct TimerRegisters {
    m_cycles_clock: u8,
    divider_clock: u8,
    base_clock: u8,
    #[getset(get_copy = "pub(crate)")]
    divider: u8,
    #[getset(get_copy = "pub(crate)", set = "pub(crate)")]
    counter: u8,
    #[getset(get_copy = "pub(crate)", set = "pub(crate)")]
    modulo: u8,
    #[getset(get_copy = "pub(crate)", set = "pub(crate)")]
    control: u8,
    #[getset(get_copy = "pub(crate)", set = "pub(crate)")]
    interrupt: bool
}

impl TimerRegisters {
    pub(crate) fn new() -> Self {
        TimerRegisters {
            m_cycles_clock: 0,
            divider_clock: 0,
            base_clock: 0,
            divider: 0,
            counter: 0,
            modulo: 0,
            control: 0,
            interrupt: false
        }
    }

    fn get_counter_rate(&self) -> Option<u8> {
        match self.control & 0x07 {
            0x04 => Some(64),
            0x05 => Some(1),
            0x06 => Some(4),
            0x7 => Some(16),
            _ => None
        }
    }

    fn increment_div_register(&mut self) {
        self.divider_clock += 1;

        if self.divider_clock >= DIVIDER_RATE {
            self.divider = self.divider.wrapping_add(1);
            self.divider_clock = 0;
        }
    }

    fn increment_counter_register(&mut self, counter_rate: u8) {
        self.base_clock += 1;

        if self.base_clock >= counter_rate {
            self.base_clock = 0;

            if self.counter == 0xFF {
                self.counter = self.modulo;
                self.interrupt = true;
            }
            else {
                self.counter += 1
            }
        }
    }

    pub(crate) fn step(&mut self) {
        self.m_cycles_clock += 1;

        if self.m_cycles_clock >= BASE_SPEED_RATE {
            self.m_cycles_clock -= BASE_SPEED_RATE;

            self.increment_div_register();

            match self.get_counter_rate() {
                Some(counter_rate) => {
                    self.increment_counter_register(counter_rate);
                }
                _ => ()
            }
        }
    }

    pub(crate) fn set_divider(&mut self, value: u8) {
        self.divider = value;
        self.divider_clock = 0;
        self.m_cycles_clock = 0;
    }
}

impl MemoryMapped for TimerRegisters {
    fn read_byte(&self, address: u16) -> u8 {
        match address {
            0xFF04 => self.divider,
            0xFF05 => self.counter,
            0xFF06 => self.modulo,
            0xFF07 => self.control,
            _ => panic!("Invalid Timer address: 0x{:04X}", address)
        }
    }

    fn write_byte(&mut self, address: u16, value: u8) {
        match address {
            0xFF04 => self.set_divider(value),
            0xFF05 => { self.counter = value; },
            0xFF06 => { self.modulo = value; },
            0xFF07 => { self.control = value; },
            _ => panic!("Invalid Timer address: 0x{:04X}", address)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increments_base_speed_by_number_of_m_cycles() {
        let mut timers = TimerRegisters::new();
        timers.step();
        assert_eq!(timers.m_cycles_clock, 1);
    }

    #[test]
    fn resets_base_speed_after_four_m_cycles() {
        let mut timers = TimerRegisters::new();
        timers.m_cycles_clock = 3;
        timers.step();
        assert_eq!(timers.m_cycles_clock, 0);
    }

    #[test]
    fn increments_divider_clock_after_four_m_cycles() {
        let mut timers = TimerRegisters::new();
        timers.m_cycles_clock = 3;
        timers.step();
        assert_eq!(timers.divider_clock, 1);
    }

    #[test]
    fn increments_divider_register_after_sixteen_divider_clock_increments() {
        let mut timers = TimerRegisters::new();
        timers.m_cycles_clock = 3;
        timers.divider_clock = 15;
        timers.step();
        assert_eq!(timers.divider, 1);
        assert_eq!(timers.divider_clock, 0);
    }

    #[test]
    fn wraps_when_divider_register_overflows() {
        let mut timers = TimerRegisters::new();
        timers.m_cycles_clock = 3;
        timers.divider_clock = 15;
        timers.divider = 0xFF;
        timers.step();
        assert_eq!(timers.divider, 0);
        assert_eq!(timers.divider_clock, 0);
    }

    #[test]
    fn increments_counter_register_at_a_fourth_the_rate_of_base_speed_when_configured() {
        let mut timers = TimerRegisters::new();
        timers.m_cycles_clock = 3;
        timers.base_clock = 3;
        timers.control = 0x06;
        timers.step();
        assert_eq!(timers.counter, 1);
        assert_eq!(timers.base_clock, 0);
    }

    #[test]
    fn increments_counter_register_at_a_sixteenth_of_the_rate_of_base_speed_when_configured() {
        let mut timers = TimerRegisters::new();
        timers.m_cycles_clock = 3;
        timers.base_clock = 15;
        timers.control = 0x07;
        timers.step();
        assert_eq!(timers.counter, 1);
        assert_eq!(timers.base_clock, 0);
    }

    #[test]
    fn increments_counter_register_at_a_sixty_fourth_of_the_rate_of_base_speed_when_configured() {
        let mut timers = TimerRegisters::new();
        timers.m_cycles_clock = 3;
        timers.base_clock = 63;
        timers.control = 0x04;
        timers.step();
        assert_eq!(timers.counter, 1);
        assert_eq!(timers.base_clock, 0);
    }

    #[test]
    fn increments_counter_register_at_same_rate_of_base_speed_when_configured() {
        let mut timers = TimerRegisters::new();
        timers.m_cycles_clock = 3;
        timers.base_clock = 0;
        timers.control = 0x05;
        timers.step();
        assert_eq!(timers.counter, 1);
        assert_eq!(timers.base_clock, 0);
    }

    #[test]
    fn should_not_increment_counter_register_at_wrong_time() {
        let mut timers = TimerRegisters::new();
        timers.m_cycles_clock = 3;
        timers.base_clock = 14;
        timers.control = 0x07;
        timers.step();
        assert_eq!(timers.counter, 0);
        assert_eq!(timers.base_clock, 15);
    }

    #[test]
    fn should_not_increment_counter_register_if_timer_is_off() {
        let mut timers = TimerRegisters::new();
        timers.m_cycles_clock = 3;
        timers.base_clock = 15;
        timers.control = 0;
        timers.step();
        assert_eq!(timers.counter, 0);
        assert_eq!(timers.base_clock, 15);
    }

    #[test]
    fn should_fire_interrupt_on_counter_register_overflow() {
        let mut timers = TimerRegisters::new();
        timers.m_cycles_clock = 3;
        timers.base_clock = 0x15;
        timers.control = 0x07;
        timers.counter = 0xFF;
        timers.step();
        assert_eq!(timers.counter, 0);
        assert_eq!(timers.base_clock, 0);
        assert_eq!(timers.interrupt, true);
    }

    #[test]
    fn should_reset_counter_register_to_modulo_on_overflow() {
        let mut timers = TimerRegisters::new();
        timers.m_cycles_clock = 3;
        timers.base_clock = 0x15;
        timers.control = 0x07;
        timers.counter = 0xFF;
        timers.modulo = 0x04;
        timers.step();
        assert_eq!(timers.counter, 0x04);
        assert_eq!(timers.base_clock, 0);
        assert_eq!(timers.interrupt, true);
    }
}
