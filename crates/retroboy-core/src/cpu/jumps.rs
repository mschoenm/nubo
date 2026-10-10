use crate::cpu::Cpu;

impl Cpu {
    fn conditional_jump(&mut self, new_address: u16, condition: bool) {
        if condition {
            self.registers.program_counter = new_address;
            self.step_one_machine_cycle();
        }   
    }

    pub(super) fn conditional_relative_jump(&mut self, condition: bool) {
        let offset_byte = self.read_next_instruction_byte() as i8;
        let program_counter = self.registers.program_counter;
        let result_address = program_counter.wrapping_add_signed(offset_byte.into());
        self.conditional_jump(result_address, condition);
    }

    pub(super) fn conditional_jump_using_immediate_word(&mut self, condition: bool) {
        let address = self.read_next_instruction_word();
        self.conditional_jump(address, condition);
    }

    pub(super) fn call(&mut self) {
        let word = self.read_next_instruction_word();
        let program_counter = self.registers.program_counter;
        self.push_word_to_stack(program_counter);
        self.registers.program_counter = word;
    }

    pub(super) fn conditional_call_using_immediate_word(&mut self, condition: bool) {
        let word = self.read_next_instruction_word();
        if condition {
            let program_counter = self.registers.program_counter;
            self.push_word_to_stack(program_counter);
            self.registers.program_counter = word;
        }
    }

    pub(super) fn stack_return(&mut self) {
        let word = self.pop_word_from_stack();
        self.registers.program_counter = word;
        self.step_one_machine_cycle();
    }

    pub(super) fn conditional_stack_return(&mut self, condition: bool) {
        self.step_one_machine_cycle();
        if condition {
            self.stack_return();
        }
    }

    pub(super) fn restart(&mut self, new_address: u16) {
        self.push_word_to_stack(self.registers.program_counter);
        self.registers.program_counter = new_address;
    }
}
