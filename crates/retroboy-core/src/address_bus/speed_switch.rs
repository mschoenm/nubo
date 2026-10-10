use crate::address_bus::AddressBus;
use crate::utils::is_bit_set;
use crate::serializable::Serializable;
use getset::{CopyGetters, Setters};
use serializable_derive::Serializable;

#[derive(Serializable, CopyGetters, Setters)]
#[getset(get_copy = "pub(crate)", set = "pub(crate)")]
pub(crate) struct SpeedSwitch {
    cgb_double_speed: bool,
    cgb_mode: bool,
    armed: bool
}

const SPEED_SWITCH_ARMED_INDEX: u8 = 0;

impl SpeedSwitch {
    pub(super) fn new() -> Self {
        SpeedSwitch {
            cgb_double_speed: false,
            cgb_mode: false,
            armed: false
        }
    }

    pub(super) fn key1(&self) -> u8 {
        if self.cgb_mode {
            let double_speed_bit = if self.cgb_double_speed { 1 } else { 0 };
            let speed_switch_armed_bit = if self.armed { 1 } else { 0 };
            double_speed_bit << 7 | speed_switch_armed_bit
        }
        else {
            0xFF
        }
    }

    pub(super) fn set_key1(&mut self, value: u8) {
        if self.cgb_mode {
            self.armed = is_bit_set(value, SPEED_SWITCH_ARMED_INDEX);
        }
    }
}

impl AddressBus {
    pub(crate) fn toggle_speed_switch(&mut self) {
        if self.cgb_mode && self.speed_switch.armed() {
            self.speed_switch.set_armed(false);

            let new_cgb_double_speed = !self.speed_switch.cgb_double_speed();
            self.speed_switch.set_cgb_double_speed(new_cgb_double_speed);
            self.apu.set_cgb_double_speed(new_cgb_double_speed);
            self.gpu.set_cgb_double_speed(new_cgb_double_speed);
            self.serial.set_cgb_double_speed(new_cgb_double_speed);
            self.hdma.set_cgb_double_speed(new_cgb_double_speed);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::address_bus::test_utils::initialize_test_address_bus;
    use super::*;

    #[test]
    fn should_read_from_key1_in_double_seed_mode() {
        let mut speed_switch = SpeedSwitch::new();
        speed_switch.set_cgb_mode(true);
        speed_switch.set_cgb_double_speed(true);
        assert_eq!(speed_switch.key1(), 0x80);
    }

    #[test]
    fn should_read_from_key1_while_speed_switch_is_armed() {
        let mut speed_switch = SpeedSwitch::new();
        speed_switch.set_cgb_mode(true);
        speed_switch.set_armed(true);
        assert_eq!(speed_switch.key1(), 0x1);
    }

    #[test]
    fn should_write_to_key1() {
        let mut speed_switch = SpeedSwitch::new();
        speed_switch.set_cgb_mode(true);
        speed_switch.set_key1(0x1);
        assert_eq!(speed_switch.armed(), true);
    }

    #[test]
    fn should_toggle_speed_switch() {
        let mut address_bus = initialize_test_address_bus();
        address_bus.cgb_mode = true;
        address_bus.speed_switch.set_cgb_mode(true);
        address_bus.speed_switch.set_armed(true);
        address_bus.speed_switch.set_cgb_double_speed(false);
        address_bus.toggle_speed_switch();
        assert_eq!(address_bus.speed_switch.armed(), false);
        assert_eq!(address_bus.speed_switch.cgb_double_speed(), true);
    }

    #[test]
    fn should_not_toggle_if_not_armed() {
        let mut address_bus = initialize_test_address_bus();
        address_bus.cgb_mode = true;
        address_bus.speed_switch.set_cgb_mode(true);
        address_bus.speed_switch.set_armed(false);
        address_bus.speed_switch.set_cgb_double_speed(false);
        address_bus.toggle_speed_switch();
        assert_eq!(address_bus.speed_switch.armed(), false);
        assert_eq!(address_bus.speed_switch.cgb_double_speed(), false);
    }
}
