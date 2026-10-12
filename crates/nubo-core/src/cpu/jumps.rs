//! Jumps, calls, returns and restarts: the instructions that change the
//! program counter.

use crate::cpu::Cpu;

impl Cpu {
    fn jump_if(&mut self, new_address: u16, condition: bool) {
        if condition {
            self.registers.program_counter = new_address;
            self.step_one_machine_cycle();
        }
    }

    /// JR e: jumps by the signed offset in the next byte.
    pub(super) fn relative_jump(&mut self) {
        self.relative_jump_if(true);
    }

    /// JR cc, e: jumps by the signed offset in the next byte if `condition` holds.
    pub(super) fn relative_jump_if(&mut self, condition: bool) {
        let offset_byte = self.read_next_instruction_byte() as i8;
        let program_counter = self.registers.program_counter;
        let result_address = program_counter.wrapping_add_signed(offset_byte.into());
        self.jump_if(result_address, condition);
    }

    /// JP nn: jumps to the address in the next two bytes.
    pub(super) fn absolute_jump(&mut self) {
        self.absolute_jump_if(true);
    }

    /// JP cc, nn: jumps to the address in the next two bytes if `condition` holds.
    pub(super) fn absolute_jump_if(&mut self, condition: bool) {
        let address = self.read_next_instruction_word();
        self.jump_if(address, condition);
    }

    /// CALL nn: pushes the return address and jumps to the address in the next two bytes.
    pub(super) fn call(&mut self) {
        self.call_if(true);
    }

    /// CALL cc, nn: like [`Self::call`], if `condition` holds.
    pub(super) fn call_if(&mut self, condition: bool) {
        let address = self.read_next_instruction_word();
        if condition {
            let program_counter = self.registers.program_counter;
            self.push_word_to_stack(program_counter);
            self.registers.program_counter = address;
        }
    }

    /// RET: pops the return address pushed by a call.
    pub(super) fn return_from_call(&mut self) {
        let word = self.pop_word_from_stack();
        self.registers.program_counter = word;
        self.step_one_machine_cycle();
    }

    /// RET cc: like [`Self::return_from_call`], if `condition` holds. Checking
    /// the condition takes an extra M-cycle.
    pub(super) fn return_from_call_if(&mut self, condition: bool) {
        self.step_one_machine_cycle();
        if condition {
            self.return_from_call();
        }
    }

    /// RST: calls one of the fixed addresses 0x00, 0x08, ..., 0x38.
    pub(super) fn restart(&mut self, new_address: u16) {
        self.push_word_to_stack(self.registers.program_counter);
        self.registers.program_counter = new_address;
    }
}
