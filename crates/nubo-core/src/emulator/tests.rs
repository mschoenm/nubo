//! Checks that a copy of a booted emulator with another game inserted is in
//! exactly the state that booting that game would have left it in, which
//! test runners rely on to skip the boot ROM, that a loaded save state
//! restores every field save states are meant to keep, that a damaged one
//! never makes the emulator panic (`damaged_save_states`), that
//! battery-backed RAM reaches the frontend once per call, that
//! `run_frame` keeps to the frame boundaries and hands out every sample, and
//! that what frontends such as the RetroArch core read and set besides
//! frames and sound (the cartridge's clock on either time source, the rumble
//! motor and the memory) is right. It also checks that the Super Game Boy's
//! intro and the player's menu choices survive loading, and that a loaded
//! state starts from the buttons it stored until the keys held now take
//! over.

use crate::address_bus::effects::empty_cartridge_effects;
use crate::{CartridgeEffects, DOTS_PER_FRAME, Emulator, Key, Model, SgbBorder};
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

const CARTRIDGE_TYPE_MBC1_RAM_BATTERY: u8 = 0x03;
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
    let header = header(0x00, 0x00, CARTRIDGE_TYPE_MBC1_RAM_BATTERY, 0x33);
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

const CARTRIDGE_TYPE_MBC5_RAM_BATTERY: u8 = 0x1B;
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
    let header = header(0x80, 0x00, CARTRIDGE_TYPE_MBC5_RAM_BATTERY, 0x33);
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
const COMPONENTS_COMPARED_BY_FIELD: [&str; 3] = ["cartridge", "gpu", "apu"];

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
/// loading one: the address bus's budgets for running ahead (see the save
/// state notes above `AddressBus::write_save_state_components`), the PPU's hand-over of the finished frame
/// to the frontend, the samples the APU already made for the frontend, and
/// how long the rumble motor was on since the frontend last asked.
/// The keys the keyboard, the controllers and the frontend hold and the
/// sample rate are left out on purpose too (they belong to the player and
/// the sound card, not the console; the joypad stores the buttons the game
/// sees), but both emulators here have the same ones, so they aren't listed.
const FIELDS_NOT_IN_SAVE_STATES: [&str; 7] = [
    "address_bus.ppu_quiet_dots",
    "address_bus.ppu_quiet_dots_hold_no_interrupt",
    "address_bus.cartridge.rumble",
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
    assert_state_with_field_does_not_load(&[b"\x0celapsed_dots"], &u64::MAX.to_le_bytes());
}

#[test]
fn a_save_state_with_an_impossible_ppu_dot_debt_does_not_load() {
    for debt in [i16::MIN, -2, 3, i16::MAX] {
        assert_state_with_field_does_not_load(&[b"\x0cppu_dot_debt"], &debt.to_le_bytes());
    }
}

#[test]
fn a_save_state_with_an_impossible_oam_dma_does_not_load() {
    for offset in [161, u8::MAX] {
        assert_state_with_field_does_not_load(&[b"\x07oam_dma", b"\x06offset"], &[offset]);
    }
    assert_state_with_field_does_not_load(
        &[b"\x07oam_dma", b"\x0esource_address"],
        &0xFFFF_u16.to_le_bytes(),
    );
    assert_state_with_field_does_not_load(&[b"\x07oam_dma", b"\x0bstart_delay"], &[3]);
}

#[test]
fn a_save_state_with_an_impossible_serial_bit_count_does_not_load() {
    for bits in [8, u8::MAX] {
        assert_state_with_field_does_not_load(&[b"\x06serial", b"\x10bits_transferred"], &[bits]);
    }
}

#[test]
fn a_save_state_with_an_impossible_speed_switch_does_not_load() {
    let speed_switch: &[u8] = b"\x0cspeed_switch";
    assert_state_with_field_does_not_load(&[speed_switch, b"\x12double_speed_delay"], &[7]);
    assert_state_with_field_does_not_load(&[speed_switch, b"\x06freeze"], &[4]);
    assert_state_with_field_does_not_load(
        &[speed_switch, b"\x0ahalt_units"],
        &0x2_0009_u32.to_le_bytes(),
    );
}

/// Checks that a save state whose field at `path` (the names of the
/// components it lies in and then its own, each with its length byte in
/// front) holds `value` fails to load, leaves the emulator as it was, and
/// that the emulator then runs on.
fn assert_state_with_field_does_not_load(path: &[&[u8]], value: &[u8]) {
    let rom = busy_rom();
    let mut emulator = booted(Model::CgbE, &rom);
    let mut state = emulator.save_state();
    let start = path.iter().fold(0, |start, name| {
        start
            + state[start..]
                .windows(name.len())
                .position(|window| window == *name)
                .expect("the state holds the field")
            + name.len()
    }) + size_of::<u32>();
    state[start..start + value.len()].copy_from_slice(value);
    let before = emulator.save_state();

    assert!(emulator.load_state(&state).is_err());
    assert!(
        emulator.save_state() == before,
        "the failed load changed the emulator"
    );
    emulator.run_frame(&mut Vec::new(), &mut Vec::new());
}

const CARTRIDGE_TYPE_MBC3_TIMER_RAM_BATTERY: u8 = 0x10;
const CARTRIDGE_TYPE_MBC5_RUMBLE: u8 = 0x1C;
/// A wall-clock time in milliseconds, about 2001.
const WALL_CLOCK_NOW: f64 = 1_000_000_000_000.0;
/// Milliseconds in an hour.
const HOUR: f64 = 3_600_000.0;

/// A DMG game of `cartridge_type` that runs `program` from 0x0150.
fn cartridge_rom(cartridge_type: u8, program: &[u8]) -> Vec<u8> {
    let header = header(0x00, 0x00, cartridge_type, 0x33);
    let mut rom = vec![0; ROM_SIZE];
    rom[HEADER_START..HEADER_START + header.len()].copy_from_slice(&header);
    rom[BUSY_PROGRAM_START..BUSY_PROGRAM_START + program.len()].copy_from_slice(program);
    rom
}

/// Cartridge effects whose clock follows `clock` and whose wall clock reads
/// whatever the test sets.
struct ClockEffects {
    clock: crate::RtcClock,
    wall_clock: Arc<Mutex<f64>>,
}

impl CartridgeEffects for ClockEffects {
    fn current_time_millis(&self) -> f64 {
        *self.wall_clock.lock().unwrap()
    }

    fn rtc_clock(&self) -> crate::RtcClock {
        self.clock
    }
}

/// An MBC3 game with a clock, booted on the DMG with its clock following
/// `clock`, and the wall clock it reads.
fn clock_game(clock: crate::RtcClock) -> (Emulator, Arc<Mutex<f64>>) {
    let wall_clock = Arc::new(Mutex::new(WALL_CLOCK_NOW));
    let rom = cartridge_rom(CARTRIDGE_TYPE_MBC3_TIMER_RAM_BATTERY, &[0x18, 0xFE]); // JR -2
    let mut emulator = Emulator::new(Model::Dmg);
    emulator
        .load_rom(
            &rom,
            Box::new(ClockEffects {
                clock,
                wall_clock: wall_clock.clone(),
            }),
        )
        .unwrap();
    run_frames(&mut emulator, 2);
    (emulator, wall_clock)
}

/// A clock showing day 3, 10:20:30, last brought up to date at `base`.
fn ten_twenty_thirty(base: f64) -> crate::RtcState {
    crate::RtcState {
        seconds: 30,
        minutes: 20,
        hours: 10,
        days: 3,
        base_timestamp: base,
        ..crate::RtcState::default()
    }
}

/// The time an emulator has run, in milliseconds, as the clock counts it on
/// emulated time.
fn emulated_millis(emulator: &Emulator) -> f64 {
    emulator.address_bus().elapsed_dots() as f64 / 4_194.304
}

#[test]
fn the_clock_state_reads_back_on_its_own_clock() {
    let (mut emulator, _) = clock_game(crate::RtcClock::WallClock);
    let state = ten_twenty_thirty(WALL_CLOCK_NOW - HOUR);
    emulator.set_rtc_state(&state, crate::RtcClock::WallClock);

    assert_eq!(emulator.rtc_state(crate::RtcClock::WallClock), Some(state));
}

#[test]
fn a_wall_clock_state_set_on_emulated_time_neither_stands_still_nor_jumps() {
    let (mut emulator, _) = clock_game(crate::RtcClock::EmulatedTime);
    emulator.set_rtc_state(
        &ten_twenty_thirty(WALL_CLOCK_NOW - HOUR),
        crate::RtcClock::WallClock,
    );

    let on_emulated_time = emulator.rtc_state(crate::RtcClock::EmulatedTime).unwrap();
    assert_eq!(on_emulated_time.hours, 11, "the hour that passed counts");
    assert_eq!(
        (on_emulated_time.minutes, on_emulated_time.seconds),
        (20, 30)
    );
    let behind = emulated_millis(&emulator) - on_emulated_time.base_timestamp;
    assert!((0.0..1.0).contains(&behind), "{behind} ms behind");

    run_frames(&mut emulator, 120);
    let two_seconds_later = emulator.rtc_state(crate::RtcClock::WallClock).unwrap();
    assert_eq!(
        (
            two_seconds_later.hours,
            two_seconds_later.minutes,
            two_seconds_later.seconds
        ),
        (11, 20, 32)
    );
    let behind = WALL_CLOCK_NOW - two_seconds_later.base_timestamp;
    assert!((0.0..1.0).contains(&behind), "{behind} ms behind");
}

/// A damaged `.rtc` file, as RetroArch passes it on emulated time, is cut to
/// the registers' widths before the hour it was away is counted.
#[test]
fn a_damaged_wall_clock_state_set_on_emulated_time_is_cut_before_it_counts() {
    let (mut emulator, _) = clock_game(crate::RtcClock::EmulatedTime);
    let damaged = crate::RtcState {
        milliseconds: 0xFFFF,
        seconds: 0xFF,
        minutes: 0xFF,
        hours: 0xFF,
        days: 0xFFFF,
        base_timestamp: WALL_CLOCK_NOW - HOUR,
        ..crate::RtcState::default()
    };
    emulator.set_rtc_state(&damaged, crate::RtcClock::WallClock);

    let clock = emulator.rtc_state(crate::RtcClock::EmulatedTime).unwrap();
    assert_eq!(
        (
            clock.days,
            clock.hours,
            clock.minutes,
            clock.seconds,
            clock.milliseconds
        ),
        (0x1FF, 31, 58, 59, 535)
    );
}

#[test]
fn an_emulated_time_state_set_on_the_wall_clock_neither_stands_still_nor_jumps() {
    let (mut emulator, wall_clock) = clock_game(crate::RtcClock::WallClock);
    let start = ten_twenty_thirty(0.0);
    emulator.set_rtc_state(&start, crate::RtcClock::EmulatedTime);

    let on_the_wall_clock = emulator.rtc_state(crate::RtcClock::WallClock).unwrap();
    assert_eq!(on_the_wall_clock.days, 3, "no decades were counted");
    assert_eq!(on_the_wall_clock.hours, 10);
    assert!((WALL_CLOCK_NOW - on_the_wall_clock.base_timestamp) < 1.0);

    *wall_clock.lock().unwrap() += 5_000.0;
    let back = emulator.rtc_state(crate::RtcClock::EmulatedTime).unwrap();
    assert_eq!((back.hours, back.minutes, back.seconds), (10, 20, 35));
}

#[test]
fn a_save_state_moves_its_clock_over_to_the_clock_the_loading_emulator_follows() {
    let (mut saved, _) = clock_game(crate::RtcClock::WallClock);
    saved.set_rtc_state(
        &ten_twenty_thirty(WALL_CLOCK_NOW),
        crate::RtcClock::WallClock,
    );
    let state = saved.save_state();

    let (mut loaded, _) = clock_game(crate::RtcClock::EmulatedTime);
    loaded.load_state(&state).unwrap();
    let clock = loaded.rtc_state(crate::RtcClock::EmulatedTime).unwrap();
    assert_eq!(
        (clock.days, clock.hours, clock.minutes, clock.seconds),
        (3, 10, 20, 30)
    );
    let behind = emulated_millis(&loaded) - clock.base_timestamp;
    assert!((0.0..1.0).contains(&behind), "{behind} ms behind");

    run_frames(&mut loaded, 60);
    let a_second_later = loaded.rtc_state(crate::RtcClock::WallClock).unwrap();
    assert_eq!(a_second_later.seconds, 31);

    let (mut back, _) = clock_game(crate::RtcClock::WallClock);
    back.load_state(&loaded.save_state()).unwrap();
    let clock = back.rtc_state(crate::RtcClock::WallClock).unwrap();
    assert_eq!((clock.hours, clock.minutes, clock.seconds), (10, 20, 31));
}

#[test]
fn a_save_state_on_the_same_clock_keeps_its_clock_as_it_is() {
    let (mut saved, _) = clock_game(crate::RtcClock::WallClock);
    let state_clock = ten_twenty_thirty(WALL_CLOCK_NOW - HOUR);
    saved.set_rtc_state(&state_clock, crate::RtcClock::WallClock);
    let state = saved.save_state();

    let (mut loaded, _) = clock_game(crate::RtcClock::WallClock);
    loaded.load_state(&state).unwrap();
    assert_eq!(
        loaded.rtc_state(crate::RtcClock::WallClock),
        Some(state_clock)
    );
}

#[test]
fn a_game_without_a_clock_has_no_clock_state() {
    let mut emulator = booted(Model::Dmg, &ram_writing_rom(0x0A));
    emulator.set_rtc_state(&ten_twenty_thirty(0.0), crate::RtcClock::WallClock);
    assert_eq!(emulator.rtc_state(crate::RtcClock::WallClock), None);
}

/// A rumble cartridge game that switches the motor on, waits `on` times
/// around a loop of 256 `DEC C`s, switches it off and waits `off` times
/// around, forever.
fn rumble_rom(on: u8, off: u8) -> Vec<u8> {
    #[rustfmt::skip]
    let program = [
        0x3E, 0x08, 0xEA, 0x00, 0x40, // loop: LD A, 0x08; LD (0x4000), A
        0x06, on,                     // LD B, on
        0x0D, 0x20, 0xFD,             // wait_on: DEC C; JR NZ, wait_on
        0x05, 0x20, 0xFA,             // DEC B; JR NZ, wait_on
        0xAF, 0xEA, 0x00, 0x40,       // XOR A; LD (0x4000), A
        0x06, off,                    // LD B, off
        0x0D, 0x20, 0xFD,             // wait_off: DEC C; JR NZ, wait_off
        0x05, 0x20, 0xFA,             // DEC B; JR NZ, wait_off
        0x18, 0xE5,                   // JR loop
    ];
    cartridge_rom(CARTRIDGE_TYPE_MBC5_RUMBLE, &program)
}

#[test]
fn the_rumble_level_is_the_share_of_the_time_the_motor_was_on() {
    for (on, off, expected) in [(1, 0, 1.0), (2, 2, 0.5), (1, 3, 0.25)] {
        let off = if off == 0 { 1 } else { off };
        let rom = if expected == 1.0 {
            cartridge_rom(
                CARTRIDGE_TYPE_MBC5_RUMBLE,
                &[0x3E, 0x08, 0xEA, 0x00, 0x40, 0x18, 0xFE], // motor on; JR -2
            )
        } else {
            rumble_rom(on, off)
        };
        let mut emulator = booted(Model::Dmg, &rom);
        run_frames(&mut emulator, 2);
        emulator.take_rumble_level();
        for frame in 0..10 {
            run_frames(&mut emulator, 1);
            let level = emulator.take_rumble_level();
            assert!(
                (level - expected).abs() < 0.05,
                "on {on}, off {off}, frame {frame}: {level}, expected {expected}"
            );
        }
    }
}

#[test]
fn games_without_a_rumble_motor_never_rumble() {
    let mut emulator = booted(Model::Dmg, &busy_rom());
    run_frames(&mut emulator, 3);
    assert_eq!(emulator.take_rumble_level(), 0.0);
}

#[test]
fn the_rumble_level_follows_a_loaded_save_state() {
    let rom = cartridge_rom(
        CARTRIDGE_TYPE_MBC5_RUMBLE,
        &[0x3E, 0x08, 0xEA, 0x00, 0x40, 0x18, 0xFE], // motor on; JR -2
    );
    let mut emulator = booted(Model::Dmg, &rom);
    let before_the_motor = emulator.save_state();
    run_frames(&mut emulator, 2);
    let with_the_motor_on = emulator.save_state();

    emulator.load_state(&before_the_motor).unwrap();
    emulator.take_rumble_level();
    assert_eq!(emulator.take_rumble_level(), 0.0);
    emulator.load_state(&with_the_motor_on).unwrap();
    run_frames(&mut emulator, 1);
    assert_eq!(emulator.take_rumble_level(), 1.0);
}

#[test]
fn memory_reads_show_what_the_game_wrote() {
    #[rustfmt::skip]
    let program = [
        0xAF, 0xE0, 0x40,       // LCDC = 0, so VRAM and OAM can be written
        0x3E, 0x42,             // LD A, 0x42
        0xEA, 0x23, 0xC1,       // LD (0xC123), A
        0xEA, 0x45, 0xD0,       // LD (0xD045), A
        0xE0, 0x90,             // LDH (0x90), A
        0xEA, 0x10, 0x80,       // LD (0x8010), A
        0xEA, 0x04, 0xFE,       // LD (0xFE04), A
        0x18, 0xFE,             // JR -2
    ];
    let mut emulator = booted(Model::Dmg, &program_rom(Model::Dmg, &program));
    run_frames(&mut emulator, 2);

    assert_eq!(emulator.work_ram().len(), 0x8000);
    assert_eq!(emulator.work_ram()[0x0123], 0x42);
    assert_eq!(emulator.work_ram()[0x1045], 0x42);
    assert_eq!(emulator.high_ram().len(), 0x7F);
    assert_eq!(emulator.high_ram()[0x10], 0x42);
    let (video_ram, oam) = emulator.video_memory();
    assert_eq!(video_ram[0x10], 0x42);
    assert_eq!(oam.len(), 0xA0);
    assert_eq!(oam[0x04], 0x42);
}

/// A state from before Nubo stored which clock a cartridge clock follows,
/// made here by renaming the field so the loader doesn't know it.
fn without_the_clock_source(mut state: Vec<u8>) -> Vec<u8> {
    let name = b"\x09rtc_clock";
    let start = state
        .windows(name.len())
        .position(|window| window == name)
        .expect("the state holds the clock's source");
    state[start + 1] = b'x';
    state
}

#[test]
fn a_state_from_before_the_clock_source_was_stored_loads_its_clock_as_it_is() {
    for clock in [crate::RtcClock::WallClock, crate::RtcClock::EmulatedTime] {
        let (mut saved, _) = clock_game(clock);
        let state_clock = ten_twenty_thirty(1_000.0);
        saved.set_rtc_state(&state_clock, clock);
        let state = without_the_clock_source(saved.save_state());

        let (mut loaded, _) = clock_game(clock);
        loaded.load_state(&state).unwrap();
        assert_eq!(loaded.rtc_state(clock), Some(state_clock), "{clock:?}");
    }
}

const CARTRIDGE_TYPE_MBC1_RAM: u8 = 0x02;

/// A game that turns the screen off and enables cartridge RAM, then copies
/// 0xC123, 0xFF90, 0x8010, 0xFE04 and 0xA000 to 0xC000-0xC004, forever.
fn memory_reading_rom() -> Vec<u8> {
    #[rustfmt::skip]
    let program = [
        0xAF, 0xE0, 0x40,                    // LCDC = 0, so VRAM and OAM can be read
        0x3E, 0x0A, 0xEA, 0x00, 0x00,        // RAM enable = 0x0A
        0xFA, 0x23, 0xC1, 0xEA, 0x00, 0xC0,  // loop: 0xC000 = (0xC123)
        0xF0, 0x90, 0xEA, 0x01, 0xC0,        // 0xC001 = (0xFF90)
        0xFA, 0x10, 0x80, 0xEA, 0x02, 0xC0,  // 0xC002 = (0x8010)
        0xFA, 0x04, 0xFE, 0xEA, 0x03, 0xC0,  // 0xC003 = (0xFE04)
        0xFA, 0x00, 0xA0, 0xEA, 0x04, 0xC0,  // 0xC004 = (0xA000)
        0x18, 0xE1,                          // JR loop
    ];
    cartridge_rom(CARTRIDGE_TYPE_MBC1_RAM, &program)
}

#[test]
fn memory_changed_directly_is_what_the_game_reads() {
    for model in [Model::Dmg, Model::CgbE] {
        let mut emulator = booted(model, &memory_reading_rom());
        run_frames(&mut emulator, 2);

        let memory = emulator.memory_mut();
        assert_eq!(memory.work_ram.len(), 0x8000);
        assert_eq!(memory.high_ram.len(), 0x7F);
        assert_eq!(memory.video_ram.len(), 0x4000);
        assert_eq!(memory.object_attribute_memory.len(), 0xA0);
        assert_eq!(memory.cartridge_ram.len(), 0x8000);
        memory.work_ram[0x123] = 0x11;
        memory.high_ram[0x10] = 0x22;
        memory.video_ram[0x10] = 0x33;
        memory.object_attribute_memory[0x04] = 0x44;
        memory.cartridge_ram[0] = 0x55;
        run_frames(&mut emulator, 1);

        assert_eq!(
            emulator.work_ram()[..5],
            [0x11, 0x22, 0x33, 0x44, 0x55],
            "{model:?}"
        );
    }
}

#[test]
fn memory_changed_directly_is_kept_in_save_states() {
    let mut emulator = booted(Model::CgbE, &memory_reading_rom());
    run_frames(&mut emulator, 2);
    emulator.memory_mut().work_ram[0x5123] = 0x99;
    let state = emulator.save_state();
    emulator.memory_mut().work_ram[0x5123] = 0x00;
    emulator.load_state(&state).unwrap();
    assert_eq!(emulator.work_ram()[0x5123], 0x99);
}

/// A Super Game Boy state made by a version without the intro lacks its
/// frames; loaded past the boot ROM, the game must go on rather than be held
/// in reset again.
#[test]
fn an_older_super_game_boy_state_past_the_boot_rom_skips_the_intro() {
    let rom = program_rom(Model::Sgb, &[0x18, 0xFE]);
    let emulator = booted(Model::Sgb, &rom);
    assert!(!emulator.address_bus().held_in_reset());

    let state = emulator.save_state();
    let mut loaded = booted(Model::Sgb, &rom);
    loaded.load_state(&state).unwrap();
    assert!(!loaded.address_bus().held_in_reset());

    loaded
        .address_bus_mut()
        .gpu_mut_for_tests()
        .sgb_mut()
        .unwrap()
        .restart_intro_for_tests();
    loaded
        .address_bus_mut()
        .finish_loading_save_state()
        .unwrap();
    assert!(!loaded.address_bus().held_in_reset());
}

/// The intro counts the frames the PPU hands over, which a game switching
/// the LCD off and on could hold back. No game runs before the intro is over,
/// though: Nubo's boot ROM, which switches the LCD on once and never off,
/// runs until after it ends.
#[test]
fn the_super_game_boy_intro_is_over_before_the_game_runs() {
    for model in [Model::Sgb, Model::Sgb2] {
        let rom = program_rom(model, &[0x18, 0xFE]);
        let mut emulator = booted(model, &rom);
        let gpu = emulator.address_bus_mut().gpu_mut_for_tests();
        assert!(gpu.sgb().unwrap().intro_over(), "{model:?}");
    }
}

#[test]
fn the_players_super_game_boy_choices_stay_when_a_state_loads() {
    let rom = program_rom(Model::Sgb, &[0x18, 0xFE]);
    let mut emulator = booted(Model::Sgb, &rom);
    assert!(emulator.sgb_player_palette_allowed());
    let state = emulator.save_state();

    let palette = [0x7FFF, 0x001F, 0x03E0, 0x7C00];
    emulator.set_sgb_player_palette(Some(palette));
    emulator.set_sgb_border(SgbBorder::Nubo);
    emulator.load_state(&state).unwrap();
    assert_eq!(emulator.sgb_player_palette(), Some(palette));
    assert_eq!(emulator.sgb_border(), SgbBorder::Nubo);

    let mut other = booted(Model::Sgb, &rom);
    other.load_state(&emulator.save_state()).unwrap();
    assert_eq!(other.sgb_player_palette(), None);
    assert_eq!(other.sgb_border(), SgbBorder::Game);
}

#[test]
fn other_models_have_no_super_game_boy_menu() {
    let rom = program_rom(Model::Dmg, &[0x18, 0xFE]);
    let mut emulator = booted(Model::Dmg, &rom);
    emulator.set_sgb_player_palette(Some([0; 4]));
    emulator.set_sgb_border(SgbBorder::Nubo);
    assert_eq!(emulator.sgb_player_palette(), None);
    assert!(!emulator.sgb_player_palette_allowed());
    assert_eq!(emulator.sgb_border(), SgbBorder::Game);
}

#[test]
fn the_super_game_boy_sees_the_buttons_the_player_holds() {
    let rom = program_rom(Model::Sgb, &[0x18, 0xFE]);
    let mut emulator = booted(Model::Sgb, &rom);
    let held = |emulator: &Emulator| {
        emulator
            .address_bus()
            .sgb()
            .unwrap()
            .buttons_held_for_tests()
    };

    emulator.press_key(Key::A);
    assert!(held(&emulator));
    emulator.release_key(Key::A);
    assert!(!held(&emulator));
    emulator.set_player_keys(3, &[Key::Start]);
    assert!(held(&emulator));

    let state = emulator.save_state();
    let mut loaded = booted(Model::Sgb, &rom);
    loaded.set_player_keys(3, &[Key::Start]);
    loaded.load_state(&state).unwrap();
    assert!(held(&loaded));
}

/// Selects the action buttons, then counts the joypad interrupts requested
/// at 0xC000, forever.
#[rustfmt::skip]
const JOYPAD_INTERRUPT_COUNTER: &[u8] = &[
    0x3E, 0x10, 0xE0, 0x00, // JOYP = 0x10, the action buttons
    0xAF, 0xEA, 0x00, 0xC0, // 0xC000 = 0
    0xE0, 0x0F,             // IF = 0
    0xF0, 0x0F,             // loop: A = IF
    0xE6, 0x10,             // AND the joypad's bit
    0x28, 0xFA,             // JR Z, loop
    0x21, 0x00, 0xC0, 0x34, // INC (0xC000)
    0xAF, 0xE0, 0x0F,       // IF = 0
    0x18, 0xF1,             // JR loop
];

/// A state saved with A held and loaded while A is released goes on, once
/// the frontend holds A again, exactly like the run it was saved from: the
/// joypad starts from the buttons of the state, so A isn't pressed anew.
#[test]
fn a_loaded_state_starts_from_the_buttons_held_when_it_was_saved() {
    let rom = program_rom(Model::Dmg, JOYPAD_INTERRUPT_COUNTER);
    let mut original = booted(Model::Dmg, &rom);
    run_frames(&mut original, 1);
    original.press_key(Key::A);
    run_frames(&mut original, 2);
    assert_eq!(original.read_memory(0xC000), 1, "pressing A requests it");
    let state = original.save_state();
    run_frames(&mut original, 2);

    let mut loaded = booted(Model::Dmg, &rom);
    run_frames(&mut loaded, 3);
    loaded.load_state(&state).unwrap();
    loaded.press_key(Key::A);
    run_frames(&mut loaded, 2);
    assert_eq!(loaded.read_memory(0xC000), 1, "A was held already");
    assert!(loaded.save_state() == original.save_state());
}

/// After a state loads, the keys held now take over from its buttons when
/// the emulator runs, even without a button change from the frontend, as
/// if they had been pressed then in the run the state was saved from.
#[test]
fn the_keys_held_now_take_over_from_a_loaded_state_when_the_emulator_runs() {
    let rom = program_rom(Model::Dmg, JOYPAD_INTERRUPT_COUNTER);
    let mut original = booted(Model::Dmg, &rom);
    run_frames(&mut original, 1);
    let state = original.save_state();
    original.press_key(Key::A);
    run_frames(&mut original, 2);
    assert_eq!(original.read_memory(0xC000), 1);

    let mut loaded = booted(Model::Dmg, &rom);
    loaded.press_key(Key::A);
    run_frames(&mut loaded, 3);
    loaded.load_state(&state).unwrap();
    assert!(
        loaded.save_state() == state,
        "the state's buttons last until it runs"
    );
    run_frames(&mut loaded, 2);
    assert!(loaded.save_state() == original.save_state());
}

/// Frontends that read every player's buttons each frame hand them over in
/// one go, so the joypad interrupt doesn't depend on the order of the keys:
/// letting go of A while pressing Right, on the same input line, requests
/// none.
#[test]
fn held_keys_change_in_one_go() {
    let mut program = JOYPAD_INTERRUPT_COUNTER.to_vec();
    program[1] = 0x00; // JOYP = 0, both groups
    let rom = program_rom(Model::Dmg, &program);
    let mut emulator = booted(Model::Dmg, &rom);
    run_frames(&mut emulator, 1);
    emulator.set_held_keys(&[&[Key::A]]);
    run_frames(&mut emulator, 1);
    assert_eq!(emulator.read_memory(0xC000), 1);
    emulator.set_held_keys(&[&[Key::Right], &[Key::Start]]);
    run_frames(&mut emulator, 1);
    assert_eq!(emulator.read_memory(0xC000), 1, "line 0 stayed low");
    assert_eq!(emulator.read_memory(0xFF00) & 0x0F, 0x0E, "Right");
}

#[test]
fn a_super_game_boy_menu_change_shows_on_the_last_frame_at_once() {
    let rom = program_rom(Model::Sgb, &[0x18, 0xFE]);
    let mut emulator = booted(Model::Sgb, &rom);
    // Past the intro, which shows instead of the game.
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for _ in 0..220 {
        emulator.run_frame(&mut left, &mut right);
    }
    emulator.set_sgb_player_palette(Some([0x001F; 4]));
    assert_eq!(&emulator.frame_buffer()[..4], &[0xFF, 0x00, 0x00, 0xFF]);
    assert!(!emulator.frame_in_dmg_shades());
}

/// Rewind and run-ahead load states into the emulator that is running,
/// whose Super Game Boy is read into in place: it must go on exactly like a
/// new emulator that loads the same state.
#[test]
fn a_super_game_boy_state_loaded_into_the_running_emulator_goes_on_like_a_new_one() {
    let rom = program_rom(Model::Sgb, &[0x18, 0xFE]);
    let (mut left, mut right) = (Vec::new(), Vec::new());
    let mut emulator = booted(Model::Sgb, &rom);
    for _ in 0..30 {
        emulator.run_frame(&mut left, &mut right);
    }
    let state = emulator.save_state();
    for _ in 0..50 {
        emulator.run_frame(&mut left, &mut right);
    }
    emulator.load_state(&state).unwrap();
    let mut new = booted(Model::Sgb, &rom);
    new.load_state(&state).unwrap();

    for _ in 0..20 {
        emulator.run_frame(&mut left, &mut right);
        new.run_frame(&mut left, &mut right);
        assert_eq!(emulator.frame_with_border(), new.frame_with_border());
    }
    assert_eq!(emulator.save_state(), new.save_state());
}

mod damaged_save_states;
