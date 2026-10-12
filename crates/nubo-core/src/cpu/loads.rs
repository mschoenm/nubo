//! Load instructions, which copy values between registers and memory, and the
//! stack's push and pop.

use crate::cpu::{Cpu, Register, RegisterPair};

impl Cpu {
    /// LD r, n: loads the next instruction byte into `register`.
    pub(super) fn load_immediate_value(&mut self, register: Register) {
        let immediate_byte = self.read_next_instruction_byte();
        self.store_in_register(register, immediate_byte);
    }

    /// LD r, r': copies `source` into `destination`.
    pub(super) fn load_source_register_in_destination_register(
        &mut self,
        source: Register,
        destination: Register,
    ) {
        let source_value = self.read_from_register(source);
        self.store_in_register(destination, source_value);
    }

    pub(super) fn load_memory_byte_in_destination_register(
        &mut self,
        address: u16,
        destination: Register,
    ) {
        let byte = self.read_byte_from_memory(address);
        self.store_in_register(destination, byte);
    }

    pub(super) fn load_source_register_in_memory(&mut self, source: Register, address: u16) {
        let byte = self.read_from_register(source);
        self.store_byte_in_memory(address, byte);
    }

    /// LD (rr), n: stores the next instruction byte at the address in `register_pair`.
    pub(super) fn load_immediate_value_in_memory(&mut self, register_pair: RegisterPair) {
        let address = self.read_from_register_pair(register_pair);
        let immediate_byte = self.read_next_instruction_byte();
        self.store_byte_in_memory(address, immediate_byte);
    }

    /// Pushes `word` onto the stack, high byte first, which takes three M-cycles.
    /// The stack grows down from the stack pointer.
    pub(super) fn push_word_to_stack(&mut self, word: u16) {
        self.step_one_machine_cycle();

        let sp_initial = self.registers.stack_pointer;
        self.check_oam_bug_write(sp_initial);

        let sp_high = sp_initial.wrapping_sub(1);
        self.store_byte_in_memory(sp_high, (word >> 8) as u8);
        self.check_oam_bug_write(sp_high);

        let sp_low = sp_high.wrapping_sub(1);
        self.store_byte_in_memory(sp_low, (word & 0xFF) as u8);
        self.check_oam_bug_write(sp_low);

        self.registers.stack_pointer = sp_low;
    }

    pub(super) fn push_register_pair_to_stack(&mut self, register_pair: RegisterPair) {
        let word = self.read_from_register_pair(register_pair);
        self.push_word_to_stack(word);
    }

    /// Pops a word off the stack, low byte first, which takes two M-cycles.
    pub(super) fn pop_word_from_stack(&mut self) -> u16 {
        let sp_low = self.registers.stack_pointer;
        let first_byte = u16::from(self.read_byte_from_memory(sp_low));
        self.check_oam_bug_read(sp_low);
        self.check_oam_bug_mixed(sp_low);

        let sp_high = sp_low.wrapping_add(1);
        let second_byte = u16::from(self.read_byte_from_memory(sp_high));
        self.check_oam_bug_read(sp_high);

        self.registers.stack_pointer = sp_high.wrapping_add(1);
        (second_byte << 8) | first_byte
    }

    pub(super) fn pop_word_into_register_pair_from_stack(&mut self, register_pair: RegisterPair) {
        let word = self.pop_word_from_stack();
        self.store_in_register_pair(register_pair, word);
    }
}
