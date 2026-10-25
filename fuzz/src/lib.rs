//! What the fuzz targets share: a minimal cartridge, emulators that have
//! already run their boot ROM, and the game the save-state targets load.

use nubo_core::{Emulator, Model, NoSaves, RtcClock};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

/// The hardware models the targets pick from, by a byte of the input: every
/// model. The six the targets started with stay first, so half the inputs of
/// an older corpus still pick the model they picked then.
pub const MODELS: [Model; 12] = [
    Model::Dmg,
    Model::Mgb,
    Model::Sgb,
    Model::CgbE,
    Model::Agb,
    Model::Cgb0,
    Model::Dmg0,
    Model::Sgb2,
    Model::CgbA,
    Model::CgbB,
    Model::CgbC,
    Model::CgbD,
];

/// The Nintendo logo every cartridge header holds at 0x0104. A real
/// console's boot ROM checks it; the ones Nubo ships don't.
const LOGO: [u8; 48] = [
    0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D,
    0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
    0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
];

/// Cartridge effects that keep nothing and run the cartridge clock on
/// emulated time, so every run of an input is the same.
pub fn effects() -> Box<NoSaves> {
    effects_on(RtcClock::EmulatedTime)
}

/// Cartridge effects that keep nothing and run the cartridge clock on
/// `clock`. The wall clock always reads zero here, so it stands still, and
/// every run of an input is still the same.
pub fn effects_on(clock: RtcClock) -> Box<NoSaves> {
    Box::new(NoSaves { rtc_clock: clock })
}

/// A cartridge with a header a real console accepts (the Nintendo logo and
/// a right header checksum) and that does nothing, with the given cartridge
/// type and ROM and RAM size codes from its header.
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
const BOOTS_KEPT: usize = 1024;

/// How many bytes of ROM the kept boots may hold together before [`start`]
/// forgets them all. Each boot keeps its own copy of its ROM, and with real
/// games as seeds (up to 8 MiB each) [`BOOTS_KEPT`] alone could pass
/// libFuzzer's default memory limit of 2 GB.
const ROM_BYTES_KEPT: usize = 256 << 20;

/// The most CPU steps a boot ROM may take; a real boot takes far fewer.
const BOOT_STEPS: u64 = 4_000_000;

/// The boots [`start_on`] keeps: one per model, cartridge clock (whether it
/// is the wall clock) and part of a ROM the boot ROM reads, `None` for one
/// that didn't finish, and how many bytes of ROM they hold together.
#[derive(Default)]
struct Boots {
    emulators: HashMap<(Model, bool, Vec<u8>), Option<Emulator>>,
    rom_bytes: usize,
}

/// An emulator of `model` with `rom` inserted that has just run its boot ROM,
/// with the cartridge clock on emulated time, or `None` when the ROM doesn't
/// load or doesn't boot. See [`start_on`].
pub fn start(model: Model, rom: &[u8]) -> Option<Emulator> {
    start_on(model, rom, RtcClock::EmulatedTime)
}

/// An emulator of `model` with `rom` inserted that has just run its boot ROM,
/// with the cartridge clock on `clock`, or `None` when the ROM doesn't load or
/// doesn't boot. The boot depends on the part of the ROM the boot ROM reads
/// ([`Emulator::boot_rom_input`]), so a boot is run once for each model,
/// clock and such part, as the test ROM runner does, and copied with
/// [`Emulator::clone_with_rom`] after that: every ROM starts exactly where a
/// player's would. Nubo's boot ROMs check neither the logo nor the header
/// checksum, so every header boots, as it would for a player. A zip file is
/// unpacked by loading it, and a ROM too short to hold that part fails to
/// load, so both are booted on their own every time.
pub fn start_on(model: Model, rom: &[u8], clock: RtcClock) -> Option<Emulator> {
    thread_local! {
        static BOOTS: RefCell<Boots> = RefCell::new(Boots::default());
    }
    if rom.starts_with(b"PK\x03\x04") {
        return boot(model, rom, clock);
    }
    let Some(input) = rom.get(Emulator::boot_rom_input(model)) else {
        return boot(model, rom, clock);
    };
    BOOTS.with(|boots| {
        let mut boots = boots.borrow_mut();
        let key = (model, clock == RtcClock::WallClock, input.to_vec());
        if !boots.emulators.contains_key(&key) {
            if boots.emulators.len() >= BOOTS_KEPT || boots.rom_bytes + rom.len() > ROM_BYTES_KEPT {
                *boots = Boots::default();
            }
            boots.rom_bytes += rom.len();
            boots.emulators.insert(key.clone(), boot(model, rom, clock));
        }
        boots.emulators[&key]
            .as_ref()?
            .clone_with_rom(rom, effects_on(clock))
            .ok()
    })
}

/// Powers on an emulator of `model` with `rom` inserted and runs its boot
/// ROM, or `None` when the ROM doesn't load or the boot doesn't finish.
fn boot(model: Model, rom: &[u8], clock: RtcClock) -> Option<Emulator> {
    let mut emulator = Emulator::new(model);
    emulator.load_rom(rom, effects_on(clock)).ok()?;
    let mut steps = 0;
    while emulator.is_running_boot_rom() && steps < BOOT_STEPS {
        emulator.step();
        steps += 1;
    }
    (!emulator.is_running_boot_rom()).then_some(emulator)
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
