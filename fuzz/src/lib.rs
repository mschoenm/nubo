//! What the fuzz targets share: a minimal cartridge, emulators that have
//! already run their boot ROM, and the game the save-state targets load.

use nubo_core::{Emulator, Model, NoSaves, RtcClock};
use std::cell::RefCell;
use std::collections::HashMap;
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

/// The Nintendo logo every cartridge header holds at 0x0104, which the boot
/// ROM checks.
const LOGO: [u8; 48] = [
    0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D,
    0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
    0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
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

/// How many boots [`start`] keeps per thread before it forgets them all, so
/// a long run of the `rom` target, whose headers keep changing, doesn't fill
/// the memory.
const BOOTS_KEPT: usize = 4096;

/// The most CPU steps a boot ROM may take; a real boot takes far fewer.
const BOOT_STEPS: u64 = 4_000_000;

/// The boots [`start`] keeps: one per model and part of a ROM the boot ROM
/// reads, `None` for one that didn't finish.
type Boots = HashMap<(Model, Vec<u8>), Option<Emulator>>;

/// Whether the boot ROM accepts `rom`'s header: the Nintendo logo and the
/// header checksum. One it rejects hangs it, as it does a real console, so
/// such a cartridge never starts and isn't worth a boot. (Game Boy Color boot
/// ROMs check only half the logo, so this skips a few headers they would
/// start.)
fn header_boots(rom: &[u8]) -> bool {
    let Some(header) = rom.get(0x134..0x14E) else {
        return false;
    };
    let checksum = header[..0x19]
        .iter()
        .fold(0u8, |sum, &byte| sum.wrapping_sub(byte).wrapping_sub(1));
    rom.get(0x104..0x134) == Some(&LOGO[..]) && header[0x19] == checksum
}

/// An emulator of `model` with `rom` inserted that has just run its boot ROM,
/// or `None` when the ROM doesn't load or doesn't boot. The boot depends on the part of the
/// ROM the boot ROM reads ([`Emulator::boot_rom_input`]), so a boot is run
/// once for each model and such part, as the test ROM runner does, and copied
/// with [`Emulator::clone_with_rom`] after that: every ROM starts exactly where
/// a player's would.
pub fn start(model: Model, rom: &[u8]) -> Option<Emulator> {
    thread_local! {
        static BOOTS: RefCell<Boots> = RefCell::new(HashMap::new());
    }
    if !header_boots(rom) {
        return None;
    }
    let input = rom.get(Emulator::boot_rom_input(model))?.to_vec();
    BOOTS.with(|boots| {
        let mut boots = boots.borrow_mut();
        let key = (model, input);
        if !boots.contains_key(&key) {
            let mut emulator = Emulator::new(model);
            emulator.load_rom(rom, effects()).ok()?;
            let mut steps = 0;
            while emulator.is_running_boot_rom() && steps < BOOT_STEPS {
                emulator.step();
                steps += 1;
            }
            if boots.len() >= BOOTS_KEPT {
                boots.clear();
            }
            let booted = (!emulator.is_running_boot_rom()).then_some(emulator);
            boots.insert(key.clone(), booted);
        }
        boots[&key].as_ref()?.clone_with_rom(rom, effects()).ok()
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
