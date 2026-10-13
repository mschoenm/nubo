//! The Sharp SM83 CPU, the Game Boy's processor.
//!
//! The SM83 is similar to the Zilog Z80 and Intel 8080. It runs one instruction
//! at a time and talks to the rest of the hardware through the [`AddressBus`].
//! Time is counted in M-cycles (machine cycles): one M-cycle is four ticks of
//! the CPU's clock, which lasts four PPU dots, or two in the CGB's double speed
//! mode (see `clock.rs`), and every memory access takes one M-cycle.

use crate::address_bus::{AddressBus, CartridgeEffects};
use crate::error::LoadRomError;
use getset::{Getters, MutGetters};
use serializable_derive::Serializable;

/// Bit of the `f` register set when a result is zero.
const ZERO_FLAG: u8 = 0x80;
/// Bit of the `f` register set when the last arithmetic operation was a subtraction.
const SUBTRACT_FLAG: u8 = 0x40;
/// Bit of the `f` register set when a result carried out of (or borrowed into) bit 3.
const HALF_CARRY_FLAG: u8 = 0x20;
/// Bit of the `f` register set when a result carried out of (or borrowed into) the top bit.
const CARRY_FLAG: u8 = 0x10;

/// The CPU registers.
///
/// `f` holds the flags: zero (bit 7), subtract (bit 6), half carry (bit 5)
/// and carry (bit 4).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serializable)]
pub struct Registers {
    /// The accumulator, which holds the result of most arithmetic and logic.
    pub a: u8,
    /// General purpose; paired with `c` as BC.
    pub b: u8,
    /// General purpose; paired with `b` as BC.
    pub c: u8,
    /// General purpose; paired with `e` as DE.
    pub d: u8,
    /// General purpose; paired with `d` as DE.
    pub e: u8,
    /// General purpose; paired with `l` as HL, which often holds a memory address.
    pub h: u8,
    /// General purpose; paired with `h` as HL.
    pub l: u8,
    /// The flags (see above). The low four bits are always zero.
    pub f: u8,
    /// The opcode of the instruction that will run next.
    pub opcode: u8,
    /// The address of the next byte of the program to read.
    pub program_counter: u16,
    /// The address of the top of the stack, which grows downward.
    pub stack_pointer: u16,
}

/// IME, the interrupt master enable flag, which decides whether the CPU
/// jumps to an interrupt handler when an interrupt is requested.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serializable)]
enum InterruptMasterEnable {
    /// Requested interrupts wait until IME is turned on.
    #[default]
    Off,
    /// EI ran while IME was off. IME turns on as the next instruction starts,
    /// so no interrupt can be handled between EI and that instruction.
    OnAfterNextInstruction,
    /// A pending interrupt is handled after the current instruction.
    On,
}

/// What the CPU is doing: running instructions or waiting in HALT or STOP.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serializable)]
enum RunState {
    /// Running instructions.
    #[default]
    Running,
    /// Just halted by HALT, or by STOP while a button is held or after a
    /// speed switch: like `Halted`, but the first halted M-cycle, which
    /// samples interrupts earlier (see `halted_step`), hasn't run yet.
    JustHalted,
    /// Waiting, one M-cycle at a time, until an interrupt is requested.
    Halted,
    /// In STOP mode: waiting, one M-cycle at a time, until a button press
    /// pulls a joypad line low.
    Stopped,
}

/// Something HALT or STOP left behind for how the next instruction starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serializable)]
enum NextInstruction {
    /// The next instruction starts as usual.
    #[default]
    Normal,
    /// The "HALT bug": HALT ran with IME off while an interrupt was pending,
    /// and the CPU fails to move the program counter past the next opcode, so
    /// the byte after HALT is read twice.
    HaltBug,
    /// HALT ran with IME off while an HBlank DMA block fell due. The CPU
    /// halted having already fetched the byte after HALT, which then runs
    /// twice like with the HALT bug, but its second fetch overlaps the DMA and
    /// takes no time.
    HaltBugDuringHdma,
    /// STOP read the byte after it as the opcode to run once the speed switch
    /// halt ends, because an HBlank DMA block was due.
    StopOpcodeKept,
}

/// Whether the CPU read from or wrote to memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusActivityKind {
    /// The CPU read a byte.
    Read,
    /// The CPU wrote a byte.
    Write,
}

/// One memory access the CPU made while running an instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BusActivityEntry {
    /// The memory address that was accessed.
    pub address: u16,
    /// The byte that was read or written.
    pub value: u8,
    /// Whether this was a read or a write.
    pub kind: BusActivityKind,
}

/// The CPU, which owns the address bus and through it the rest of the hardware.
///
/// Like the address bus's, its `Debug` output is compared by test runners to
/// prove two emulators are in exactly the same state (`--verify-boot` and
/// `clone_with_rom_matches_a_real_boot`), so it must show every field and stay
/// deterministic.
#[derive(Debug, Getters, MutGetters, Serializable)]
pub struct Cpu {
    #[getset(get = "pub(crate)", get_mut = "pub(crate)")]
    registers: Registers,
    run_state: RunState,
    next_instruction: NextInstruction,
    ime: InterruptMasterEnable,
    /// The memory accesses of the last instruction, one entry per M-cycle
    /// (`None` for cycles without an access). Only recorded in processor test
    /// mode, whose tests look at one instruction at a time, so save states
    /// leave it out.
    #[getset(get = "pub(crate)")]
    #[serializable(skip)]
    opcode_bus_activity: Vec<Option<BusActivityEntry>>,
    /// When the last instruction started, in dots since power-on (see
    /// `AddressBus::elapsed_dots`), so each access in `opcode_bus_activity`
    /// lands in its M-cycle. Only set in processor test mode.
    #[serializable(skip)]
    instruction_start_dots: u64,
    /// Save states store the components on the address bus next to the CPU
    /// rather than inside it (see `Emulator`'s save state), so that the
    /// stored layout follows the hardware, not which struct owns which.
    #[getset(get = "pub(crate)", get_mut = "pub(crate)")]
    #[serializable(skip)]
    address_bus: AddressBus,
}

/// One of the CPU's 8-bit registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Register {
    A,
    B,
    C,
    D,
    E,
    F,
    H,
    L,
}

/// Two 8-bit registers used together as one 16-bit value, `first` holding the
/// high byte (for example BC, with B as the high byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisterPair {
    pub first: Register,
    pub second: Register,
}

pub const REGISTER_AF: RegisterPair = RegisterPair {
    first: Register::A,
    second: Register::F,
};
pub const REGISTER_HL: RegisterPair = RegisterPair {
    first: Register::H,
    second: Register::L,
};
pub const REGISTER_BC: RegisterPair = RegisterPair {
    first: Register::B,
    second: Register::C,
};
pub const REGISTER_DE: RegisterPair = RegisterPair {
    first: Register::D,
    second: Register::E,
};

/// Whether `opcode` is one of the eleven opcodes the SM83 doesn't define.
/// Running one locks up the CPU until the console is reset.
fn is_illegal_opcode(opcode: u8) -> bool {
    matches!(
        opcode,
        0xD3 | 0xDB | 0xDD | 0xE3 | 0xE4 | 0xEB | 0xEC | 0xED | 0xF4 | 0xFC | 0xFD
    )
}

impl Cpu {
    pub(super) fn new(address_bus: AddressBus) -> Self {
        Cpu {
            registers: Registers::default(),
            run_state: RunState::Running,
            next_instruction: NextInstruction::Normal,
            ime: InterruptMasterEnable::Off,
            opcode_bus_activity: Vec::new(),
            instruction_start_dots: 0,
            address_bus,
        }
    }

    /// A copy of the CPU and all the hardware behind it, with the cartridge
    /// made from `rom` inserted instead of the current one.
    pub(super) fn clone_with_rom(
        &self,
        rom: Vec<u8>,
        cartridge_effects: Box<dyn CartridgeEffects>,
    ) -> Result<Cpu, LoadRomError> {
        Ok(Cpu {
            registers: self.registers.clone(),
            run_state: self.run_state,
            next_instruction: self.next_instruction,
            ime: self.ime,
            opcode_bus_activity: self.opcode_bus_activity.clone(),
            instruction_start_dots: self.instruction_start_dots,
            address_bus: self.address_bus.clone_with_rom(rom, cartridge_effects)?,
        })
    }

    /// Reads the byte at the program counter and moves past it.
    pub(super) fn read_next_instruction_byte(&mut self) -> u8 {
        let byte = self.read_byte_from_memory(self.registers.program_counter);
        self.registers.program_counter = self.registers.program_counter.wrapping_add(1);
        byte
    }

    /// Reads the little-endian word at the program counter and moves past it.
    pub(super) fn read_next_instruction_word(&mut self) -> u16 {
        let word = self.read_word_from_memory(self.registers.program_counter);
        self.registers.program_counter = self.registers.program_counter.wrapping_add(2);
        word
    }

    /// Whether the CPU ran an illegal opcode and has stopped for good.
    ///
    /// This is derived from the opcode register, which keeps the illegal opcode
    /// because the CPU never fetches another one, so save states need no extra field.
    /// A halted CPU may hold an opcode it fetched before halting but hasn't run yet.
    pub(crate) fn is_locked_up(&self) -> bool {
        !self.is_halted() && is_illegal_opcode(self.registers.opcode)
    }

    /// Whether HALT, or STOP without entering STOP mode, paused the CPU until
    /// an interrupt is pending.
    fn is_halted(&self) -> bool {
        matches!(self.run_state, RunState::JustHalted | RunState::Halted)
    }
}

mod alu;
mod bitops;
pub(crate) mod interrupts;
mod jumps;
mod loads;
mod microops;
mod opcodes;
mod post_boot;
pub(crate) mod timers;
