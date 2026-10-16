//! What the fuzz targets share: a minimal cartridge, emulators that have
//! already run their boot ROM, and the game the save-state targets load.

use nubo_core::{Emulator, Model, NoSaves, RtcClock};
use std::cell::OnceCell;
use std::sync::OnceLock;

/// The hardware models the targets pick from, by a byte of the input.
pub const MODELS: [Model; 6] = [
    Model::Dmg,
    Model::Mgb,
    Model::Sgb,
    Model::CgbE,
    Model::Agb,
    Model::Cgb0,
];

/// Cartridge effects that keep nothing and run the cartridge clock on
/// emulated time, so every run of an input is the same.
pub fn effects() -> Box<NoSaves> {
    Box::new(NoSaves {
        rtc_clock: RtcClock::EmulatedTime,
    })
}

/// A cartridge the boot ROM accepts (the Nintendo logo and a right header
/// checksum) and that does nothing, with the given cartridge type and ROM
/// and RAM size codes from its header.
pub fn minimal_rom(cartridge_type: u8, rom_size: u8, ram_size: u8, color: bool) -> Vec<u8> {
    const LOGO: [u8; 48] = [
        0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00,
        0x0D, 0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD,
        0xD9, 0x99, 0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB,
        0xB9, 0x33, 0x3E,
    ];
    let banks = 2usize << rom_size.min(8);
    let mut rom = vec![0u8; 0x4000 * banks];
    rom[0x104..0x134].copy_from_slice(&LOGO);
    rom[0x134..0x13C].copy_from_slice(b"FUZZTEST");
    if color {
        rom[0x143] = 0x80;
    }
    // Super Game Boy support, which needs the old licensee code 0x33.
    rom[0x146] = 0x03;
    rom[0x14B] = 0x33;
    rom[0x147] = cartridge_type;
    rom[0x148] = rom_size;
    rom[0x149] = ram_size;
    let checksum = rom[0x134..0x14D]
        .iter()
        .fold(0u8, |sum, &byte| sum.wrapping_sub(byte).wrapping_sub(1));
    rom[0x14D] = checksum;
    rom
}

/// An emulator of `model` that has run its boot ROM, made once per model
/// and thread, so a target can skip the boot with
/// [`Emulator::clone_with_rom`]. Every model has its own, since a save state
/// can name any of them.
pub fn booted(model: Model) -> &'static Emulator {
    thread_local! {
        static BOOTED: [OnceCell<&'static Emulator>; Model::ALL.len()] =
            const { [const { OnceCell::new() }; Model::ALL.len()] };
    }
    let index = Model::ALL
        .iter()
        .position(|&each| each == model)
        .expect("Model::ALL has every model");
    BOOTED.with(|booted| {
        *booted[index].get_or_init(|| {
            let mut emulator = Emulator::new(model);
            emulator
                .load_rom(&minimal_rom(0, 0, 0, true), effects())
                .expect("the minimal cartridge loads");
            let mut steps = 0u64;
            while emulator.is_running_boot_rom() && steps < 50_000_000 {
                emulator.step();
                steps += 1;
            }
            Box::leak(Box::new(emulator))
        })
    })
}

/// The model a byte of the input picks.
pub fn model_from_byte(byte: u8) -> Model {
    MODELS[usize::from(byte) % MODELS.len()]
}

/// Tobu Tobu Girl Deluxe, one of the benchmark games, whose kept save
/// states (`save-states/`) seed the save-state targets. `cargo xtask
/// benchmark-roms`, run from the repository, downloads it.
pub fn game() -> &'static [u8] {
    static GAME: OnceLock<Vec<u8>> = OnceLock::new();
    GAME.get_or_init(|| {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../target/benchmark-roms/tobudx.gb"
        );
        std::fs::read(path).unwrap_or_else(|error| {
            panic!("{path}: {error}; run `cargo xtask benchmark-roms` first")
        })
    })
}

/// Runs `frames` frames, dropping the sound.
pub fn run_frames(emulator: &mut Emulator, frames: usize) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    for _ in 0..frames {
        emulator.run_frame(&mut left, &mut right);
        left.clear();
        right.clear();
    }
}
