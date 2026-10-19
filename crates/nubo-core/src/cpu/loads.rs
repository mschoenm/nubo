//! Load instructions, which copy values between registers and memory, and the
//! stack's push and pop.

use crate::cpu::{Cpu, REGISTER_HL, Register, RegisterPair};

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

    /// LD r, (HL) and the other loads from memory: reads the byte at `address`
    /// into `destination`.
    pub(super) fn load_memory_byte_in_destination_register(
        &mut self,
        address: u16,
        destination: Register,
    ) {
        let byte = self.read_byte_from_memory(address);
        self.store_in_register(destination, byte);
    }

    /// LD (HL), r and the other stores to memory: writes `source` to `address`.
    pub(super) fn load_source_register_in_memory(&mut self, source: Register, address: u16) {
        let byte = self.read_from_register(source);
        self.store_byte_in_memory(address, byte);
    }

    /// LD (HL), n: stores the next instruction byte at the address in HL.
    pub(super) fn load_immediate_value_in_memory(&mut self) {
        let address = self.read_from_register_pair(REGISTER_HL);
        let immediate_byte = self.read_next_instruction_byte();
        self.store_byte_in_memory(address, immediate_byte);
    }

    /// Pushes `word` onto the stack, high byte first, which takes three M-cycles.
    /// The stack grows down from the stack pointer.
    pub(super) fn push_word_to_stack(&mut self, word: u16) {
        self.step_one_machine_cycle();

        let stack_pointer = self.registers.stack_pointer;
        self.check_oam_bug_write(stack_pointer);

        let high_byte_address = stack_pointer.wrapping_sub(1);
        self.store_byte_in_memory(high_byte_address, (word >> 8) as u8);
        self.check_oam_bug_write(high_byte_address);

        let low_byte_address = high_byte_address.wrapping_sub(1);
        self.store_byte_in_memory(low_byte_address, (word & 0xFF) as u8);
        self.check_oam_bug_write(low_byte_address);

        self.registers.stack_pointer = low_byte_address;
    }

    /// PUSH rr: pushes `register_pair` onto the stack.
    pub(super) fn push_register_pair_to_stack(&mut self, register_pair: RegisterPair) {
        let word = self.read_from_register_pair(register_pair);
        self.push_word_to_stack(word);
    }

    /// Pops a word off the stack, low byte first, which takes two M-cycles.
    pub(super) fn pop_word_from_stack(&mut self) -> u16 {
        let low_byte_address = self.registers.stack_pointer;
        let first_byte = u16::from(self.read_byte_from_memory(low_byte_address));
        self.check_oam_bug_read(low_byte_address);
        self.check_oam_bug_mixed(low_byte_address);

        let high_byte_address = low_byte_address.wrapping_add(1);
        let second_byte = u16::from(self.read_byte_from_memory(high_byte_address));
        self.check_oam_bug_read(high_byte_address);

        self.registers.stack_pointer = high_byte_address.wrapping_add(1);
        (second_byte << 8) | first_byte
    }

    /// POP rr: pops a word off the stack into `register_pair`.
    pub(super) fn pop_word_into_register_pair_from_stack(&mut self, register_pair: RegisterPair) {
        let word = self.pop_word_from_stack();
        self.store_in_register_pair(register_pair, word);
    }
}
