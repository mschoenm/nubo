//! The CPU's basic operations that instructions are built from: advancing time,
//! memory accesses, register access and the flags.

use crate::clock::dots_per_m_cycle;
use crate::cpu::{
    BusActivityEntry, BusActivityKind, CARRY_FLAG, Cpu, HALF_CARRY_FLAG, Register, RegisterPair,
    SUBTRACT_FLAG, ZERO_FLAG,
};
use crate::utils;

impl Cpu {
    /// Spends one M-cycle without a memory access, letting the rest of the
    /// hardware run for that long.
    pub(super) fn step_one_machine_cycle(&mut self) {
        self.address_bus.sync();
    }

    /// Spends one M-cycle like [`Self::step_one_machine_cycle`], but calls
    /// `sample` halfway through it and returns what it saw.
    pub(super) fn step_one_machine_cycle_sampling_mid_cycle<T>(
        &mut self,
        sample: impl FnOnce(&Self) -> T,
    ) -> T {
        self.address_bus.sync_until_mid_cycle();
        let sampled = sample(self);
        self.address_bus.finish_mid_cycle_sync();
        sampled
    }

    /// Records a memory access in the M-cycle it happened in, padding the cycles
    /// since the previous access with `None`. Processor test mode never lets
    /// M-cycles wait to run (see `bulk_cycles.rs`), so the dots since power-on
    /// are up to date.
    fn record_bus_activity(&mut self, bus_activity_entry: BusActivityEntry) {
        let dots_per_machine_cycle =
            u64::from(dots_per_m_cycle(self.address_bus.cgb_double_speed()));
        let elapsed_dots = self.address_bus.elapsed_dots() - self.instruction_start_dots;
        let current_machine_cycle = usize::try_from(elapsed_dots / dots_per_machine_cycle)
            .expect("an instruction takes a few M-cycles");
        let recorded_cycles = self.opcode_bus_activity.len();
        let cycles_with_no_activity = (current_machine_cycle - 1) - recorded_cycles;

        for _ in 0..cycles_with_no_activity {
            self.opcode_bus_activity.push(None);
        }

        self.opcode_bus_activity.push(Some(bus_activity_entry));
    }

    fn record_bus_read(&mut self, address: u16, value: u8) {
        self.record_bus_activity(BusActivityEntry {
            address,
            value,
            kind: BusActivityKind::Read,
        });
    }

    fn record_bus_write(&mut self, address: u16, value: u8) {
        self.record_bus_activity(BusActivityEntry {
            address,
            value,
            kind: BusActivityKind::Write,
        });
    }

    /// Reads a byte from memory, which takes one M-cycle.
    pub(super) fn read_byte_from_memory(&mut self, address: u16) -> u8 {
        self.step_one_machine_cycle();
        self.read_byte_in_synced_cycle(address)
    }

    /// The read at the end of an M-cycle the rest of the hardware has already
    /// run through.
    pub(super) fn read_byte_in_synced_cycle(&mut self, address: u16) -> u8 {
        // A read puts its address on the bus after its M-cycle ran, like SameBoy's `cycle_read`,
        // and a write before (see `store_byte_in_memory`). The APU's stopped wave channel reads
        // the bus in some states. No test ROM result depends on either order: putting a read's
        // address on the bus before its M-cycle only changes four Gambatte ch3 runs' end states.
        self.address_bus.set_cpu_bus_address(address);
        let byte = self.address_bus.cpu_read_byte(address);

        if self.address_bus.processor_test_mode() {
            self.record_bus_read(address, byte);
        }

        byte
    }

    /// Reads a little-endian word from memory, which takes two M-cycles.
    pub(super) fn read_word_from_memory(&mut self, address: u16) -> u16 {
        let first_byte = self.read_byte_from_memory(address);
        let second_byte = self.read_byte_from_memory(address.wrapping_add(1));
        utils::as_word(first_byte, second_byte)
    }

    /// Writes a byte to memory, which takes one M-cycle.
    pub(super) fn store_byte_in_memory(&mut self, address: u16, byte: u8) {
        // The write's address is on the bus during its own M-cycle. SameBoy's `cycle_write` puts it
        // there only after the write, which changes the sound of SameSuite's
        // channel_3_wave_ram_locked_write and of Super Mario Land 2 on CGB-E, but no test ROM
        // result, so it stays as it was until a test shows which is right.
        self.address_bus.set_cpu_bus_address(address);
        self.address_bus.cpu_write_cycle(address, byte);

        if self.address_bus.processor_test_mode() {
            self.record_bus_write(address, byte);
        }
    }

    /// Writes a little-endian word to memory, which takes two M-cycles.
    pub(super) fn store_word_in_memory(&mut self, address: u16, word: u16) {
        let (first_byte, second_byte) = utils::as_bytes(word);
        self.store_byte_in_memory(address, first_byte);
        self.store_byte_in_memory(address.wrapping_add(1), second_byte);
    }

    pub(super) fn read_from_register(&self, register: Register) -> u8 {
        match register {
            Register::A => self.registers.a,
            Register::B => self.registers.b,
            Register::C => self.registers.c,
            Register::D => self.registers.d,
            Register::E => self.registers.e,
            Register::F => self.registers.f,
            Register::H => self.registers.h,
            Register::L => self.registers.l,
        }
    }

    pub(super) fn store_in_register(&mut self, register: Register, value: u8) {
        match register {
            Register::A => self.registers.a = value,
            Register::B => self.registers.b = value,
            Register::C => self.registers.c = value,
            Register::D => self.registers.d = value,
            Register::E => self.registers.e = value,
            Register::F => self.registers.f = value,
            Register::H => self.registers.h = value,
            Register::L => self.registers.l = value,
        }
    }

    pub(super) fn read_from_register_pair(&self, register_pair: RegisterPair) -> u16 {
        let high_byte = self.read_from_register(register_pair.first);
        let low_byte = self.read_from_register(register_pair.second);
        u16::from_be_bytes([high_byte, low_byte])
    }

    pub(super) fn store_in_register_pair(&mut self, register_pair: RegisterPair, value: u16) {
        let [high_byte, low_byte] = value.to_be_bytes();
        self.store_in_register(register_pair.first, high_byte);
        self.store_in_register(register_pair.second, low_byte);
    }

    fn set_flag(&mut self, flag: u8, is_set: bool) {
        if is_set {
            self.registers.f |= flag;
        } else {
            self.registers.f &= !flag;
        }
    }

    fn is_flag_set(&self, flag: u8) -> bool {
        self.registers.f & flag != 0
    }

    /// Sets or clears the zero flag (Z).
    pub(super) fn set_flag_z(&mut self, is_set: bool) {
        self.set_flag(ZERO_FLAG, is_set);
    }

    /// Sets or clears the subtract flag (N).
    pub(super) fn set_flag_n(&mut self, is_set: bool) {
        self.set_flag(SUBTRACT_FLAG, is_set);
    }

    /// Sets or clears the half carry flag (H).
    pub(super) fn set_flag_h(&mut self, is_set: bool) {
        self.set_flag(HALF_CARRY_FLAG, is_set);
    }

    /// Sets or clears the carry flag (C).
    pub(super) fn set_flag_c(&mut self, is_set: bool) {
        self.set_flag(CARRY_FLAG, is_set);
    }

    pub(super) fn is_z_flag_set(&self) -> bool {
        self.is_flag_set(ZERO_FLAG)
    }

    pub(super) fn is_n_flag_set(&self) -> bool {
        self.is_flag_set(SUBTRACT_FLAG)
    }

    pub(super) fn is_h_flag_set(&self) -> bool {
        self.is_flag_set(HALF_CARRY_FLAG)
    }

    pub(super) fn is_c_flag_set(&self) -> bool {
        self.is_flag_set(CARRY_FLAG)
    }

    /// Whether a 16-bit increment, decrement or stack access at `address` can
    /// trigger the OAM bug. OAM (object attribute memory, 0xFE00-0xFE9F) holds the
    /// sprite table; on the DMG, touching this range with 16-bit register
    /// arithmetic while the PPU scans OAM corrupts it.
    fn can_trigger_oam_bug(&self, address: u16) -> bool {
        !self.address_bus.processor_test_mode() && (0xFE00..=0xFEFF).contains(&address)
    }

    /// Triggers the OAM bug's write corruption if `address` is in OAM.
    pub(super) fn check_oam_bug_write(&mut self, address: u16) {
        if self.can_trigger_oam_bug(address) {
            self.address_bus.trigger_oam_bug_write();
        }
    }

    /// Triggers the OAM bug's read corruption if `address` is in OAM.
    pub(super) fn check_oam_bug_read(&mut self, address: u16) {
        if self.can_trigger_oam_bug(address) {
            self.address_bus.trigger_oam_bug_read();
        }
    }

    /// Triggers the OAM bug's corruption for an increment and read in the same
    /// M-cycle if `address` is in OAM.
    pub(super) fn check_oam_bug_mixed(&mut self, address: u16) {
        if self.can_trigger_oam_bug(address) {
            self.address_bus.trigger_oam_bug_mixed();
        }
    }
}
