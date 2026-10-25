//! Checks that a damaged save state never makes the emulator panic: loading it either fails with
//! `SaveStateError`, or the state loads and the emulator runs on from it.
//!
//! Changing random bytes of a state mostly breaks the names and lengths that frame its values,
//! which the loader refuses long before any value reaches the hardware. The values the console
//! can't be in, such as an OAM DMA past the end of OAM or a serial transfer with more than 8 bits
//! shifted, are only found by changing whole values. So the test reads the state's own layout
//! (`Emulator::save_state_layout`, which says how each value is stored) to find every value in a
//! real state, and writes edge values over one value at a time and over random pairs of them.
//!
//! The states come from a game made here that keeps the hardware busy (`busy_game`): sound on all
//! four channels, the timer, a serial transfer, objects and the window on screen, OAM DMA, a Super
//! Game Boy packet half sent, and on the Game Boy Color palettes, an HBlank VRAM DMA and a speed
//! switch every fourth frame. Each model saves at a few moments picked by what the state holds
//! (`MOMENTS`), such as the PPU in the middle of fetching an object.
//!
//! The number of cases is fixed, with fixed seeds, so the test takes a few seconds on every run.
//! `NUBO_DAMAGED_STATE_PAIRS` sets how many random pairs each model tries instead, for a longer
//! run.

use super::{BUSY_PROGRAM_START, HEADER_START, ROM_SIZE, booted, header};
use crate::address_bus::effects::empty_cartridge_effects;
use crate::test_random::Random;
use crate::{Emulator, Model, SgbBorder, SgbBorderTime};
use std::collections::HashMap;
use std::ops::Range;

/// What every save state starts with, before its header's fields: the magic text and the format
/// version.
const MAGIC_AND_VERSION_LENGTH: usize = 17;

/// Byte arrays this long or shorter have every byte changed; longer ones, such as VRAM and work
/// RAM, where any value is valid, only their first and last bytes.
const LONGEST_ARRAY_CHANGED_WHOLE: usize = 16;

/// Values just past the bounds the hardware's counters and indices keep to: bits in a byte and
/// pixels in a FIFO, objects on a line, objects in OAM, lines in a frame, OAM's bytes and the
/// screen's width, dots in a line and a VRAM bank's size.
const PLAUSIBLE_BOUNDS: [u64; 7] = [8, 10, 40, 154, 160, 456, 0x2000];

/// Frames the busy game runs after booting before the test looks for its moments, so that every
/// part of the hardware it uses has started.
const WARM_UP_FRAMES: usize = 3;

/// The most steps the test looks for a model's moments in, several frames.
const MOMENT_SEARCH_STEPS: usize = 20_000;

/// A model the test runs on, with the cartridge type its game uses (so that several memory bank
/// controllers are covered), which values it changes one at a time, and how many random pairs
/// of values it changes together. The counts are fixed, with fixed seeds, so that the test never
/// takes longer than it does now.
struct TestedModel {
    model: Model,
    /// The cartridge type byte of the header (0x0147).
    cartridge_type: u8,
    /// The start of the paths of the values changed one at a time (`""` for all of them).
    single_values: &'static [&'static str],
    pairs: usize,
    seed: u64,
}

/// The models, one thread each, each taking a few seconds: a Game Boy with an MBC1, a Super Game
/// Boy with an MBC3 and its clock, and the Game Boy Color's revisions C and E with an MBC5 and an
/// MBC3. The Super Game Boy shares the rest of its hardware with the Game Boy, and revision C
/// differs from E mostly in its sound and PPU, so they change their other values only in pairs,
/// which keeps them as fast as the others.
const TESTED_MODELS: [TestedModel; 4] = [
    TestedModel {
        model: Model::Dmg,
        cartridge_type: 0x03,
        single_values: &[""],
        pairs: 700,
        seed: 0x5EED_0001,
    },
    TestedModel {
        model: Model::Sgb,
        cartridge_type: 0x10,
        single_values: &["ppu.sgb", "joypad."],
        pairs: 2_000,
        seed: 0x5EED_0002,
    },
    TestedModel {
        model: Model::CgbC,
        cartridge_type: 0x1B,
        single_values: &["apu.", "ppu."],
        pairs: 300,
        seed: 0x5EED_0003,
    },
    TestedModel {
        model: Model::CgbE,
        cartridge_type: 0x10,
        single_values: &[""],
        pairs: 100,
        seed: 0x5EED_0004,
    },
];

/// A moment to save a state at: the first step of the search at which `found` holds for the
/// saved state. Each value is changed one at a time in the state of the moment whose `focus`
/// names it most closely (the longest path that starts its path), or of the first moment when
/// none does; random pairs of values are changed in the states of every moment.
struct Moment {
    name: &'static str,
    /// Whether the moment can happen on a model.
    on: fn(Model) -> bool,
    found: fn(&StateView) -> bool,
    /// The start of the paths of the values this moment is about.
    focus: &'static [&'static str],
}

fn any_model(_: Model) -> bool {
    true
}

fn cgb(model: Model) -> bool {
    model.is_cgb()
}

fn sgb(model: Model) -> bool {
    model.is_sgb()
}

/// Whether the PPU is in the middle of fetching an object's tile in mode 3.
fn fetching_an_object(state: &StateView) -> bool {
    state.variant("ppu.line_state") == "Drawing"
        && state.number("ppu.pipeline.during_object_fetch") == 1
}

const MOMENTS: [Moment; 7] = [
    Moment {
        name: "fetching an object",
        on: any_model,
        found: fetching_an_object,
        focus: &["ppu."],
    },
    Moment {
        name: "fetching the window",
        on: any_model,
        found: |state| {
            state.variant("ppu.line_state") == "Drawing"
                && state.number("ppu.pipeline.window_is_being_fetched") == 1
        },
        focus: &[
            "ppu.pipeline.window",
            "ppu.pipeline.wx",
            "ppu.pipeline.cgb_wx",
            "ppu.window",
            "ppu.delayed_wy",
            "ppu.wx",
            "ppu.wy",
        ],
    },
    Moment {
        name: "copying OAM DMA during a serial transfer",
        on: any_model,
        found: |state| {
            state.number("oam_dma.in_progress") == 1
                && state.number("serial.transfer_enabled") == 1
                && state.number("serial.bits_transferred") > 0
        },
        focus: &[
            "oam_dma.",
            "serial.",
            "ppu.oam_dma_access",
            "ppu.next_oam_dma_access",
            "ppu.pending_dma_oam_write",
            "ppu.countdowns.oam_dma_access",
            "bus.extra_oam",
        ],
    },
    Moment {
        name: "receiving a Super Game Boy packet",
        on: sgb,
        found: |state| state.number("ppu.sgb.receiver.bits_received") > 0,
        focus: &["ppu.sgb.", "joypad."],
    },
    Moment {
        name: "copying HBlank VRAM DMA",
        on: cgb,
        found: |state| state.number("vram_dma.in_progress") == 1,
        focus: &[
            "vram_dma.",
            "ppu.hdma_start_delay",
            "ppu.lcd_off_hblank_dma",
        ],
    },
    Moment {
        name: "switching speed",
        on: cgb,
        found: |state| state.number("speed_switch.halt_units") > 0,
        focus: &[
            "speed_switch.",
            "timers.",
            "cpu.",
            "ppu.stop",
            "bus.ppu_dot_debt",
            "bus.elapsed_dots",
        ],
    },
    Moment {
        name: "fetching an object in double speed",
        on: cgb,
        found: |state| {
            state.number("speed_switch.cgb_double_speed") == 1 && fetching_an_object(state)
        },
        focus: &["ppu.pipeline.", "ppu.timing", "apu."],
    },
];

/// The index of the moment among `moments` in whose state the value at `path` is changed on its
/// own (see `Moment`).
fn home_moment(moments: &[(&Moment, StoredState)], path: &str) -> usize {
    let closeness = |moment: &Moment| {
        moment
            .focus
            .iter()
            .filter(|focus| path.starts_with(*focus))
            .map(|focus| focus.len())
            .max()
    };
    let mut home = 0;
    let mut closest = None;
    for (index, (moment, _)) in moments.iter().enumerate() {
        let closeness = closeness(moment);
        if closeness > closest {
            home = index;
            closest = closeness;
        }
    }
    home
}

/// Machine code for the busy game, written an instruction at a time.
#[derive(Default)]
struct Program(Vec<u8>);

/// The opcode of `JR NZ`, the relative jump the program's loops make back while their count or
/// comparison isn't zero.
const JR_NZ: u8 = 0x20;

impl Program {
    fn bytes(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    /// Writes `value` to the I/O register at 0xFF00 + `register` (`LD A, value; LDH (register),
    /// A`).
    fn write_register(&mut self, register: u8, value: u8) {
        self.bytes(&[0x3E, value, 0xE0, register]);
    }

    fn write_registers(&mut self, writes: &[(u8, u8)]) {
        for &(register, value) in writes {
            self.write_register(register, value);
        }
    }

    /// Where the next instruction goes, counted from the program's start.
    fn here(&self) -> usize {
        self.0.len()
    }

    /// A relative jump (`opcode` is a `JR`) back to `target`.
    fn jump_back(&mut self, opcode: u8, target: usize) {
        let distance = i8::try_from(self.here() + 2 - target).expect("a jump of at most 128 bytes");
        self.bytes(&[opcode, distance.wrapping_neg().to_le_bytes()[0]]);
    }

    /// A jump to `target` (`JP target`), which may be further away than a relative one reaches.
    fn jump_to(&mut self, target: usize) {
        let address = u16::try_from(BUSY_PROGRAM_START + target).expect("an address in the ROM");
        let [low, high] = address.to_le_bytes();
        self.bytes(&[0xC3, low, high]);
    }

    /// Waits in a loop while LY isn't `line`. Reading LY catches the PPU up, so the states saved
    /// during the loop show where it really is.
    fn wait_for_line(&mut self, line: u8) {
        let wait = self.here();
        self.bytes(&[0xF0, 0x44, 0xFE, line]); // LDH A, (LY); CP line
        self.jump_back(JR_NZ, wait);
    }
}

/// Where the OAM DMA routine is kept in the ROM, to be copied to HRAM: the CPU can't read the
/// ROM while OAM DMA copies from work RAM on a Game Boy.
const OAM_DMA_ROUTINE_ADDRESS: u16 = 0x3F00;
/// Starts OAM DMA from the page in A and waits the 160 M-cycles it takes.
const OAM_DMA_ROUTINE: [u8; 8] = [
    0xE0, 0x46, // LDH (0x46), A
    0x3E, 0x28, // LD A, 40
    0x3D, // wait: DEC A
    0x20, 0xFD, // JR NZ, wait
    0xC9, // RET
];
/// The line on which the busy game wakes up each frame and starts its transfers. Objects and the
/// window are on the lines around it.
const TRANSFER_LINE: u8 = 96;
/// The line until which the busy game then reads LY, after which it halts until the next frame's
/// `TRANSFER_LINE`: halted, the emulator runs whole stretches at once, which keeps the test fast.
const LAST_BUSY_LINE: u8 = 104;

/// A game for `model` that sets up as much of the hardware as it can, and then, once a frame,
/// starts a serial transfer, sends part of a Super Game Boy packet, retriggers two sound
/// channels, starts OAM DMA and, on the Game Boy Color, starts an HBlank VRAM DMA and switches
/// speed.
fn busy_game(model: Model, cartridge_type: u8) -> Vec<u8> {
    let cgb_flag = if model.is_cgb() { 0x80 } else { 0x00 };
    let header = header(cgb_flag, 0x03, cartridge_type, 0x33);
    let mut rom = vec![0; ROM_SIZE];
    rom[HEADER_START..HEADER_START + header.len()].copy_from_slice(&header);
    let routine = usize::from(OAM_DMA_ROUTINE_ADDRESS);
    rom[routine..routine + OAM_DMA_ROUTINE.len()].copy_from_slice(&OAM_DMA_ROUTINE);

    let mut program = Program::default();
    program.bytes(&[0xF3, 0x31, 0xFE, 0xFF]); // DI; LD SP, 0xFFFE
    let [routine_low, routine_high] = OAM_DMA_ROUTINE_ADDRESS.to_le_bytes();
    let routine_length = u8::try_from(OAM_DMA_ROUTINE.len()).unwrap();
    program.bytes(&[0x21, routine_low, routine_high]); // LD HL, routine
    program.bytes(&[0x11, 0x80, 0xFF, 0x0E, routine_length]); // LD DE, 0xFF80; LD C, length
    let copy = program.here();
    program.bytes(&[0x2A, 0x12, 0x13, 0x0D]); // LD A, (HL+); LD (DE), A; INC DE; DEC C
    program.jump_back(JR_NZ, copy);

    // Sound on, every channel on both sides: channel 1 with a sweep and an envelope, channel 2
    // with its length counting, then wave RAM and channels 3 and 4.
    program.write_registers(&[
        (0x26, 0x80),
        (0x24, 0x77),
        (0x25, 0xFF),
        (0x10, 0x16),
        (0x11, 0x80),
        (0x12, 0xF3),
        (0x13, 0x00),
        (0x14, 0x87),
        (0x16, 0x40),
        (0x17, 0xA1),
        (0x18, 0x50),
        (0x19, 0xC6),
    ]);
    program.bytes(&[0x21, 0x30, 0xFF, 0x3E, 0x1F, 0x0E, 0x10]); // LD HL, 0xFF30; LD A, 0x1F; LD C, 16
    let wave = program.here();
    program.bytes(&[0x22, 0xC6, 0x33, 0x0D]); // LD (HL+), A; ADD A, 0x33; DEC C
    program.jump_back(JR_NZ, wave);
    program.write_registers(&[
        (0x1A, 0x80),
        (0x1B, 0x20),
        (0x1C, 0x20),
        (0x1D, 0x00),
        (0x1E, 0xC7),
        (0x20, 0x00),
        (0x21, 0xF2),
        (0x22, 0x34),
        (0x23, 0xC0),
    ]);
    // The timer at 262 kHz, reloading from 0xF0.
    program.write_registers(&[(0x06, 0xF0), (0x07, 0x05)]);
    // The cartridge: RAM on, ROM bank 1, RAM bank 2, a byte of RAM, and the clock latched.
    program.bytes(&[0x3E, 0x0A, 0xEA, 0x00, 0x00]); // LD A, 0x0A; LD (0x0000), A
    program.bytes(&[0x3E, 0x01, 0xEA, 0x00, 0x20]); // LD A, 1; LD (0x2000), A
    program.bytes(&[0x3E, 0x02, 0xEA, 0x00, 0x40]); // LD A, 2; LD (0x4000), A
    program.bytes(&[0x3E, 0x77, 0xEA, 0x00, 0xA0]); // LD A, 0x77; LD (0xA000), A
    program.bytes(&[0xAF, 0xEA, 0x00, 0x60, 0x3C, 0xEA, 0x00, 0x60]); // XOR A; LD (0x6000), A; INC A; LD (0x6000), A

    // 40 objects at 0xC000 for OAM DMA, at 8 heights from line 96 on and across the whole line,
    // with every combination of flags, so lines 96 to 110 have the 10 objects a line can show.
    // Lines with objects are drawn a dot at a time, so the others have none.
    program.bytes(&[0x21, 0x00, 0xC0, 0x06, 0x28, 0x1E, 0x08]); // LD HL, 0xC000; LD B, 40; LD E, 8
    let object = program.here();
    #[rustfmt::skip]
    program.bytes(&[
        0x7B, 0xE6, 0x07, 0xC6, 0x70, 0x22, // LD A, E; AND 0x07; ADD A, 0x70; LD (HL+), A: Y
        0x7B, 0x22, 0x22,                   // LD A, E; LD (HL+), A: X; LD (HL+), A: tile
        0xE6, 0xF0, 0x22,                   // AND 0xF0; LD (HL+), A: flags
        0x7B, 0xC6, 0x07, 0x5F, 0x05,       // LD A, E; ADD A, 7; LD E, A; DEC B
    ]);
    program.jump_back(JR_NZ, object);

    // Scrolled by a few pixels, the window from line 90 and pixel 73, and the screen on with the
    // window and objects. The STAT interrupt for LY = LYC wakes the CPU from HALT (IME is
    // off, so no interrupt is taken).
    program.write_registers(&[
        (0x42, 0x05),
        (0x43, 0x03),
        (0x4A, 0x5A),
        (0x4B, 0x50),
        (0x47, 0xE4),
        (0x48, 0xD2),
        (0x49, 0x1B),
        (0x45, TRANSFER_LINE),
        (0x41, 0x40),
        (0xFF, 0x02),
        (0x40, 0xF3),
        (0x01, 0xA5),
    ]);
    if model.is_cgb() {
        program.write_register(0x70, 0x03); // SVBK: work RAM bank 3
        for (index_register, data_register) in [(0x68, 0x69), (0x6A, 0x6B)] {
            program.write_register(index_register, 0x80);
            program.bytes(&[0x0E, 0x40, 0x3E, 0x35]); // LD C, 64; LD A, 0x35
            let palette = program.here();
            program.bytes(&[0xE0, data_register, 0xC6, 0x29, 0x0D]); // LDH (data), A; ADD A, 0x29; DEC C
            program.jump_back(JR_NZ, palette);
        }
    }

    let frame = program.here();
    program.write_register(0x0F, 0x00); // IF = 0
    program.bytes(&[0x76]); // HALT until LY = LYC
    program.write_register(0x0F, 0x00);
    program.write_registers(&[(0x01, 0xA5), (0x02, 0x81), (0x19, 0xC6), (0x23, 0xC0)]);
    // The start of a Super Game Boy packet: a reset pulse and 12 of its bits.
    program.write_registers(&[(0x00, 0x00), (0x00, 0x30)]);
    for bit in 0..12 {
        let pulse = if (0xB91_u16 >> bit) & 1 == 1 {
            0x10
        } else {
            0x20
        };
        program.write_registers(&[(0x00, pulse), (0x00, 0x30)]);
    }
    if model.is_cgb() {
        // An HBlank VRAM DMA of 6 blocks from 0x4000 to 0x8100.
        program.write_registers(&[
            (0x51, 0x40),
            (0x52, 0x00),
            (0x53, 0x81),
            (0x54, 0x00),
            (0x55, 0x85),
        ]);
    }
    program.bytes(&[0x3E, 0xC0, 0xCD, 0x80, 0xFF]); // LD A, 0xC0; CALL 0xFF80: OAM DMA
    if model.is_cgb() {
        // Every fourth frame, counted in D, a speed switch: the CPU halts for about two frames.
        program.bytes(&[0x14, 0x7A, 0xE6, 0x03, 0x20, 0x06]); // INC D; LD A, D; AND 3; JR NZ, +6
        program.write_register(0x4D, 0x01); // KEY1: arm a speed switch
        program.bytes(&[0x10, 0x00]); // STOP
    }
    program.wait_for_line(LAST_BUSY_LINE);
    program.jump_to(frame);

    let program = program.0;
    assert!(BUSY_PROGRAM_START + program.len() < routine);
    rom[BUSY_PROGRAM_START..BUSY_PROGRAM_START + program.len()].copy_from_slice(&program);
    rom
}

/// How the test changes a stored value.
#[derive(Debug)]
enum Kind {
    /// A whole number of this many bytes, little-endian.
    Number(usize),
    /// A `bool`, one byte.
    Bool,
    /// Whether an `Option` holds a value: 0 or 1, one byte.
    OptionTag,
    /// A floating-point number of this many bytes.
    Float(usize),
    /// An enum variant, stored as its name after the name's length; every name it can have.
    Variant(Vec<String>),
}

/// A value in a save state that the test changes.
#[derive(Debug)]
struct StoredValue {
    /// Its path, such as `ppu.lcdc` or `bus.work_ram[0]`.
    path: String,
    /// Where its bytes are in the state.
    bytes: Range<usize>,
    kind: Kind,
    /// Where the lengths of the fields it is in are stored, outermost first: they change when a
    /// variant's name gets longer or shorter.
    field_lengths: Vec<usize>,
}

/// A value to write over a stored one.
#[derive(Clone, Debug)]
struct Replacement {
    bytes: Vec<u8>,
    /// How a failure shows it.
    shown: String,
}

impl StoredValue {
    /// The values the test writes over this one, all different from what it holds: the edges of
    /// its type, values next to what it holds and values just past the hardware's bounds.
    fn replacements(&self, state: &[u8]) -> Vec<Replacement> {
        let current = &state[self.bytes.clone()];
        let candidates: Vec<Replacement> = match &self.kind {
            Kind::Number(width) => number_replacements(current, *width),
            // Any byte but 0 reads as true.
            Kind::Bool => [0, 1, 0xFF]
                .into_iter()
                .map(|value| number_replacement(value, 1))
                .collect(),
            Kind::OptionTag => [0, 1, 2]
                .into_iter()
                .map(|value| number_replacement(value, 1))
                .collect(),
            Kind::Float(width) => float_replacements(*width),
            Kind::Variant(names) => names
                .iter()
                .map(String::as_str)
                .chain(["Unknown"])
                .map(|name| {
                    let mut bytes = vec![u8::try_from(name.len()).expect("a short name")];
                    bytes.extend_from_slice(name.as_bytes());
                    Replacement {
                        bytes,
                        shown: name.to_owned(),
                    }
                })
                .collect(),
        };
        let mut replacements: Vec<Replacement> = Vec::new();
        for candidate in candidates {
            if candidate.bytes != current
                && !replacements
                    .iter()
                    .any(|replacement| replacement.bytes == candidate.bytes)
            {
                replacements.push(candidate);
            }
        }
        replacements
    }
}

fn number_replacement(value: u64, width: usize) -> Replacement {
    Replacement {
        bytes: value.to_le_bytes()[..width].to_vec(),
        shown: format!("{value:#X}"),
    }
}

/// Edge values for a whole number of `width` bytes that holds `current`.
fn number_replacements(current: &[u8], width: usize) -> Vec<Replacement> {
    let current = little_endian(current);
    let max = u64::MAX >> (64 - 8 * width);
    let top_bit = 1 << (8 * width - 1);
    let mut values = vec![
        0,
        1,
        max,
        top_bit,
        current ^ (0x5555_5555_5555_5555 & max),
        current.wrapping_add(1) & max,
        current.wrapping_sub(1) & max,
    ];
    values.extend(PLAUSIBLE_BOUNDS.iter().filter(|&&bound| bound <= max));
    values
        .into_iter()
        .map(|value| number_replacement(value, width))
        .collect()
}

/// Edge values for a floating-point number of `width` bytes: ones that aren't numbers or are
/// infinite, the largest and smallest, a huge one, zero and a negative one.
fn float_replacements(width: usize) -> Vec<Replacement> {
    [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MAX,
        -f64::MAX,
        1e30,
        0.0,
        -1.0,
    ]
    .into_iter()
    .map(|value| Replacement {
        bytes: if width == 4 {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "an f32 edge value, rounded or infinite on purpose"
            )]
            let narrow = value as f32;
            narrow.to_le_bytes().to_vec()
        } else {
            value.to_le_bytes().to_vec()
        },
        shown: format!("{value}"),
    })
    .collect()
}

/// The number stored in up to 8 little-endian `bytes`.
fn little_endian(bytes: &[u8]) -> u64 {
    let mut padded = [0; 8];
    padded[..bytes.len()].copy_from_slice(bytes);
    u64::from_le_bytes(padded)
}

/// The `u32` stored at `position`, a length or a count.
fn stored_length(state: &[u8], position: usize) -> usize {
    let bytes = state[position..position + 4].try_into().unwrap();
    usize::try_from(u32::from_le_bytes(bytes)).unwrap()
}

/// A field of a struct in a save state, which `serializable.rs` writes as its name after the
/// name's length byte, then the value's length as a `u32`, then the value.
struct StoredField<'a> {
    name: &'a str,
    /// Where the value's length is.
    length_position: usize,
    value: Range<usize>,
}

/// The field at `position`, or `None` at the empty name that follows a struct's last field.
fn field_at(state: &[u8], position: usize) -> Option<StoredField<'_>> {
    let name_length = usize::from(state[position]);
    if name_length == 0 {
        return None;
    }
    let length_position = position + 1 + name_length;
    let start = length_position + 4;
    Some(StoredField {
        name: std::str::from_utf8(&state[position + 1..length_position])
            .expect("a field's name is text"),
        length_position,
        value: start..start + stored_length(state, length_position),
    })
}

/// Where the emulator's fields start in a save state, after the header's.
fn emulator_fields_start(state: &[u8]) -> usize {
    assert!(state.starts_with(b"NUBO SAVE STATE\0"));
    let mut position = MAGIC_AND_VERSION_LENGTH;
    while let Some(field) = field_at(state, position) {
        position = field.value.end;
    }
    position + 1
}

/// A saved state as the moments' checks see it: single values found by their path.
struct StateView<'a> {
    bytes: &'a [u8],
    layout: &'a Layout,
}

impl StateView<'_> {
    /// Where the value at `path` is, such as `ppu.pipeline.lcd_x`.
    fn find(&self, path: &str) -> Range<usize> {
        let mut position = emulator_fields_start(self.bytes);
        let mut walked = String::new();
        let mut names = path.split('.').peekable();
        while let Some(name) = names.next() {
            let field = std::iter::successors(field_at(self.bytes, position), |field| {
                field_at(self.bytes, field.value.end)
            })
            .find(|field| field.name == name)
            .unwrap_or_else(|| panic!("a save state stores no {path}"));
            if names.peek().is_none() {
                return field.value;
            }
            walked = join(&walked, name);
            position = field.value.start;
            if self.layout.encodings[&walked].starts_with("optional ") {
                assert_eq!(self.bytes[position], 1, "the state has no {walked}");
                position += 1;
            }
        }
        unreachable!("a path has a name")
    }

    /// The whole number or `bool` at `path`.
    fn number(&self, path: &str) -> u64 {
        little_endian(&self.bytes[self.find(path)])
    }

    /// The name of the variant at `path`.
    fn variant(&self, path: &str) -> &str {
        let bytes = self.find(path);
        std::str::from_utf8(&self.bytes[bytes.start + 1..bytes.end])
            .expect("a variant's name is text")
    }
}

/// A save state with the paths and places of all the values the test changes, read with the
/// help of the layout that says how each value is stored.
struct StoredState {
    bytes: Vec<u8>,
    values: Vec<StoredValue>,
}

impl StoredState {
    /// Reads `bytes`, a state saved by an emulator whose layout is `layout`. Damaging the header
    /// would only make the state belong to another game or model, so its values are left out.
    fn read(bytes: Vec<u8>, layout: &Layout) -> StoredState {
        let mut walker = Walker {
            state: &bytes,
            layout,
            values: Vec::new(),
        };
        let end = walker.fields(emulator_fields_start(&bytes), "", "", &[]);
        assert_eq!(end, bytes.len());
        let values = walker.values;
        StoredState { bytes, values }
    }

    /// This state with each of `changes` written over its value. The lengths of the fields
    /// around a value whose size changes follow it.
    fn changed(&self, changes: &[(&StoredValue, &Replacement)]) -> Vec<u8> {
        let mut changes = changes.to_vec();
        // From the end of the state back, so that a change never moves the bytes of one still
        // to be made, nor the field lengths in front of it.
        changes.sort_by_key(|(value, _)| std::cmp::Reverse(value.bytes.start));
        let mut state = self.bytes.clone();
        for (value, replacement) in changes {
            state.splice(value.bytes.clone(), replacement.bytes.iter().copied());
            for &position in &value.field_lengths {
                let length =
                    stored_length(&state, position) + replacement.bytes.len() - value.bytes.len();
                let length = u32::try_from(length).unwrap();
                state[position..position + 4].copy_from_slice(&length.to_le_bytes());
            }
        }
        state
    }
}

/// What `Emulator::save_state_layout` says about every stored value: how it is stored, and every
/// variant an enum can be stored as, by path.
struct Layout {
    encodings: HashMap<String, String>,
    variants: HashMap<String, Vec<String>>,
}

impl Layout {
    fn of(emulator: &Emulator) -> Layout {
        let mut encodings = HashMap::new();
        let mut variants: HashMap<String, Vec<String>> = HashMap::new();
        for entry in emulator.save_state_layout() {
            if let Some((path, variant)) = entry.split_once(" = ") {
                variants
                    .entry(path.to_owned())
                    .or_default()
                    .push(variant.to_owned());
            } else if let Some((path, encoding)) = entry.split_once(": ") {
                encodings.insert(path.to_owned(), encoding.to_owned());
            }
        }
        Layout {
            encodings,
            variants,
        }
    }
}

/// Finds the values of a save state, following its fields and the layout.
struct Walker<'a> {
    state: &'a [u8],
    layout: &'a Layout,
    values: Vec<StoredValue>,
}

fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}.{name}")
    }
}

impl Walker<'_> {
    /// Reads the fields of a struct from `position` on, and returns where they end.
    /// `layout_path` is the struct's path in the layout and `path` its path with the indices of
    /// arrays, which the layout leaves out; `field_lengths` are where the lengths of the fields
    /// around it are.
    fn fields(
        &mut self,
        mut position: usize,
        layout_path: &str,
        path: &str,
        field_lengths: &[usize],
    ) -> usize {
        while let Some(field) = field_at(self.state, position) {
            let field_layout_path = join(layout_path, field.name);
            let encoding = self
                .layout
                .encodings
                .get(&field_layout_path)
                .unwrap_or_else(|| panic!("the layout doesn't list {field_layout_path}"))
                .clone();
            let mut lengths = field_lengths.to_vec();
            lengths.push(field.length_position);
            let end = self.value(
                field.value.start,
                &encoding,
                &field_layout_path,
                &join(path, field.name),
                &lengths,
            );
            assert_eq!(
                end, field.value.end,
                "{field_layout_path} has bytes left over"
            );
            position = end;
        }
        position + 1
    }

    fn push(&mut self, path: String, bytes: Range<usize>, kind: Kind, field_lengths: &[usize]) {
        self.values.push(StoredValue {
            path,
            bytes,
            kind,
            field_lengths: field_lengths.to_vec(),
        });
    }

    /// Reads one value stored as `encoding` (as the layout writes it, such as `u8`, `fields`,
    /// `optional fields` or `8 × fields`) from `position` on, and returns where it ends.
    fn value(
        &mut self,
        position: usize,
        encoding: &str,
        layout_path: &str,
        path: &str,
        lengths: &[usize],
    ) -> usize {
        if let Some(inner) = encoding.strip_prefix("optional ") {
            let tag = position..position + 1;
            self.push(
                format!("{path} (whether it is there)"),
                tag,
                Kind::OptionTag,
                lengths,
            );
            return if self.state[position] == 1 {
                self.value(position + 1, inner, layout_path, path, lengths)
            } else {
                position + 1
            };
        }
        if let Some(item) = encoding.strip_prefix("list of ") {
            let count = position..position + 4;
            self.push(
                format!("{path} (its length)"),
                count,
                Kind::Number(4),
                lengths,
            );
            let size = match item {
                "u8" => 1,
                _ => item
                    .strip_suffix(" bytes")
                    .and_then(|size| size.parse().ok())
                    .unwrap_or_else(|| panic!("{path}: this test can't change a list of {item}")),
            };
            let length = stored_length(self.state, position) * size;
            return self.byte_array(position + 4, length, path, lengths);
        }
        if let Some((count, item)) = encoding.split_once(" × ") {
            let count: usize = count.parse().expect("a count");
            let mut position = position;
            for index in 0..count {
                let item_path = format!("{path}[{index}]");
                if item == "fields" {
                    position = self.fields(position, layout_path, &item_path, lengths);
                } else {
                    position = self.value(position, item, layout_path, &item_path, lengths);
                    // Of long arrays of numbers, only the first and the last.
                    if count > 4 && index != 0 && index != count - 1 {
                        self.values.pop();
                    }
                }
            }
            return position;
        }
        if let Some(length) = encoding.strip_suffix(" bytes") {
            let length = length.parse().expect("a length");
            return self.byte_array(position, length, path, lengths);
        }
        let (kind, width) = match encoding {
            "fields" => return self.fields(position, layout_path, path, lengths),
            "variant" | "variant and u8" => {
                let names = self
                    .layout
                    .variants
                    .get(layout_path)
                    .unwrap_or_else(|| panic!("the layout lists no variants of {layout_path}"))
                    .clone();
                let end = position + 1 + usize::from(self.state[position]);
                self.push(
                    path.to_owned(),
                    position..end,
                    Kind::Variant(names),
                    lengths,
                );
                if encoding == "variant" {
                    return end;
                }
                let number = end..end + 1;
                self.push(format!("{path} (its u8)"), number, Kind::Number(1), lengths);
                return end + 1;
            }
            "text" => return position + 4 + stored_length(self.state, position),
            "bool" => (Kind::Bool, 1),
            "u8" => (Kind::Number(1), 1),
            "u16" | "i16" => (Kind::Number(2), 2),
            "u32" => (Kind::Number(4), 4),
            "u64" => (Kind::Number(8), 8),
            "f32" => (Kind::Float(4), 4),
            "f64" => (Kind::Float(8), 8),
            _ => panic!("{path} is stored as {encoding}, which this test doesn't know: teach it"),
        };
        self.push(path.to_owned(), position..position + width, kind, lengths);
        position + width
    }

    /// Notes the bytes of an array of `length` bytes at `position`: all of them when it is
    /// short, else the first and the last (see `LONGEST_ARRAY_CHANGED_WHOLE`).
    fn byte_array(
        &mut self,
        position: usize,
        length: usize,
        path: &str,
        lengths: &[usize],
    ) -> usize {
        for index in 0..length {
            if length <= LONGEST_ARRAY_CHANGED_WHOLE || index == 0 || index == length - 1 {
                let at = position + index;
                self.push(
                    format!("{path}[{index}]"),
                    at..at + 1,
                    Kind::Number(1),
                    lengths,
                );
            }
        }
        position + length
    }
}

/// The states `model` saves at each of its moments running `rom`, with the moments.
fn states_at_moments(model: Model, rom: &[u8]) -> Vec<(&'static Moment, StoredState)> {
    let mut emulator = booted(model, rom);
    let layout = Layout::of(&emulator);
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for _ in 0..WARM_UP_FRAMES {
        emulator.run_frame(&mut left, &mut right);
    }
    let mut wanted: Vec<&Moment> = MOMENTS.iter().filter(|moment| (moment.on)(model)).collect();
    let mut states = Vec::new();
    for _ in 0..MOMENT_SEARCH_STEPS {
        if wanted.is_empty() {
            break;
        }
        emulator.step();
        let bytes = emulator.save_state();
        let view = StateView {
            bytes: &bytes,
            layout: &layout,
        };
        if let Some(index) = wanted.iter().position(|moment| (moment.found)(&view)) {
            states.push((wanted.remove(index), StoredState::read(bytes, &layout)));
        }
    }
    let missing: Vec<&str> = wanted.iter().map(|moment| moment.name).collect();
    assert!(missing.is_empty(), "{model:?} never got to {missing:?}");
    states
}

/// Loads damaged states for one model and runs them, noting what happened.
struct Tester {
    model: Model,
    rom: Vec<u8>,
    /// The emulator states load into. One emulator takes every state, as rewinding in RetroArch
    /// does, since a new one would make the power-on state every load starts from again, which
    /// takes long on a Super Game Boy. A state that loads replaces everything a state stores.
    emulator: Emulator,
    tried: usize,
    loaded: usize,
    /// The damaged states that made the emulator panic, and the panics.
    panics: Vec<String>,
}

impl Tester {
    fn new(model: Model, rom: Vec<u8>) -> Self {
        Tester {
            model,
            emulator: Self::emulator(model, &rom),
            rom,
            tried: 0,
            loaded: 0,
            panics: Vec::new(),
        }
    }

    fn emulator(model: Model, rom: &[u8]) -> Emulator {
        let mut emulator = Emulator::new(model);
        emulator.set_sample_rate(48_000);
        emulator.load_rom(rom, empty_cartridge_effects()).unwrap();
        emulator
    }

    /// Loads `state`, described by `case`, and if it loads, runs two frames and saves it again.
    /// A panic is noted rather than stopping the test, and the emulator replaced.
    fn try_state(&mut self, state: &[u8], case: impl FnOnce() -> String) {
        self.tried += 1;
        let emulator = &mut self.emulator;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if emulator.load_state(state).is_err() {
                return false;
            }
            let (mut left, mut right) = (Vec::new(), Vec::new());
            emulator.run_frame(&mut left, &mut right);
            emulator.run_frame(&mut left, &mut right);
            emulator.save_state();
            true
        }));
        match result {
            Ok(loaded) => self.loaded += usize::from(loaded),
            Err(payload) => {
                let message = payload
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| payload.downcast_ref::<&str>().copied())
                    .unwrap_or("a panic without a message");
                self.panics
                    .push(format!("{:?}, {}: {message}", self.model, case()));
                self.emulator = Self::emulator(self.model, &self.rom);
            }
        }
    }
}

/// How many random pairs of values `tested` changes: its own count, or the one in
/// `NUBO_DAMAGED_STATE_PAIRS` when that is set (see the module docs).
fn pair_count(tested: &TestedModel) -> usize {
    std::env::var("NUBO_DAMAGED_STATE_PAIRS").map_or(tested.pairs, |count| {
        count
            .parse()
            .expect("NUBO_DAMAGED_STATE_PAIRS is a whole number")
    })
}

/// Changes the values of `tested`'s states one at a time, then random pairs of values together,
/// and loads and runs each damaged state.
fn damage_states(tested: &TestedModel) -> Tester {
    let rom = busy_game(tested.model, tested.cartridge_type);
    let moments = states_at_moments(tested.model, &rom);
    let mut tester = Tester::new(tested.model, rom);
    for (index, (moment, state)) in moments.iter().enumerate() {
        for value in &state.values {
            let changed_alone = tested
                .single_values
                .iter()
                .any(|start| value.path.starts_with(start));
            if !changed_alone || home_moment(&moments, &value.path) != index {
                continue;
            }
            for replacement in value.replacements(&state.bytes) {
                tester.try_state(&state.changed(&[(value, &replacement)]), || {
                    format!("{}, {} = {}", moment.name, value.path, replacement.shown)
                });
            }
        }
    }
    let mut random = Random(tested.seed);
    for pair in 0..pair_count(tested) {
        let (moment, state) = &moments[pair % moments.len()];
        let mut pick = || {
            let value = &state.values[random.below(state.values.len())];
            let replacements = value.replacements(&state.bytes);
            (
                value,
                replacements[random.below(replacements.len())].clone(),
            )
        };
        let (first, first_replacement) = pick();
        let (second, second_replacement) = pick();
        if first.bytes == second.bytes {
            continue;
        }
        let changes = [(first, &first_replacement), (second, &second_replacement)];
        tester.try_state(&state.changed(&changes), || {
            format!(
                "{}, {} = {} and {} = {}",
                moment.name,
                first.path,
                first_replacement.shown,
                second.path,
                second_replacement.shown
            )
        });
    }
    tester
}

#[test]
fn damaged_save_states_are_refused_or_run_without_panicking() {
    // One thread per model keeps this test at a few seconds.
    let testers: Vec<Tester> = std::thread::scope(|scope| {
        let threads: Vec<_> = TESTED_MODELS
            .iter()
            .map(|tested| scope.spawn(|| damage_states(tested)))
            .collect();
        threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect()
    });
    let panics: Vec<&str> = testers
        .iter()
        .flat_map(|tester| tester.panics.iter().map(String::as_str))
        .collect();
    assert!(
        panics.is_empty(),
        "{} damaged states made the emulator panic:\n{}",
        panics.len(),
        panics.join("\n")
    );
    for tester in &testers {
        assert!(
            tester.loaded * 4 > tester.tried,
            "only {} of {} damaged states loaded on {:?}",
            tester.loaded,
            tester.tried,
            tester.model
        );
    }
}

/// The state `model` saves after running the busy game with `cartridge_type` for a while.
fn busy_state(model: Model, cartridge_type: u8) -> (Vec<u8>, StoredState) {
    let rom = busy_game(model, cartridge_type);
    let mut emulator = booted(model, &rom);
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for _ in 0..WARM_UP_FRAMES {
        emulator.run_frame(&mut left, &mut right);
    }
    let layout = Layout::of(&emulator);
    let state = StoredState::read(emulator.save_state(), &layout);
    (rom, state)
}

/// `state` with the value at `path` replaced by `bytes`.
fn with_value(state: &StoredState, path: &str, bytes: Vec<u8>) -> Vec<u8> {
    let value = state
        .values
        .iter()
        .find(|value| value.path == path)
        .unwrap_or_else(|| panic!("a save state stores no {path}"));
    let replacement = Replacement {
        bytes,
        shown: String::new(),
    };
    state.changed(&[(value, &replacement)])
}

/// A variant's name as a state stores it, after its length.
fn variant_bytes(name: &str) -> Vec<u8> {
    let mut bytes = vec![u8::try_from(name.len()).expect("a short name")];
    bytes.extend_from_slice(name.as_bytes());
    bytes
}

/// The PPU and the APU keep copies of the CPU's speed and the system counter, which the speed
/// switch and the timers own. A state where a copy differs, or where a Game Boy's serial port
/// uses the Game Boy Color's fast clock, is one Nubo never saves.
#[test]
fn states_whose_copies_of_the_speed_or_system_counter_differ_are_refused() {
    let (rom, state) = busy_state(Model::CgbE, 0x10);
    let mut emulator = Tester::emulator(Model::CgbE, &rom);
    emulator.load_state(&state.bytes).unwrap();
    let view = StateView {
        bytes: &state.bytes,
        layout: &Layout::of(&emulator),
    };
    let double_speed = view.number("speed_switch.cgb_double_speed") != 0;
    let system_counter = u16::try_from(view.number("apu.system_counter")).unwrap();
    let other_timing = if double_speed {
        "Cgb"
    } else {
        "CgbDoubleSpeed"
    };
    let damaged = [
        (
            "apu.system_counter",
            system_counter.wrapping_add(4).to_le_bytes().to_vec(),
        ),
        ("apu.cgb_double_speed", vec![u8::from(!double_speed)]),
        ("ppu.timing", variant_bytes(other_timing)),
        (
            "speed_switch.cgb_double_speed",
            vec![u8::from(!double_speed)],
        ),
    ];
    for (path, bytes) in damaged {
        assert!(
            emulator
                .load_state(&with_value(&state, path, bytes))
                .is_err(),
            "a state with another {path} loaded"
        );
    }

    let (rom, state) = busy_state(Model::Dmg, 0x03);
    let mut emulator = Tester::emulator(Model::Dmg, &rom);
    emulator.load_state(&state.bytes).unwrap();
    let fast_serial_clock = with_value(&state, "serial.is_high_speed_clock", vec![1]);
    assert!(emulator.load_state(&fast_serial_clock).is_err());
}

/// A refused state that lacks the Super Game Boy leaves what save states don't keep as it was:
/// the player's border, its look and palette, and the last frame with its border.
#[test]
fn a_refused_state_without_the_super_game_boy_keeps_the_players_choices() {
    let (rom, state) = busy_state(Model::Sgb, 0x03);
    let mut emulator = Tester::emulator(Model::Sgb, &rom);
    emulator.load_state(&state.bytes).unwrap();
    let (mut left, mut right) = (Vec::new(), Vec::new());
    emulator.run_frame(&mut left, &mut right);
    emulator.set_sgb_border(SgbBorder::Nubo);
    emulator.set_sgb_border_time(SgbBorderTime::Day);
    emulator.set_sgb_player_palette(Some([0x7FFF, 0x5294, 0x294A, 0x0000]));
    let palette = emulator.sgb_player_palette();
    let frame = emulator.frame_with_border().unwrap().to_vec();

    let without_sgb = with_value(&state, "ppu.sgb (whether it is there)", vec![0]);
    assert!(emulator.load_state(&without_sgb).is_err());

    assert_eq!(emulator.sgb_border(), SgbBorder::Nubo);
    assert_eq!(emulator.sgb_border_time(), SgbBorderTime::Day);
    assert_eq!(emulator.sgb_player_palette(), palette);
    assert_eq!(emulator.frame_with_border().unwrap(), frame);
}

/// A stored value a test damages: its path, the bytes written over it and the number it must hold
/// once the state has loaded.
type DamagedValue = (&'static str, &'static [u8], u64);

/// The bank registers of MBC1, MBC2, MBC5 and HuC1 hold only the bits the hardware has, and
/// MBC1's and MBC2's ROM bank isn't 0, also when a damaged state stores more, as when a game
/// writes them.
#[test]
fn a_loaded_controller_keeps_only_the_bits_its_bank_registers_have() {
    let cases: [(u8, &[DamagedValue]); 5] = [
        (
            0x03,
            &[
                ("cartridge.mapper.mbc1.bank1", &[0xE0], 1),
                ("cartridge.mapper.mbc1.bank2", &[0xFF], 3),
            ],
        ),
        (
            0x06,
            &[("cartridge.mapper.mbc2.rom_bank_number", &[0xF0], 1)],
        ),
        (
            0x1B,
            &[
                (
                    "cartridge.mapper.mbc5.rom_bank_number",
                    &[0xFF, 0xFF],
                    0x1FF,
                ),
                ("cartridge.mapper.mbc5.ram_bank_number", &[0xFF], 0x0F),
            ],
        ),
        (
            0x1E,
            &[("cartridge.mapper.mbc5.ram_bank_number", &[0xFF], 0x07)],
        ),
        (
            0xFF,
            &[
                ("cartridge.mapper.huc1.rom_bank_number", &[0xFF], 0x3F),
                ("cartridge.mapper.huc1.ram_bank_number", &[0xFF], 0x03),
            ],
        ),
    ];
    for (cartridge_type, values) in cases {
        let (rom, state) = busy_state(Model::Dmg, cartridge_type);
        let mut emulator = Tester::emulator(Model::Dmg, &rom);
        let mut damaged = state.bytes.clone();
        for (path, bytes, _) in values {
            let stored = StoredState::read(damaged, &Layout::of(&emulator));
            damaged = with_value(&stored, path, bytes.to_vec());
        }
        emulator.load_state(&damaged).unwrap();

        let layout = Layout::of(&emulator);
        let saved = emulator.save_state();
        let view = StateView {
            bytes: &saved,
            layout: &layout,
        };
        for (path, _, expected) in values {
            assert_eq!(view.number(path), *expected, "{path}");
        }
    }
}

/// An MBC3's registers hold only the bits the hardware has, also when a damaged state stores
/// more, as they do when a game writes them or a `.rtc` file loads.
#[test]
fn a_loaded_mbc3_keeps_only_the_bits_its_registers_have() {
    let (rom, state) = busy_state(Model::CgbE, 0x10);
    let mut emulator = Tester::emulator(Model::CgbE, &rom);
    let mut damaged = state.bytes.clone();
    for path in [
        "cartridge.mapper.mbc3.rtc.seconds",
        "cartridge.mapper.mbc3.rtc.minutes",
        "cartridge.mapper.mbc3.rtc.hours",
        "cartridge.mapper.mbc3.ram_bank_or_rtc_register",
        "cartridge.mapper.mbc3.rom_bank_number",
    ] {
        let stored = StoredState::read(damaged, &Layout::of(&emulator));
        damaged = with_value(&stored, path, vec![0xFF]);
    }
    let stored = StoredState::read(damaged, &Layout::of(&emulator));
    damaged = with_value(&stored, "cartridge.mapper.mbc3.rtc.days", vec![0xFF, 0xFF]);
    emulator.load_state(&damaged).unwrap();

    let rtc = emulator.rtc_state(crate::RtcClock::EmulatedTime).unwrap();
    assert_eq!(
        (rtc.seconds, rtc.minutes, rtc.hours, rtc.days),
        (63, 63, 31, 0x1FF)
    );
    let layout = Layout::of(&emulator);
    let saved = emulator.save_state();
    let view = StateView {
        bytes: &saved,
        layout: &layout,
    };
    assert_eq!(
        view.number("cartridge.mapper.mbc3.ram_bank_or_rtc_register"),
        0x0F
    );
    assert_eq!(view.number("cartridge.mapper.mbc3.rom_bank_number"), 0x7F);
}
