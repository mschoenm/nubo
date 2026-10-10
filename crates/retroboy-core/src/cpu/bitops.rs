use crate::cpu::{Register, Cpu, REGISTER_HL};
use crate::utils::{is_bit_set, set_bit, reset_bit};

fn swap_nibbles(byte: u8) -> u8 {
    let first_nibble = (byte >> 4) & 0xF;
    let second_nibble = byte & 0xF;
    (second_nibble << 4) | first_nibble
}

impl Cpu {
    fn rotate_left(&mut self, byte: u8) -> u8 {
        let most_significant_bit = byte >> 7;
        let rotated_value = byte << 1 | most_significant_bit;
        self.set_flag_z(rotated_value == 0);
        self.set_flag_n(false);
        self.set_flag_h(false);
        self.set_flag_c(most_significant_bit == 0x01);
        rotated_value
    }

    fn rotate_left_through_carry(&mut self, byte: u8) -> u8 {
        let c_flag = if self.is_c_flag_set() { 0x1 } else { 0x0 };
        let most_significant_bit = byte >> 7;
        let rotated_value = byte << 1 | c_flag;
        self.set_flag_z(rotated_value == 0);
        self.set_flag_n(false);
        self.set_flag_h(false);
        self.set_flag_c(most_significant_bit == 0x01);
        rotated_value
    }

    fn rotate_right(&mut self, byte: u8) -> u8 {
        let least_significant_bit = byte & 0x1;
        let rotated_value: u8 = least_significant_bit << 7 | byte >> 1;
        self.set_flag_z(rotated_value == 0);
        self.set_flag_n(false);
        self.set_flag_h(false);
        self.set_flag_c(least_significant_bit == 0x01);
        rotated_value
    }

    fn rotate_right_through_carry(&mut self, byte: u8) -> u8 {
        let c_flag = if self.is_c_flag_set() { 0x1 } else { 0x0 };
        let least_significant_bit = byte & 0x1;
        let rotated_value = c_flag << 7 | byte >> 1;
        self.set_flag_z(rotated_value == 0);
        self.set_flag_n(false);
        self.set_flag_h(false);
        self.set_flag_c(least_significant_bit == 0x01);
        rotated_value
    }

    pub(super) fn rotate_register_left(&mut self, register: Register) {
        let byte = self.read_from_register(&register);
        let rotated_value = self.rotate_left(byte);
        self.store_in_register(register, rotated_value);
    }

    pub(super) fn rotate_register_left_through_carry(&mut self, register: Register) {
        let byte = self.read_from_register(&register);
        let rotated_value = self.rotate_left_through_carry(byte);
        self.store_in_register(register, rotated_value);
    }

    pub(super) fn rotate_register_right(&mut self, register: Register) {
        let byte = self.read_from_register(&register);
        let rotated_value = self.rotate_right(byte);
        self.store_in_register(register, rotated_value);
    }

    pub(super) fn rotate_register_right_through_carry(&mut self, register: Register) {
        let byte = self.read_from_register(&register);
        let rotated_value = self.rotate_right_through_carry(byte);
        self.store_in_register(register, rotated_value);
    }

    pub(super) fn rotate_memory_byte_left(&mut self) {
        let address = self.read_from_register_pair(&REGISTER_HL);
        let byte = self.read_byte_from_memory(address);
        let rotated_value = self.rotate_left(byte);
        self.store_byte_in_memory(address, rotated_value);
    }

    pub(super) fn rotate_memory_byte_left_through_carry(&mut self) {
        let address = self.read_from_register_pair(&REGISTER_HL);
        let byte = self.read_byte_from_memory(address);
        let rotated_value = self.rotate_left_through_carry(byte);
        self.store_byte_in_memory(address, rotated_value);
    }

    pub(super) fn rotate_memory_byte_right(&mut self) {
        let address = self.read_from_register_pair(&REGISTER_HL);
        let byte = self.read_byte_from_memory(address);
        let rotated_value = self.rotate_right(byte);
        self.store_byte_in_memory(address, rotated_value);
    }

    pub(super) fn rotate_memory_byte_right_through_carry(&mut self) {
        let address = self.read_from_register_pair(&REGISTER_HL);
        let byte = self.read_byte_from_memory(address);
        let rotated_value = self.rotate_right_through_carry(byte);
        self.store_byte_in_memory(address, rotated_value);
    }

    pub(super) fn swap_nibbles_in_register(&mut self, register: Register) {
        let byte = self.read_from_register(&register);
        let with_swapped_nibbles = swap_nibbles(byte);

        self.store_in_register(register, with_swapped_nibbles);

        self.set_flag_z(with_swapped_nibbles == 0);
        self.set_flag_n(false);
        self.set_flag_h(false);
        self.set_flag_c(false); 
    }

    pub(super) fn swap_nibbles_in_memory_byte(&mut self, address: u16) {
        let byte = self.read_byte_from_memory(address);
        let with_swapped_nibbles = swap_nibbles(byte);

        self.store_byte_in_memory(address, with_swapped_nibbles);

        self.set_flag_z(with_swapped_nibbles == 0);
        self.set_flag_n(false);
        self.set_flag_h(false);
        self.set_flag_c(false);
    }

    fn shift_left(&mut self, byte: u8) -> u8 {
        let most_significant_bit = byte >> 7;
        let shifted_value = byte << 1;
        self.set_flag_z(shifted_value == 0);
        self.set_flag_n(false);
        self.set_flag_h(false);
        self.set_flag_c(most_significant_bit == 0x01);
        shifted_value
    }

    pub(super) fn shift_register_left(&mut self, register: Register) {
        let byte = self.read_from_register(&register);
        let shifted_value = self.shift_left(byte);
        self.store_in_register(register, shifted_value);
    }

    pub(super) fn shift_memory_byte_left(&mut self) {
        let address = self.read_from_register_pair(&REGISTER_HL);
        let byte = self.read_byte_from_memory(address);
        let shifted_value = self.shift_left(byte);
        self.store_byte_in_memory(address, shifted_value);
    }

    fn shift_right(&mut self, byte: u8, maintain_msb: bool) -> u8 {
        let least_significant_bit = byte & 0x1;
        let updated_most_significant_bit = if maintain_msb { byte & 0x80 } else { 0 };
        let shifted_value = byte >> 1 | updated_most_significant_bit;
        self.set_flag_z(shifted_value == 0);
        self.set_flag_n(false);
        self.set_flag_h(false);
        self.set_flag_c(least_significant_bit == 0x01);
        shifted_value
    }

    pub(super) fn shift_register_right_maintaining_msb(&mut self, register: Register) {
        let byte = self.read_from_register(&register);
        let shifted_value = self.shift_right(byte, true);
        self.store_in_register(register, shifted_value);
    }

    pub(super) fn shift_memory_byte_right_maintaining_msb(&mut self) {
        let address = self.read_from_register_pair(&REGISTER_HL);
        let byte = self.read_byte_from_memory(address);
        let shifted_value = self.shift_right(byte, true);
        self.store_byte_in_memory(address, shifted_value);
    }

    pub(super) fn shift_register_right(&mut self, register: Register) {
        let byte = self.read_from_register(&register);
        let shifted_value = self.shift_right(byte, false);
        self.store_in_register(register, shifted_value);
    }

    pub(super) fn shift_memory_byte_right(&mut self) {
        let address = self.read_from_register_pair(&REGISTER_HL);
        let byte = self.read_byte_from_memory(address);
        let shifted_value = self.shift_right(byte, false);
        self.store_byte_in_memory(address, shifted_value);
    }

    fn test_bit(&mut self, byte: u8, bit_index: u8) {
        self.set_flag_z(!is_bit_set(byte, bit_index));
        self.set_flag_n(false);
        self.set_flag_h(true);
    }

    pub(super) fn test_register_bit(&mut self, register: Register, bit_index: u8) {
        let byte = self.read_from_register(&register);
        self.test_bit(byte, bit_index);
    }

    pub(super) fn test_memory_bit(&mut self, bit_index: u8) {
        let address = self.read_from_register_pair(&REGISTER_HL);
        let byte = self.read_byte_from_memory(address);
        self.test_bit(byte, bit_index);
    }

    pub(super) fn reset_register_bit(&mut self, register: Register, bit_index: u8) {
        let byte = self.read_from_register(&register);
        let updated_byte = reset_bit(byte, bit_index);
        self.store_in_register(register, updated_byte);
    }

    pub(super) fn reset_memory_bit(&mut self, bit_index: u8) {
        let address = self.read_from_register_pair(&REGISTER_HL);
        let byte = self.read_byte_from_memory(address);
        let updated_byte = reset_bit(byte, bit_index);
        self.store_byte_in_memory(address, updated_byte);
    }

    pub(super) fn set_register_bit(&mut self, register: Register, bit_index: u8) {
        let byte = self.read_from_register(&register);
        let updated_byte = set_bit(byte, bit_index);
        self.store_in_register(register, updated_byte);
    }

    pub(super) fn set_memory_bit(&mut self, bit_index: u8) {
        let address = self.read_from_register_pair(&REGISTER_HL);
        let byte = self.read_byte_from_memory(address);
        let updated_byte = set_bit(byte, bit_index);
        self.store_byte_in_memory(address, updated_byte);
    }
}