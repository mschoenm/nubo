//! Bit instructions: rotates, shifts, nibble swaps and the single-bit
//! BIT/RES/SET instructions. Most of them are CB-prefixed opcodes and work
//! either on a register or on the byte at the address in HL.

use crate::cpu::{Cpu, REGISTER_HL, Register};
use crate::utils::{is_bit_set, reset_bit, set_bit};

impl Cpu {
    /// Replaces `register` with `operation` applied to it.
    fn update_register(&mut self, register: Register, operation: impl FnOnce(&mut Self, u8) -> u8) {
        let byte = self.read_from_register(register);
        let updated_byte = operation(self, byte);
        self.store_in_register(register, updated_byte);
    }

    /// Replaces the byte at the address in HL with `operation` applied to it,
    /// which takes two M-cycles: a read and a write.
    fn update_byte_at_hl(&mut self, operation: impl FnOnce(&mut Self, u8) -> u8) {
        let address = self.read_from_register_pair(REGISTER_HL);
        let byte = self.read_byte_from_memory(address);
        let updated_byte = operation(self, byte);
        self.store_byte_in_memory(address, updated_byte);
    }

    /// Sets the flags after a rotate, shift or swap: zero from the result,
    /// carry from the bit shifted out.
    fn set_shift_flags(&mut self, result: u8, carry: bool) {
        self.set_flag_z(result == 0);
        self.set_flag_n(false);
        self.set_flag_h(false);
        self.set_flag_c(carry);
    }

    /// RLC: rotates left, copying bit 7 to bit 0 and to the carry flag.
    fn rotate_left(&mut self, byte: u8) -> u8 {
        let rotated_value = byte.rotate_left(1);
        self.set_shift_flags(rotated_value, byte & 0x80 != 0);
        rotated_value
    }

    /// RL: rotates left through the carry flag, so bit 7 goes to the carry
    /// flag and the old carry flag to bit 0.
    fn rotate_left_through_carry(&mut self, byte: u8) -> u8 {
        let carry_value = u8::from(self.is_c_flag_set());
        let rotated_value = byte << 1 | carry_value;
        self.set_shift_flags(rotated_value, byte & 0x80 != 0);
        rotated_value
    }

    /// RRC: rotates right, copying bit 0 to bit 7 and to the carry flag.
    fn rotate_right(&mut self, byte: u8) -> u8 {
        let rotated_value = byte.rotate_right(1);
        self.set_shift_flags(rotated_value, byte & 0x01 != 0);
        rotated_value
    }

    /// RR: rotates right through the carry flag, so bit 0 goes to the carry
    /// flag and the old carry flag to bit 7.
    fn rotate_right_through_carry(&mut self, byte: u8) -> u8 {
        let carry_value = u8::from(self.is_c_flag_set());
        let rotated_value = carry_value << 7 | byte >> 1;
        self.set_shift_flags(rotated_value, byte & 0x01 != 0);
        rotated_value
    }

    /// SLA: shifts left, moving bit 7 to the carry flag and clearing bit 0.
    fn shift_left(&mut self, byte: u8) -> u8 {
        let shifted_value = byte << 1;
        self.set_shift_flags(shifted_value, byte & 0x80 != 0);
        shifted_value
    }

    /// SRA (with `keep_sign`) or SRL: shifts right, moving bit 0 to the
    /// carry flag. SRA keeps bit 7 so signed numbers keep their sign; SRL clears it.
    fn shift_right(&mut self, byte: u8, keep_sign: bool) -> u8 {
        let sign_bit = if keep_sign { byte & 0x80 } else { 0 };
        let shifted_value = byte >> 1 | sign_bit;
        self.set_shift_flags(shifted_value, byte & 0x01 != 0);
        shifted_value
    }

    /// SWAP: exchanges the upper and lower nibbles (4 bits) of the byte.
    fn swap_nibbles(&mut self, byte: u8) -> u8 {
        let swapped_value = byte.rotate_left(4);
        self.set_shift_flags(swapped_value, false);
        swapped_value
    }

    /// BIT: sets the zero flag if bit `bit_index` of `byte` is clear.
    fn test_bit(&mut self, byte: u8, bit_index: u8) {
        self.set_flag_z(!is_bit_set(byte, bit_index));
        self.set_flag_n(false);
        self.set_flag_h(true);
    }

    pub(super) fn rotate_register_left(&mut self, register: Register) {
        self.update_register(register, Self::rotate_left);
    }

    pub(super) fn rotate_register_left_through_carry(&mut self, register: Register) {
        self.update_register(register, Self::rotate_left_through_carry);
    }

    pub(super) fn rotate_register_right(&mut self, register: Register) {
        self.update_register(register, Self::rotate_right);
    }

    pub(super) fn rotate_register_right_through_carry(&mut self, register: Register) {
        self.update_register(register, Self::rotate_right_through_carry);
    }

    pub(super) fn rotate_memory_byte_left(&mut self) {
        self.update_byte_at_hl(Self::rotate_left);
    }

    pub(super) fn rotate_memory_byte_left_through_carry(&mut self) {
        self.update_byte_at_hl(Self::rotate_left_through_carry);
    }

    pub(super) fn rotate_memory_byte_right(&mut self) {
        self.update_byte_at_hl(Self::rotate_right);
    }

    pub(super) fn rotate_memory_byte_right_through_carry(&mut self) {
        self.update_byte_at_hl(Self::rotate_right_through_carry);
    }

    pub(super) fn swap_nibbles_in_register(&mut self, register: Register) {
        self.update_register(register, Self::swap_nibbles);
    }

    pub(super) fn swap_nibbles_in_memory_byte(&mut self) {
        self.update_byte_at_hl(Self::swap_nibbles);
    }

    pub(super) fn shift_register_left(&mut self, register: Register) {
        self.update_register(register, Self::shift_left);
    }

    pub(super) fn shift_memory_byte_left(&mut self) {
        self.update_byte_at_hl(Self::shift_left);
    }

    pub(super) fn shift_register_right_keeping_sign(&mut self, register: Register) {
        self.update_register(register, |cpu, byte| cpu.shift_right(byte, true));
    }

    pub(super) fn shift_memory_byte_right_keeping_sign(&mut self) {
        self.update_byte_at_hl(|cpu, byte| cpu.shift_right(byte, true));
    }

    pub(super) fn shift_register_right(&mut self, register: Register) {
        self.update_register(register, |cpu, byte| cpu.shift_right(byte, false));
    }

    pub(super) fn shift_memory_byte_right(&mut self) {
        self.update_byte_at_hl(|cpu, byte| cpu.shift_right(byte, false));
    }

    pub(super) fn test_register_bit(&mut self, register: Register, bit_index: u8) {
        let byte = self.read_from_register(register);
        self.test_bit(byte, bit_index);
    }

    pub(super) fn test_memory_bit(&mut self, bit_index: u8) {
        let address = self.read_from_register_pair(REGISTER_HL);
        let byte = self.read_byte_from_memory(address);
        self.test_bit(byte, bit_index);
    }

    pub(super) fn reset_register_bit(&mut self, register: Register, bit_index: u8) {
        self.update_register(register, |_, byte| reset_bit(byte, bit_index));
    }

    pub(super) fn reset_memory_bit(&mut self, bit_index: u8) {
        self.update_byte_at_hl(|_, byte| reset_bit(byte, bit_index));
    }

    pub(super) fn set_register_bit(&mut self, register: Register, bit_index: u8) {
        self.update_register(register, |_, byte| set_bit(byte, bit_index));
    }

    pub(super) fn set_memory_bit(&mut self, bit_index: u8) {
        self.update_byte_at_hl(|_, byte| set_bit(byte, bit_index));
    }
}
