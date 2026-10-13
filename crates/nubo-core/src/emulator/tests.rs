//! Checks that a copy of a booted emulator with another game inserted is in
//! exactly the state that booting that game would have left it in, which
//! test runners rely on to skip the boot ROM, that a loaded save state
//! restores every field save states are meant to keep, that
//! battery-backed RAM reaches the frontend once per call, and that
//! `run_frame` keeps to the frame boundaries and hands out every sample.

use crate::address_bus::effects::empty_cartridge_effects;
use crate::{CartridgeEffects, DOTS_PER_FRAME, Emulator, Model};
use std::sync::{Arc, Mutex};

const ROM_SIZE: usize = 0x8000;
/// More instructions than any boot ROM runs.
const BOOT_STEP_LIMIT: usize = 10_000_000;
const HEADER_START: usize = 0x0100;
const CGB_FLAG: usize = 0x0143;
const SGB_FLAG: usize = 0x0146;
const CARTRIDGE_TYPE: usize = 0x0147;
const RAM_SIZE: usize = 0x0149;
const OLD_LICENSEE: usize = 0x014B;
const HEADER_CHECKSUM: usize = 0x014D;
const GLOBAL_CHECKSUM: usize = 0x014E;

/// A cartridge header (0x0100-0x014F) for a game with the given CGB flag,
/// SGB flag, cartridge type and old licensee code. The logo is a made-up
/// pattern; the boot ROMs Nubo ships don't check it. The title is made up too,
/// with a checksum (0xDB) the CGB boot ROM's palette table lists, so a Nintendo
/// game gets a CGB palette.
fn header(cgb_flag: u8, sgb_flag: u8, cartridge_type: u8, old_licensee: u8) -> Vec<u8> {
    let mut header = vec![0; 0x50];
    let mut set = |address: usize, value: u8| header[address - HEADER_START] = value;
    // NOP; JP 0x0150
    for (offset, byte) in [0x00, 0xC3, 0x50, 0x01].into_iter().enumerate() {
        set(HEADER_START + offset, byte);
    }
    for offset in 0..0x30 {
        set(0x0104 + offset, (offset as u8).wrapping_mul(37) ^ 0x5A);
    }
    for (offset, byte) in b"TEST GAME A".iter().enumerate() {
        set(0x0134 + offset, *byte);
    }
    set(CGB_FLAG, cgb_flag);
    set(SGB_FLAG, sgb_flag);
    set(CARTRIDGE_TYPE, cartridge_type);
    set(RAM_SIZE, if cartridge_type == 0 { 0 } else { 0x03 });
    set(OLD_LICENSEE, old_licensee);
    set(GLOBAL_CHECKSUM, 0x12);
    set(GLOBAL_CHECKSUM + 1, 0x34);
    header[HEADER_CHECKSUM - HEADER_START] = header
        [0x0134 - HEADER_START..HEADER_CHECKSUM - HEADER_START]
        .iter()
        .fold(0u8, |sum, byte| sum.wrapping_sub(*byte).wrapping_sub(1));
    header
}

/// A ROM that has `header`'s bytes where `model`'s boot ROM can see them and
/// `filler(address)` everywhere else.
fn rom(model: Model, header: &[u8], filler: impl Fn(usize) -> u8) -> Vec<u8> {
    let visible = Emulator::boot_rom_input(model);
    (0..ROM_SIZE)
        .map(|address| {
            if visible.contains(&address) {
                header[address - HEADER_START]
            } else {
                filler(address)
            }
        })
        .collect()
}

/// An emulator for `model` with `rom` inserted, run until the boot ROM hands
/// over to the cartridge.
fn booted(model: Model, rom: &[u8]) -> Emulator {
    let mut emulator = Emulator::new(model);
    emulator.set_sample_rate(48_000);
    emulator.load_rom(rom, empty_cartridge_effects()).unwrap();
    let mut steps = 0;
    while emulator.is_running_boot_rom() {
        emulator.step();
        steps += 1;
        assert!(
            steps < BOOT_STEP_LIMIT,
            "the {model:?} boot ROM never finished"
        );
    }
    emulator
}

/// Fails with the first difference between two emulator states.
fn assert_same_state(copied: &Emulator, booted: &Emulator, context: &str) {
    let copied = format!("{copied:?}");
    let booted = format!("{booted:?}");
    if let Some(position) = copied
        .bytes()
        .zip(booted.bytes())
        .position(|(copied, booted)| copied != booted)
        .or_else(|| (copied.len() != booted.len()).then(|| copied.len().min(booted.len())))
    {
        let excerpt = |state: &str| {
            let start = state.floor_char_boundary(position.saturating_sub(200));
            let end = state.ceil_char_boundary((position + 100).min(state.len()));
            state[start..end].to_string()
        };
        panic!(
            "{context}: the copy differs from a real boot\n copy: ...{}\n boot: ...{}",
            excerpt(&copied),
            excerpt(&booted)
        );
    }
}

#[test]
fn clone_with_rom_matches_a_real_boot() {
    let headers = [
        ("DMG game", header(0x00, 0x00, 0x00, 0x33)),
        ("SGB game with an MBC5", header(0x00, 0x03, 0x1B, 0x33)),
        ("CGB game", header(0x80, 0x00, 0x1B, 0x33)),
        ("CGB-only game", header(0xC0, 0x00, 0x00, 0x33)),
        (
            "Nintendo DMG game, which gets a CGB palette",
            header(0x00, 0x00, 0x00, 0x01),
        ),
    ];
    // One thread per model keeps this test at a few seconds.
    std::thread::scope(|scope| {
        for model in Model::ALL {
            let headers = &headers;
            scope.spawn(move || {
                for (name, header) in headers {
                    let first = rom(model, header, |_| 0);
                    let second = rom(model, header, |address| (address * 7 + address / 251) as u8);

                    let copied = booted(model, &first)
                        .clone_with_rom(&second, empty_cartridge_effects())
                        .unwrap();
                    let booted = booted(model, &second);
                    assert_same_state(&copied, &booted, &format!("{model:?}, {name}"));
                }
            });
        }
    });
}

const CART_TYPE_MBC1_RAM_BATTERY: u8 = 0x03;
/// Audio buffers to run before giving up on the game writing its RAM; the
/// boot ROM takes a few hundred.
const BUFFER_LIMIT: usize = 2_000;

/// Cartridge effects that keep a copy of every RAM save.
struct RecordingEffects(Arc<Mutex<Vec<Vec<u8>>>>);

impl CartridgeEffects for RecordingEffects {
    fn current_time_millis(&self) -> f64 {
        0.0
    }

    fn save_ram(&mut self, ram: &[u8]) {
        self.0.lock().unwrap().push(ram.to_vec());
    }
}

/// A battery-backed MBC1 game that writes `ram_enable` to the RAM enable
/// register, then writes 0xA000 over and over with a counter.
fn ram_writing_rom(ram_enable: u8) -> Vec<u8> {
    let header = header(0x00, 0x00, CART_TYPE_MBC1_RAM_BATTERY, 0x33);
    let mut rom = vec![0; ROM_SIZE];
    rom[HEADER_START..HEADER_START + header.len()].copy_from_slice(&header);
    let program = [
        0x3E, ram_enable, // LD A, ram_enable
        0xEA, 0x00, 0x00, // LD (0x0000), A
        0x21, 0x00, 0xA0, // LD HL, 0xA000
        0x77, // loop: LD (HL), A
        0x3C, // INC A
        0x18, 0xFC, // JR loop
    ];
    rom[0x0150..0x0150 + program.len()].copy_from_slice(&program);
    rom
}

/// Runs `rom` for `buffers` audio buffers after the game's first RAM save,
/// returning the saves made during those buffers and the emulator.
fn saves_after_first(rom: &[u8], buffers: usize) -> (Vec<Vec<u8>>, Emulator) {
    let saves = Arc::new(Mutex::new(Vec::new()));
    let mut emulator = Emulator::new(Model::Dmg);
    emulator.set_sample_rate(48_000);
    emulator
        .load_rom(rom, Box::new(RecordingEffects(saves.clone())))
        .unwrap();

    let mut waited = 0;
    while saves.lock().unwrap().is_empty() && waited < BUFFER_LIMIT {
        emulator.step_until_next_audio_buffer();
        waited += 1;
    }
    saves.lock().unwrap().clear();
    for _ in 0..buffers {
        emulator.step_until_next_audio_buffer();
    }
    let saves = std::mem::take(&mut *saves.lock().unwrap());
    (saves, emulator)
}

#[test]
fn battery_ram_is_saved_once_per_call_however_often_it_is_written() {
    let (saves, emulator) = saves_after_first(&ram_writing_rom(0x0A), 3);

    assert_eq!(saves.len(), 3);
    assert_eq!(
        saves.last().map(Vec::as_slice),
        Some(emulator.address_bus().cartridge_ram())
    );
}

#[test]
fn battery_ram_is_not_saved_when_the_game_cannot_write_it() {
    let (saves, _) = saves_after_first(&ram_writing_rom(0x00), 3);

    assert!(saves.is_empty());
}

const CART_TYPE_MBC5_RAM_BATTERY: u8 = 0x1B;
const BUSY_PROGRAM_START: usize = 0x0150;
/// Where the busy program's loop starts.
const BUSY_LOOP: u16 = 0x0194;
/// Steps after the busy program reaches its loop at which the save state is
/// taken: OAM DMA is still copying and the serial transfer still running.
const STEPS_INTO_THE_LOOP: usize = 20;

/// A CGB game that sets up as much of the hardware as it can and then loops:
/// the timer at 262 kHz, a serial transfer on the internal clock, the joypad
/// group select, work RAM bank 3, IE, an undocumented CGB register, HRAM,
/// cartridge RAM, KEY1, an HBlank VRAM DMA and an OAM DMA.
fn busy_rom() -> Vec<u8> {
    let header = header(0x80, 0x00, CART_TYPE_MBC5_RAM_BATTERY, 0x33);
    let mut rom = vec![0; ROM_SIZE];
    rom[HEADER_START..HEADER_START + header.len()].copy_from_slice(&header);
    #[rustfmt::skip]
    let program = [
        0xF3,                         // DI
        0x3E, 0x05, 0xE0, 0x07,       // TAC = 0x05
        0x3E, 0x81, 0xE0, 0x02,       // SC = 0x81
        0x3E, 0x20, 0xE0, 0x00,       // JOYP = 0x20
        0x3E, 0x03, 0xE0, 0x70,       // SVBK = 3
        0x3E, 0x1F, 0xE0, 0xFF,       // IE = 0x1F
        0x3E, 0x5A, 0xE0, 0x72,       // FF72 = 0x5A
        0xE0, 0x90,                   // HRAM
        0xEA, 0x23, 0xD1,             // work RAM bank 3
        0x3E, 0x0A, 0xEA, 0x00, 0x00, // enable cartridge RAM
        0x3E, 0x77, 0xEA, 0x00, 0xA0, // cartridge RAM
        0x3E, 0x01, 0xE0, 0x4D,       // KEY1: arm a speed switch
        0x3E, 0x40, 0xE0, 0x51,       // HDMA source 0x4000
        0x3E, 0x00, 0xE0, 0x52,
        0x3E, 0x81, 0xE0, 0x53,       // HDMA destination 0x8100
        0x3E, 0x00, 0xE0, 0x54,
        0x3E, 0x85, 0xE0, 0x55,       // HBlank DMA of 6 blocks
        0x3E, 0xC0, 0xE0, 0x46,       // OAM DMA from 0xC000
        0x04,                         // loop: INC B
        0x18, 0xFD,                   // JR loop
    ];
    rom[BUSY_PROGRAM_START..BUSY_PROGRAM_START + program.len()].copy_from_slice(&program);
    assert_eq!(rom[usize::from(BUSY_LOOP)], 0x04);
    rom
}

/// The fields of a struct in pretty `Debug` output, whose names are indented
/// by `indent` spaces, with the text of each field's value.
fn debug_fields(debug: &str, indent: usize) -> Vec<(String, String)> {
    let prefix = " ".repeat(indent);
    let mut fields: Vec<(String, String)> = Vec::new();
    for line in debug.lines() {
        let field_name = line
            .strip_prefix(&prefix)
            .filter(|rest| !rest.starts_with(' '))
            .and_then(|rest| rest.split_once(": "))
            .map(|(name, _)| name)
            .filter(|name| {
                name.chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
            });
        if let Some(name) = field_name {
            fields.push((name.to_string(), String::new()));
        }
        if let Some((_, text)) = fields.last_mut() {
            text.push_str(line);
            text.push('\n');
        }
    }
    fields
}

/// The parts of the address bus whose own fields are compared one by one, so
/// fields they leave out of save states on purpose can be listed.
const COMPONENTS_COMPARED_BY_FIELD: [&str; 2] = ["gpu", "apu"];

/// The fields of the CPU, the address bus, the PPU and the APU, each named by
/// its path, with the PPU caught up.
fn cpu_and_bus_fields(emulator: &mut Emulator) -> Vec<(String, String)> {
    emulator.address_bus_mut().catch_up_ppu();
    let debug = format!("{:#?}", emulator.cpu);
    let mut fields = Vec::new();
    for (name, text) in debug_fields(&debug, 4) {
        if name != "address_bus" {
            fields.push((name, text));
            continue;
        }
        for (bus_field, bus_text) in debug_fields(&text, 8) {
            if COMPONENTS_COMPARED_BY_FIELD.contains(&bus_field.as_str()) {
                for (field, field_text) in debug_fields(&bus_text, 12) {
                    fields.push((format!("address_bus.{bus_field}.{field}"), field_text));
                }
            } else {
                fields.push((format!("address_bus.{bus_field}"), bus_text));
            }
        }
    }
    fields
}

/// Fields that save states leave out on purpose and that can differ after
/// loading one: the address bus's budgets for running ahead (see its
/// `Serializable` implementation), the PPU's hand-over of the finished frame
/// to the frontend, and the samples the APU already made for the frontend.
/// The buttons the player holds and the sample rate are left out on purpose
/// too (they belong to the player and the sound card, not the console), but
/// both emulators here have the same ones, so they aren't listed.
const FIELDS_NOT_IN_SAVE_STATES: [&str; 5] = [
    "address_bus.ppu_quiet_dots",
    "address_bus.deferral_budget",
    "address_bus.gpu.frame_ready",
    "address_bus.apu.left_sample_queue",
    "address_bus.apu.right_sample_queue",
];

/// Catches a field missing from save states: an emulator that loaded a state
/// must match the one that saved it in every field but the ones listed above.
#[test]
fn a_loaded_state_restores_every_field_of_the_cpu_the_address_bus_the_ppu_and_the_apu() {
    let rom = busy_rom();
    let mut saved = booted(Model::CgbE, &rom);
    while saved.registers().program_counter != BUSY_LOOP + 1 {
        saved.step();
    }
    for _ in 0..STEPS_INTO_THE_LOOP {
        saved.step();
    }
    let mut loaded = Emulator::new(Model::CgbE);
    loaded.set_sample_rate(48_000);
    loaded.load_rom(&rom, empty_cartridge_effects()).unwrap();
    loaded.load_state(&saved.save_state()).unwrap();

    let saved_fields = cpu_and_bus_fields(&mut saved);
    let loaded_fields = cpu_and_bus_fields(&mut loaded);
    let names = |fields: &[(String, String)]| -> Vec<String> {
        fields.iter().map(|(name, _)| name.clone()).collect()
    };
    assert_eq!(names(&saved_fields), names(&loaded_fields));
    for component in COMPONENTS_COMPARED_BY_FIELD {
        let prefix = format!("address_bus.{component}.");
        assert!(
            saved_fields
                .iter()
                .any(|(name, _)| name.starts_with(&prefix)),
            "found no fields of the {component} to compare"
        );
    }
    let differing: Vec<&str> = saved_fields
        .iter()
        .zip(&loaded_fields)
        .filter(|((name, saved), (_, loaded))| {
            saved != loaded && !FIELDS_NOT_IN_SAVE_STATES.contains(&name.as_str())
        })
        .map(|((name, _), _)| name.as_str())
        .collect();
    assert!(
        differing.is_empty(),
        "loading a save state left {differing:?} as they were: save them, or list them as left out"
    );
}

/// The most PPU dots one CPU step takes without VRAM DMA: an instruction of
/// six M-cycles and an interrupt dispatch of five, at four dots each.
const MOST_DOTS_PER_STEP: u64 = 44;
/// The most PPU dots a step that copies 128 blocks of general-purpose VRAM
/// DMA takes: 32 dots per block, plus the instruction.
const MOST_DOTS_PER_DMA_STEP: u64 = 128 * 32 + MOST_DOTS_PER_STEP;
/// How many frames the `run_frame` tests run.
const FRAMES: usize = 120;
/// PPU dots per emulated second.
const DOTS_PER_SECOND: u64 = 4 * 1024 * 1024;

/// A game for `model` that runs `program` from 0x0150, with a timer
/// interrupt handler that only returns.
fn program_rom(model: Model, program: &[u8]) -> Vec<u8> {
    let cgb_flag = if model.is_cgb() { 0x80 } else { 0x00 };
    let header = header(cgb_flag, 0x00, 0x00, 0x33);
    let mut rom = vec![0; ROM_SIZE];
    rom[HEADER_START..HEADER_START + header.len()].copy_from_slice(&header);
    rom[TIMER_INTERRUPT_HANDLER] = 0xD9; // RETI
    rom[BUSY_PROGRAM_START..BUSY_PROGRAM_START + program.len()].copy_from_slice(program);
    rom
}

/// Where the CPU goes for a timer interrupt.
const TIMER_INTERRUPT_HANDLER: usize = 0x0050;

/// Games that keep the CPU busy, halt it for good, turn the screen off,
/// switch to double speed, take timer interrupts, copy VRAM DMA and play a
/// tone, each running on forever.
#[rustfmt::skip]
const FRAME_PROGRAMS: [(&str, &[u8]); 7] = [
    ("busy", &[
        0x04,                   // loop: INC B
        0x18, 0xFD,             // JR loop
    ]),
    ("halted", &[
        0xF3, 0xAF, 0xE0, 0xFF, // DI; IE = 0
        0x76,                   // loop: HALT
        0x18, 0xFD,             // JR loop
    ]),
    ("screen off", &[
        0xAF, 0xE0, 0x40,       // LCDC = 0
        0x04,                   // loop: INC B
        0x18, 0xFD,             // JR loop
    ]),
    ("double speed", &[
        0xF3, 0xAF, 0xE0, 0xFF, // DI; IE = 0
        0x3E, 0x01, 0xE0, 0x4D, // KEY1: arm a speed switch
        0x10, 0x00,             // STOP
        0x04,                   // loop: INC B
        0x18, 0xFD,             // JR loop
    ]),
    ("interrupts", &[
        0x3E, 0x05, 0xE0, 0x07, // TAC: the timer at 262 kHz
        0x3E, 0x04, 0xE0, 0xFF, // IE = the timer
        0xFB,                   // EI
        0xCD, 0x5E, 0x01,       // loop: CALL return
        0x18, 0xFB,             // JR loop
        0xC9,                   // return: RET
    ]),
    ("VRAM DMA", &[
        0x3E, 0x40, 0xE0, 0x51, // loop: HDMA source 0x4000
        0xAF, 0xE0, 0x52,
        0xE0, 0x53, 0xE0, 0x54, // HDMA destination 0x8000
        0x3E, 0x7F, 0xE0, 0x55, // a general-purpose copy of 128 blocks
        0x18, 0xEF,             // JR loop
    ]),
    ("sound", &[
        0x3E, 0x80, 0xE0, 0x26, // NR52: sound on
        0x3E, 0x77, 0xE0, 0x24, // NR50: full volume
        0x3E, 0xFF, 0xE0, 0x25, // NR51: every channel on both sides
        0x3E, 0xF0, 0xE0, 0x12, // NR12: channel 1 loud, no envelope
        0x3E, 0x80, 0xE0, 0x11, // NR11: half duty
        0xAF, 0xE0, 0x13,       // NR13
        0x3E, 0x87, 0xE0, 0x14, // NR14: trigger channel 1
        0x04,                   // loop: INC B
        0x18, 0xFD,             // JR loop
    ]),
];

/// The models the `run_frame` tests run `name`'s program on.
fn frame_models(name: &str) -> &'static [Model] {
    if name == "double speed" || name == "VRAM DMA" {
        &[Model::CgbE]
    } else {
        &[Model::Dmg, Model::CgbE]
    }
}

/// What `run_frame` calls leave: the time after each, its samples and
/// whether it finished a frame.
#[derive(Debug, PartialEq)]
struct FrameRun {
    dots: Vec<u64>,
    left: Vec<f32>,
    right: Vec<f32>,
    frames: Vec<Option<Vec<u8>>>,
}

/// Calls `run_frame` `count` times.
fn run_frames(emulator: &mut Emulator, count: usize) -> FrameRun {
    let mut run = FrameRun {
        dots: Vec::new(),
        left: Vec::new(),
        right: Vec::new(),
        frames: Vec::new(),
    };
    for _ in 0..count {
        emulator.run_frame(&mut run.left, &mut run.right);
        run.dots.push(emulator.address_bus().elapsed_dots());
        run.frames.push(emulator.take_frame().map(<[u8]>::to_vec));
    }
    run
}

#[test]
fn run_frame_stops_within_one_cpu_step_past_each_frame_boundary() {
    for (name, program) in FRAME_PROGRAMS {
        for &model in frame_models(name) {
            let mut emulator = booted(model, &program_rom(model, program));
            let most_overshoot = if name == "VRAM DMA" {
                MOST_DOTS_PER_DMA_STEP
            } else {
                MOST_DOTS_PER_STEP
            };
            let mut before = emulator.address_bus().elapsed_dots();
            let run = run_frames(&mut emulator, FRAMES);
            for (call, &after) in run.dots.iter().enumerate() {
                let boundary = (before / DOTS_PER_FRAME + 1) * DOTS_PER_FRAME;
                assert!(
                    (boundary..boundary + most_overshoot).contains(&after),
                    "{name} on {model:?}: call {call} ended at {after}, boundary {boundary}"
                );
                before = after;
            }
        }
    }
}

#[test]
fn run_frame_finishes_one_frame_per_call_even_with_the_screen_off() {
    for (name, program) in FRAME_PROGRAMS {
        for &model in frame_models(name) {
            let mut emulator = booted(model, &program_rom(model, program));
            let run = run_frames(&mut emulator, FRAMES);
            let frames = run.frames[2..]
                .iter()
                .filter(|frame| frame.is_some())
                .count();
            assert_eq!(frames, FRAMES - 2, "{name} on {model:?}");
        }
    }
}

#[test]
fn run_frame_hands_out_every_sample() {
    for (name, program) in FRAME_PROGRAMS {
        for &model in frame_models(name) {
            for sample_rate in [44_100, 48_000, 96_000] {
                let mut emulator = booted(model, &program_rom(model, program));
                emulator.set_sample_rate(sample_rate);
                let start = emulator.address_bus().elapsed_dots();
                let run = run_frames(&mut emulator, FRAMES);
                let dots = run.dots[FRAMES - 1] - start;
                let expected = dots * u64::from(sample_rate) / DOTS_PER_SECOND;
                let samples = run.left.len() as u64;
                assert_eq!(run.left.len(), run.right.len());
                assert!(
                    samples.abs_diff(expected) <= 1,
                    "{name} on {model:?} at {sample_rate} Hz: {samples} samples in {dots} dots, expected {expected}"
                );
            }
        }
    }
}

#[test]
fn run_frame_hands_out_the_samples_that_running_by_audio_buffers_does() {
    for (name, program) in FRAME_PROGRAMS {
        for &model in frame_models(name) {
            let rom = program_rom(model, program);
            let mut by_frames = booted(model, &rom);
            let frames = run_frames(&mut by_frames, FRAMES / 2);

            let mut by_buffers = booted(model, &rom);
            let (mut left, mut right) = (Vec::new(), Vec::new());
            while left.len() < frames.left.len() {
                let (new_left, new_right) = by_buffers.step_until_next_audio_buffer();
                left.extend_from_slice(new_left);
                right.extend_from_slice(new_right);
            }
            let count = frames.left.len();
            if name == "sound" {
                assert!(frames.left.iter().any(|sample| *sample != 0.0));
            }
            assert!(
                frames.left == left[..count] && frames.right == right[..count],
                "{name} on {model:?}: the samples differ"
            );
        }
    }
}

#[test]
fn run_frame_goes_on_alike_after_loading_a_save_state() {
    let games = FRAME_PROGRAMS
        .iter()
        .flat_map(|(name, program)| {
            frame_models(name)
                .iter()
                .map(move |&model| (*name, model, program_rom(model, program)))
        })
        .chain([("busy hardware", Model::CgbE, busy_rom())]);
    for (name, model, rom) in games {
        let mut saved = booted(model, &rom);
        run_frames(&mut saved, FRAMES / 4);
        let state = saved.save_state();
        let first = run_frames(&mut saved, FRAMES / 2);

        let mut loaded = Emulator::new(model);
        loaded.set_sample_rate(48_000);
        loaded.load_rom(&rom, empty_cartridge_effects()).unwrap();
        loaded.load_state(&state).unwrap();
        let second = run_frames(&mut loaded, FRAMES / 2);
        assert!(first == second, "{name} on {model:?} went on differently");
    }
}

#[test]
fn a_save_state_with_an_impossible_time_since_power_on_does_not_load() {
    let rom = busy_rom();
    let mut emulator = booted(Model::CgbE, &rom);
    let mut state = emulator.save_state();
    let name = b"\x0celapsed_dots";
    let start = state
        .windows(name.len())
        .position(|window| window == name)
        .expect("the state holds the time since power-on")
        + name.len()
        + size_of::<u32>();
    state[start..start + size_of::<u64>()].copy_from_slice(&u64::MAX.to_le_bytes());
    let before = emulator.save_state();

    assert!(emulator.load_state(&state).is_err());
    assert!(
        emulator.save_state() == before,
        "the failed load changed the emulator"
    );
    emulator.run_frame(&mut Vec::new(), &mut Vec::new());
}
