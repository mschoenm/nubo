use super::*;
use crate::test_roms::{
    CHEATS, CLOCK, JOYPAD, JOYPAD_INTERRUPTS, MBC1_RAM_BATTERY, MBC3_TIMER_RAM_BATTERY,
    MBC5_RUMBLE, RAM, RAM_2_KIB, RAM_8_KIB, RAM_32_KIB, ROM_BYTE_AT_0200, ROM_ONLY, RUMBLE, TONE,
    cartridge,
};
use nubo_frontend::model::ModelPreference;
use nubo_frontend::palettes::DmgPalette;
use nubo_frontend::sgb_menu::SgbColors;
use std::time::Instant;

/// Frames the boot ROM takes, with room to spare, before a test program
/// runs.
const BOOT_FRAMES: usize = 200;

fn load(rom: &[u8]) -> Core {
    load_with(rom, Options::default())
}

fn load_with(rom: &[u8], options: Options) -> Core {
    Core::load(rom, options, PixelFormat::Xrgb8888).unwrap()
}

fn run(core: &mut Core, frames: usize) {
    run_holding(core, frames, 0);
}

fn run_holding(core: &mut Core, frames: usize, buttons: u16) {
    for _ in 0..frames {
        core.run(buttons);
    }
}

/// A core past the boot ROM, running `program` on a cartridge of
/// `cartridge_type` for the Game Boy.
fn booted(cartridge_type: u8, ram_size: u8, program: &[u8]) -> Core {
    let mut core = load(&cartridge(cartridge_type, ram_size, false, program));
    run(&mut core, BOOT_FRAMES);
    core
}

/// The byte at `address` in work RAM (0xC000-0xDFFF) after the last frame.
fn work_ram(core: &mut Core, address: u16) -> u8 {
    core.system_ram()[usize::from(address - 0xC000)]
}

fn frame(core: &Core) -> Vec<u32> {
    match core.video() {
        Video::Xrgb8888(pixels) => pixels.to_vec(),
        Video::Rgb1555(_) => panic!("expected XRGB8888"),
    }
}

/// Bits for libretro buttons, as `Core::run` takes them.
fn buttons(ids: &[u32]) -> u16 {
    ids.iter().fold(0, |bits, id| bits | 1 << id)
}

const RIGHT: u32 = 7;
const A: u32 = 8;
const START: u32 = 3;
const UP: u32 = 4;

#[test]
fn a_game_that_is_not_a_rom_does_not_load() {
    assert!(Core::load(&[0; 0x40], Options::default(), PixelFormat::Xrgb8888).is_err());
    let mut unsupported = cartridge(ROM_ONLY, 0, false, JOYPAD);
    unsupported[0x0147] = 0x22; // MBC7
    assert!(matches!(
        Core::load(&unsupported, Options::default(), PixelFormat::Xrgb8888),
        Err(LoadRomError::UnsupportedCartridge(_))
    ));
}

#[test]
fn the_model_follows_the_option() {
    let dmg_game = cartridge(ROM_ONLY, 0, false, JOYPAD);
    let cgb_game = cartridge(ROM_ONLY, 0, true, JOYPAD);
    assert_eq!(load(&dmg_game).model(), Model::Dmg);
    assert_eq!(load(&cgb_game).model(), Model::CgbE);
    let options = Options {
        model: ModelPreference::Fixed(Model::Mgb),
        ..Options::default()
    };
    assert_eq!(load_with(&cgb_game, options).model(), Model::Mgb);
}

#[test]
fn every_frame_has_a_whole_screen_and_one_frame_of_sound() {
    for cgb in [false, true] {
        let mut core = load(&cartridge(ROM_ONLY, 0, cgb, TONE));
        let mut samples = 0;
        let frames: u32 = 600;
        for _ in 0..frames {
            core.run(0);
            assert_eq!(frame(&core).len(), SCREEN_WIDTH * SCREEN_HEIGHT);
            assert_eq!(core.audio().len() % 2, 0);
            let per_frame = core.audio().len() / 2;
            assert!(
                (790..=820).contains(&per_frame),
                "{per_frame} samples in a frame"
            );
            samples += per_frame;
        }
        let expected = f64::from(frames) * f64::from(SAMPLE_RATE) / FRAMES_PER_SECOND;
        assert!(
            (samples as f64 - expected).abs() <= 2.0,
            "{samples} samples in {frames} frames, expected {expected}"
        );
    }
}

#[test]
fn the_sound_is_the_emulators_as_16_bit_numbers() {
    let mut core = load(&cartridge(ROM_ONLY, 0, false, TONE));
    run(&mut core, BOOT_FRAMES);
    assert!(core.audio().iter().any(|&sample| sample > 1_000));
    assert!(core.audio().iter().any(|&sample| sample < -1_000));
    assert_eq!(to_i16(0.0), 0);
    assert_eq!(to_i16(0.5), 16_384);
    assert_eq!(to_i16(-1.0), -32_768);
    assert_eq!(to_i16(1.0), 32_767);
    assert_eq!(to_i16(3.0), 32_767);
    assert_eq!(to_i16(-3.0), -32_768);
}

#[test]
fn pixels_convert_to_xrgb8888_and_0rgb1555() {
    let mut xrgb = load(&cartridge(ROM_ONLY, 0, true, JOYPAD));
    let mut rgb1555 = Core::load(
        &cartridge(ROM_ONLY, 0, true, JOYPAD),
        Options::default(),
        PixelFormat::Rgb1555,
    )
    .unwrap();
    run(&mut xrgb, 30);
    run(&mut rgb1555, 30);
    let rgba = xrgb.emulator().frame_buffer().to_vec();
    let Video::Rgb1555(pixels) = rgb1555.video() else {
        panic!("expected 0RGB1555");
    };
    for ((xrgb, rgb1555), rgba) in frame(&xrgb).iter().zip(pixels).zip(rgba.as_chunks::<4>().0) {
        let [red, green, blue, _] = *rgba;
        assert_eq!(*xrgb, u32::from_be_bytes([0, red, green, blue]));
        assert_eq!(
            *rgb1555,
            u16::from(red >> 3) << 10 | u16::from(green >> 3) << 5 | u16::from(blue >> 3)
        );
    }
    assert_eq!(PixelFormat::Xrgb8888.pitch(SCREEN_WIDTH), 640);
    assert_eq!(PixelFormat::Rgb1555.pitch(SCREEN_WIDTH), 320);
    assert_eq!(xrgb.pitch(), 640);
}

#[test]
fn a_super_game_boy_shows_its_border_unless_it_is_hidden() {
    let options = Options {
        model: ModelPreference::Fixed(Model::Sgb),
        ..Options::default()
    };
    let mut core = Core::load(
        &cartridge(ROM_ONLY, 0, false, JOYPAD),
        options,
        PixelFormat::Xrgb8888,
    )
    .unwrap();
    core.run(0);
    assert_eq!(core.video_size(), (MAX_WIDTH, MAX_HEIGHT));
    assert_eq!(frame(&core).len(), MAX_WIDTH * MAX_HEIGHT);
    assert_eq!(core.pitch(), MAX_WIDTH * 4);

    core.set_options(Options {
        sgb_border: false,
        ..options
    });
    assert_eq!(core.video_size(), (SCREEN_WIDTH, SCREEN_HEIGHT));
    assert_eq!(frame(&core).len(), SCREEN_WIDTH * SCREEN_HEIGHT);
}

#[test]
fn the_super_game_boy_options_reach_its_menu() {
    let options = Options {
        model: ModelPreference::Fixed(Model::Sgb),
        sgb_colors: SgbColors::Preset(DmgPalette::Green),
        nubo_border: true,
        ..Options::default()
    };
    let mut core = load_with(&cartridge(ROM_ONLY, 0, false, JOYPAD), options);
    assert_eq!(
        core.emulator().sgb_player_palette(),
        Some(DmgPalette::Green.rgb555())
    );
    assert_eq!(core.emulator().sgb_border(), SgbBorder::Nubo);

    core.set_options(Options {
        sgb_colors: SgbColors::Game,
        nubo_border: false,
        ..options
    });
    assert_eq!(core.emulator().sgb_player_palette(), None);
    assert_eq!(core.emulator().sgb_border(), SgbBorder::Game);
}

#[test]
fn game_boy_games_show_in_the_palette_the_player_picked() {
    let mut core = load(&cartridge(ROM_ONLY, 0, false, JOYPAD));
    run(&mut core, 30);
    let gray = frame(&core);
    let shades = DmgPalette::Gray
        .colors()
        .map(|[r, g, b]| u32::from_be_bytes([0, r, g, b]));
    assert!(gray.iter().all(|pixel| shades.contains(pixel)));
    assert!(gray.iter().any(|pixel| *pixel != gray[0]), "the logo shows");

    core.set_options(Options {
        palette: DmgPalette::Green,
        ..Options::default()
    });
    let green = frame(&core);
    let greens = DmgPalette::Green
        .colors()
        .map(|[r, g, b]| u32::from_be_bytes([0, r, g, b]));
    for (gray, green) in gray.iter().zip(&green) {
        let shade = shades.iter().position(|shade| shade == gray).unwrap();
        assert_eq!(*green, greens[shade]);
    }
}

#[test]
fn game_boy_color_games_keep_their_colors_whatever_the_palette() {
    let rom = cartridge(ROM_ONLY, 0, true, JOYPAD);
    let mut gray = load(&rom);
    let mut green = load_with(
        &rom,
        Options {
            palette: DmgPalette::Green,
            ..Options::default()
        },
    );
    run(&mut gray, 30);
    run(&mut green, 30);
    assert_eq!(frame(&gray), frame(&green));
}

#[test]
fn held_buttons_reach_the_game() {
    let mut core = booted(ROM_ONLY, 0, JOYPAD);
    assert_eq!(work_ram(&mut core, 0xC000) & 0x0F, 0x0F);
    assert_eq!(work_ram(&mut core, 0xC001) & 0x0F, 0x0F);

    run_holding(&mut core, 2, buttons(&[RIGHT, A]));
    assert_eq!(work_ram(&mut core, 0xC000) & 0x0F, 0b1110, "right");
    assert_eq!(work_ram(&mut core, 0xC001) & 0x0F, 0b1110, "A");

    run_holding(&mut core, 2, buttons(&[UP, START]));
    assert_eq!(work_ram(&mut core, 0xC000) & 0x0F, 0b1011, "up");
    assert_eq!(work_ram(&mut core, 0xC001) & 0x0F, 0b0111, "Start");

    run(&mut core, 2);
    assert_eq!(work_ram(&mut core, 0xC000) & 0x0F, 0x0F);
    assert_eq!(work_ram(&mut core, 0xC001) & 0x0F, 0x0F);
}

/// The buttons held now take over from a loaded state's at its first
/// frame, so a button held across a load, as run-ahead and rewind do every
/// frame, stays pressed.
#[test]
fn a_button_held_across_a_loaded_state_stays_pressed() {
    let mut core = booted(ROM_ONLY, 0, JOYPAD);
    let mut state = vec![0; core.serialize_size()];
    assert!(core.serialize(&mut state), "saved with A released");
    run_holding(&mut core, 1, buttons(&[A]));
    assert!(core.unserialize(&state).is_ok());
    run_holding(&mut core, 2, buttons(&[A]));
    assert_eq!(work_ram(&mut core, 0xC001) & 0x0F, 0b1110, "A");

    assert!(core.unserialize(&state).is_ok());
    run(&mut core, 2);
    assert_eq!(work_ram(&mut core, 0xC001) & 0x0F, 0x0F, "released");
}

/// Save states store the buttons the game saw, so a state saved with A held
/// and loaded while A is released goes on with A held exactly like the run
/// it was saved from, as netplay's rollback needs: A isn't pressed anew.
#[test]
fn a_state_saved_with_a_held_goes_on_the_same_when_loaded_with_a_released() {
    let mut core = booted(ROM_ONLY, 0, JOYPAD_INTERRUPTS);
    run_holding(&mut core, 2, buttons(&[A]));
    assert_eq!(work_ram(&mut core, 0xC000), 1, "pressing A requests it");
    let mut state = vec![0; core.serialize_size()];
    assert!(core.serialize(&mut state), "saved with A held");
    run_holding(&mut core, 2, buttons(&[A]));
    let mut original = vec![0; core.serialize_size()];
    assert!(core.serialize(&mut original));

    run(&mut core, 1);
    assert!(core.unserialize(&state).is_ok(), "loaded with A released");
    run_holding(&mut core, 2, buttons(&[A]));
    assert_eq!(work_ram(&mut core, 0xC000), 1, "A was held already");
    let mut replayed = vec![0; core.serialize_size()];
    assert!(core.serialize(&mut replayed));
    assert!(replayed == original, "went on differently");
}

#[test]
fn every_game_boy_button_has_its_own_libretro_button() {
    let mut keys: Vec<_> = BUTTONS.iter().map(|(_, key, _)| *key).collect();
    keys.sort_by_key(|key| format!("{key:?}"));
    keys.dedup();
    assert_eq!(keys.len(), 8);
    let mut ids: Vec<_> = BUTTONS.iter().map(|(id, _, _)| *id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 8);
}

#[test]
fn games_without_a_battery_have_no_save_buffers() {
    let mut core = load(&cartridge(ROM_ONLY, 0, false, JOYPAD));
    assert!(core.save_ram().is_empty());
    assert!(core.rtc_memory().is_empty());
    let mut core = load(&cartridge(0x02, RAM_8_KIB, false, RAM)); // MBC1 with RAM, no battery
    assert!(core.save_ram().is_empty());
}

#[test]
fn a_save_filled_in_before_the_first_frame_is_what_the_game_reads() {
    let mut core = load(&cartridge(MBC1_RAM_BATTERY, RAM_8_KIB, false, RAM));
    assert_eq!(core.save_ram().len(), 0x2000);
    assert!(core.rtc_memory().is_empty());
    core.save_ram()[0] = 0x5A;
    run(&mut core, BOOT_FRAMES);
    assert_eq!(work_ram(&mut core, 0xC000), 0x5A);
}

#[test]
fn what_the_game_saves_reaches_the_buffer() {
    let mut core = booted(MBC1_RAM_BATTERY, RAM_8_KIB, RAM);
    let first = core.save_ram()[1];
    run(&mut core, 1);
    let second = core.save_ram()[1];
    assert_ne!(first, second, "the counter at 0xA001 moved on");
    let cartridge_ram = core.emulator().cartridge_ram().to_vec();
    assert_eq!(core.save_ram(), cartridge_ram);
}

#[test]
fn a_save_retroarch_changes_while_the_game_runs_reaches_the_game() {
    let mut core = booted(MBC1_RAM_BATTERY, RAM_8_KIB, RAM);
    core.save_ram()[0] = 0x77;
    run(&mut core, 2);
    assert_eq!(work_ram(&mut core, 0xC000), 0x77);
}

#[test]
fn loading_a_state_puts_its_ram_in_the_buffer_and_keeps_it_there() {
    let mut core = booted(MBC1_RAM_BATTERY, RAM_8_KIB, RAM);
    core.save_ram()[0] = 0x5A;
    run(&mut core, 2);
    let mut state = vec![0; core.serialize_size()];
    assert!(core.serialize(&mut state));

    core.save_ram()[0] = 0x33;
    run(&mut core, 2);
    assert_eq!(work_ram(&mut core, 0xC000), 0x33);

    assert!(core.unserialize(&state).is_ok());
    assert_eq!(core.save_ram()[0], 0x5A);
    let cartridge_ram = core.emulator().cartridge_ram().to_vec();
    assert_eq!(core.save_ram(), cartridge_ram);
    run(&mut core, 2);
    assert_eq!(core.save_ram()[0], 0x5A, "the old save didn't come back");
    assert_eq!(work_ram(&mut core, 0xC000), 0x5A);
}

/// A clock showing 10:20:30 on day 3, brought up to date with the wall
/// clock just now.
fn ten_twenty_thirty() -> RtcState {
    RtcState {
        seconds: 30,
        minutes: 20,
        hours: 10,
        days: 3,
        base_timestamp: wall_clock_millis(),
        ..RtcState::default()
    }
}

/// The time the [`CLOCK`] program last read: hours, minutes and seconds.
fn game_time(core: &mut Core) -> (u8, u8, u8) {
    (
        work_ram(core, 0xC002),
        work_ram(core, 0xC001),
        work_ram(core, 0xC000),
    )
}

/// How many seconds the game's clock read past 10:20:30 (see [`ten_twenty_thirty`]).
fn seconds_past_ten_twenty_thirty((hours, minutes, seconds): (u8, u8, u8)) -> i64 {
    let total = |hours: u8, minutes: u8, seconds: u8| {
        i64::from(hours) * 3600 + i64::from(minutes) * 60 + i64::from(seconds)
    };
    total(hours, minutes, seconds) - total(10, 20, 30)
}

/// The whole real seconds since `started`, plus one for the second under way: as far as a
/// wall clock can have moved on.
fn real_seconds_since(started: Instant) -> i64 {
    i64::try_from(started.elapsed().as_secs()).unwrap() + 1
}

fn emulated_time() -> Options {
    Options {
        rtc_clock: RtcClock::EmulatedTime,
        ..Options::default()
    }
}

fn clock_game(options: Options) -> Core {
    load_with(
        &cartridge(MBC3_TIMER_RAM_BATTERY, RAM_32_KIB, false, CLOCK),
        options,
    )
}

#[test]
fn a_clock_filled_in_before_the_first_frame_is_what_the_game_reads() {
    for options in [Options::default(), emulated_time()] {
        let mut core = clock_game(options);
        assert_eq!(core.rtc_memory().len(), RtcState::BYTES);
        let started = Instant::now();
        core.rtc_memory()
            .copy_from_slice(&ten_twenty_thirty().to_bytes());
        run(&mut core, BOOT_FRAMES);
        let passed = seconds_past_ten_twenty_thirty(game_time(&mut core));
        let most = match options.rtc_clock {
            RtcClock::EmulatedTime => 4,
            RtcClock::WallClock => real_seconds_since(started),
        };
        assert!(
            (0..=most).contains(&passed),
            "{options:?}: {passed} seconds past 10:20:30"
        );
    }
}

#[test]
fn the_clock_the_game_keeps_reaches_the_buffer_in_the_wall_clock_layout() {
    let mut core = clock_game(emulated_time());
    core.rtc_memory()
        .copy_from_slice(&ten_twenty_thirty().to_bytes());
    run(&mut core, BOOT_FRAMES);
    let before = RtcState::from_bytes(core.rtc_memory()).unwrap();
    let started = Instant::now();
    run(&mut core, 120);
    let after = RtcState::from_bytes(core.rtc_memory()).unwrap();
    let seconds = |clock: &RtcState| u32::from(clock.minutes) * 60 + u32::from(clock.seconds);
    assert_eq!(
        seconds(&after) - seconds(&before),
        2,
        "two emulated seconds"
    );
    let behind = wall_clock_millis() - after.base_timestamp;
    let most = started.elapsed().as_secs_f64() * 1000.0 + 1000.0;
    assert!(
        (0.0..most).contains(&behind),
        "on the wall clock: {behind} ms"
    );
}

#[test]
fn loading_a_state_puts_its_clock_in_the_buffer_and_keeps_it_there() {
    let mut core = clock_game(emulated_time());
    core.rtc_memory()
        .copy_from_slice(&ten_twenty_thirty().to_bytes());
    run(&mut core, BOOT_FRAMES);
    let mut state = vec![0; core.serialize_size()];
    assert!(core.serialize(&mut state));
    let saved_clock = RtcState::from_bytes(core.rtc_memory()).unwrap();

    let mut later = ten_twenty_thirty();
    later.hours = 20;
    core.rtc_memory().copy_from_slice(&later.to_bytes());
    run(&mut core, 2);
    assert_eq!(game_time(&mut core).0, 20);

    assert!(core.unserialize(&state).is_ok());
    let loaded_clock = RtcState::from_bytes(core.rtc_memory()).unwrap();
    assert_eq!(
        (
            loaded_clock.hours,
            loaded_clock.minutes,
            loaded_clock.seconds
        ),
        (saved_clock.hours, saved_clock.minutes, saved_clock.seconds)
    );
    run(&mut core, 2);
    assert_eq!(
        game_time(&mut core).0,
        10,
        "the later clock didn't come back"
    );
    assert_eq!(RtcState::from_bytes(core.rtc_memory()).unwrap().hours, 10);
}

#[test]
fn a_restart_keeps_the_save_and_the_clock() {
    let mut core = clock_game(Options::default());
    core.rtc_memory()
        .copy_from_slice(&ten_twenty_thirty().to_bytes());
    core.save_ram()[0] = 0x5A;
    run(&mut core, BOOT_FRAMES);
    core.reset().unwrap();
    assert_eq!(core.save_ram()[0], 0x5A);
    let clock = RtcState::from_bytes(core.rtc_memory()).unwrap();
    assert_eq!((clock.days, clock.hours, clock.minutes), (3, 10, 20));
    run(&mut core, BOOT_FRAMES);
    assert_eq!(game_time(&mut core).0, 10);
    assert_eq!(core.save_ram()[0], 0x5A);
}

#[test]
fn a_restart_takes_on_the_model_and_the_clock_the_options_pick_now() {
    let mut core = clock_game(emulated_time());
    let started = Instant::now();
    core.rtc_memory()
        .copy_from_slice(&ten_twenty_thirty().to_bytes());
    run(&mut core, BOOT_FRAMES);
    assert_eq!(core.model(), Model::Dmg);

    core.set_options(Options {
        model: ModelPreference::Fixed(Model::CgbC),
        ..Options::default()
    });
    assert_eq!(core.model(), Model::Dmg, "not before the restart");
    core.reset().unwrap();
    assert_eq!(core.model(), Model::CgbC);
    run(&mut core, BOOT_FRAMES);
    // The emulated seconds of the first run, then the real ones since the restart.
    let passed = seconds_past_ten_twenty_thirty(game_time(&mut core));
    assert!(
        (0..=4 + real_seconds_since(started)).contains(&passed),
        "the clock jumped: {passed} seconds past 10:20:30"
    );

    core.set_options(emulated_time());
    core.reset().unwrap();
    run(&mut core, BOOT_FRAMES);
    assert_eq!(game_time(&mut core).0, 10);
}

/// Frames and sound of `frames` frames.
fn record(core: &mut Core, frames: usize) -> (Vec<Vec<u32>>, Vec<i16>) {
    let mut pictures = Vec::new();
    let mut sound = Vec::new();
    for _ in 0..frames {
        core.run(0);
        pictures.push(frame(core));
        sound.extend_from_slice(core.audio());
    }
    (pictures, sound)
}

#[test]
fn a_loaded_state_goes_on_exactly_like_the_run_it_was_saved_from() {
    for (cartridge_type, ram_size, cgb, program) in [
        (ROM_ONLY, 0, false, TONE),
        (ROM_ONLY, 0, true, TONE),
        (MBC1_RAM_BATTERY, RAM_8_KIB, false, RAM),
        (MBC3_TIMER_RAM_BATTERY, RAM_32_KIB, true, CLOCK),
    ] {
        let mut core = load_with(
            &cartridge(cartridge_type, ram_size, cgb, program),
            emulated_time(),
        );
        run(&mut core, 50);
        let size = core.serialize_size();
        let mut state = vec![0xFF; size];
        assert!(core.serialize(&mut state));
        let first = record(&mut core, 100);

        assert!(core.unserialize(&state).is_ok());
        let second = record(&mut core, 100);
        assert!(
            first == second,
            "type {cartridge_type:#04X}: went on differently"
        );
        assert_eq!(core.serialize_size(), size);
    }
}

#[test]
fn save_states_are_padded_to_one_size_with_their_length_first() {
    let mut core = booted(ROM_ONLY, 0, TONE);
    let mut state = vec![0xFF; core.serialize_size()];
    assert!(core.serialize(&mut state));
    let length = u32::from_le_bytes(state[..4].try_into().unwrap()) as usize;
    assert_eq!(&state[4..20], b"NUBO SAVE STATE\0");
    assert!(state[4 + length..].iter().all(|&byte| byte == 0));
    assert!(state.len() - 4 - length >= STATE_SIZE_MARGIN);
}

#[test]
fn a_restart_onto_a_super_game_boy_still_saves_states() {
    let mut core = booted(ROM_ONLY, 0, TONE);
    let size = core.serialize_size();
    for model in [Model::Sgb2, Model::CgbE, Model::Dmg] {
        core.set_options(Options {
            model: ModelPreference::Fixed(model),
            ..Options::default()
        });
        core.reset().unwrap();
        run(&mut core, BOOT_FRAMES);
        assert_eq!(core.model(), model);
        assert_eq!(core.serialize_size(), size, "RetroArch's size stays");
        let mut state = vec![0; size];
        assert!(core.serialize(&mut state), "a state on the {model:?} fits");
        assert!(core.unserialize(&state).is_ok());
    }
}

#[test]
fn cartridge_ram_is_copied_as_its_index_lays_it_out() {
    for length in [0, 512, 2048, 0x2000, 0x8000, CARTRIDGE_RAM_SIZE] {
        let ram: Vec<u8> = (0..length).map(|index| (index % 251) as u8 + 1).collect();
        let mut copied = vec![0xEE; CARTRIDGE_RAM_SIZE];
        copy_cartridge_ram(&mut copied, &ram);
        let expected: Vec<u8> = (0..CARTRIDGE_RAM_SIZE)
            .map(|index| cartridge_ram_index(index, length).map_or(0, |index| ram[index]))
            .collect();
        assert!(copied == expected, "RAM of {length} bytes");
    }
}

#[test]
fn a_save_state_buffer_too_small_is_left_alone() {
    let mut core = booted(ROM_ONLY, 0, TONE);
    let mut small = vec![0xAB; 100];
    assert!(!core.serialize(&mut small));
    assert!(small.iter().all(|&byte| byte == 0xAB));
}

#[test]
fn damaged_or_foreign_states_do_not_load_and_change_nothing() {
    let mut core = booted(MBC1_RAM_BATTERY, RAM_8_KIB, RAM);
    let mut state = vec![0; core.serialize_size()];
    assert!(core.serialize(&mut state));
    let mut before = vec![0; core.serialize_size()];

    // Nubo tells games apart by the title and checksum in their header.
    let mut other_rom = cartridge(ROM_ONLY, 0, false, TONE);
    other_rom[0x0134] = b'X';
    let mut other_game = load(&other_rom);
    run(&mut other_game, 2);
    let mut other_state = vec![0; other_game.serialize_size()];
    assert!(other_game.serialize(&mut other_state));

    let mut too_long = state.clone();
    too_long[..4].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut cut_off = state.clone();
    cut_off.truncate(state.len() / 2);
    let mut short = state.clone();
    let length = u32::from_le_bytes(state[..4].try_into().unwrap());
    short[..4].copy_from_slice(&(length - 10).to_le_bytes());
    for bad in [
        &[][..],
        &[1, 2, 3],
        &too_long,
        &cut_off,
        &short,
        &other_state,
    ] {
        assert!(core.serialize(&mut before));
        assert!(core.unserialize(bad).is_err());
        let mut after = vec![0; core.serialize_size()];
        assert!(core.serialize(&mut after));
        assert!(before == after, "a failed load changed the game");
    }
}

#[test]
fn a_state_from_another_model_says_so() {
    let mut core = booted(MBC1_RAM_BATTERY, RAM_8_KIB, RAM);
    let mut state = vec![0; core.serialize_size()];
    assert!(core.serialize(&mut state));
    core.set_options(Options {
        model: ModelPreference::Fixed(Model::CgbC),
        ..Options::default()
    });
    core.reset().unwrap();
    assert_eq!(
        core.unserialize(&state),
        Err(SaveStateError::DifferentModel {
            found: Model::Dmg,
            expected: Model::CgbC,
        })
    );
}

/// How much a state's length can change while a game runs: every program
/// above on both models, at every frame for a few seconds, stays far within
/// the margin.
#[test]
fn save_states_grow_by_far_less_than_the_margin() {
    let mut lengths = Vec::new();
    for program in [TONE, JOYPAD, RAM, CLOCK, CHEATS] {
        for cgb in [false, true] {
            let mut core = load(&cartridge(MBC3_TIMER_RAM_BATTERY, RAM_32_KIB, cgb, program));
            for frame in 0..300 {
                core.run(if frame % 7 == 0 { buttons(&[A]) } else { 0 });
                let mut state = vec![0; core.serialize_size()];
                assert!(core.serialize(&mut state));
                lengths.push(u32::from_le_bytes(state[..4].try_into().unwrap()));
            }
        }
    }
    let spread = lengths.iter().max().unwrap() - lengths.iter().min().unwrap();
    assert!(
        (spread as usize) < STATE_SIZE_MARGIN / 8,
        "states vary by {spread} bytes"
    );
}

/// The byte the [`CHEATS`] program copied from 0xC100 and from ROM 0x0200.
fn cheated(core: &mut Core) -> (u8, u8) {
    (work_ram(core, 0xC000), work_ram(core, 0xC001))
}

#[test]
fn game_genie_and_gameshark_codes_work_alone_and_together() {
    let mut core = booted(ROM_ONLY, 0, CHEATS);
    assert_eq!(cheated(&mut core), (0x00, ROM_BYTE_AT_0200));

    assert!(core.cheat_set(0, true, "014200C1").is_empty());
    run(&mut core, 2);
    assert_eq!(cheated(&mut core), (0x42, ROM_BYTE_AT_0200));

    assert!(core.cheat_set(1, true, "772-00F").is_empty());
    run(&mut core, 2);
    assert_eq!(cheated(&mut core), (0x42, 0x77));

    core.cheat_reset();
    run(&mut core, 2);
    assert_eq!(cheated(&mut core), (0x00, ROM_BYTE_AT_0200));

    assert!(core.cheat_set(5, true, " 014200C1 + 772-00F ").is_empty());
    run(&mut core, 2);
    assert_eq!(cheated(&mut core), (0x42, 0x77));
}

#[test]
fn a_cheat_set_again_loses_the_codes_it_had() {
    let mut core = booted(ROM_ONLY, 0, CHEATS);
    assert!(core.cheat_set(3, true, "014200C1+772-00F").is_empty());
    assert!(core.cheat_set(3, true, "015500C1").is_empty());
    run(&mut core, 2);
    assert_eq!(cheated(&mut core), (0x55, ROM_BYTE_AT_0200));

    assert!(core.cheat_set(3, false, "015500C1").is_empty());
    run(&mut core, 2);
    assert_eq!(cheated(&mut core), (0x00, ROM_BYTE_AT_0200));
}

#[test]
fn invalid_cheat_codes_are_reported_and_the_valid_ones_still_work() {
    let mut core = booted(ROM_ONLY, 0, CHEATS);
    let invalid = core.cheat_set(0, true, "nonsense+014200C1+0142FFFF");
    assert_eq!(
        invalid,
        [
            InvalidCheat(String::from("nonsense")),
            InvalidCheat(String::from("0142FFFF"))
        ]
    );
    run(&mut core, 2);
    assert_eq!(cheated(&mut core), (0x42, ROM_BYTE_AT_0200));
}

#[test]
fn cheats_stay_on_after_a_restart() {
    let mut core = booted(ROM_ONLY, 0, CHEATS);
    assert!(core.cheat_set(0, true, "014200C1").is_empty());
    core.reset().unwrap();
    run(&mut core, BOOT_FRAMES);
    assert_eq!(cheated(&mut core), (0x42, ROM_BYTE_AT_0200));
}

#[test]
fn the_memory_regions_fit_libretros_memory_descriptors() {
    for cgb in [false, true] {
        let mut core = load(&cartridge(MBC1_RAM_BATTERY, RAM_8_KIB, cgb, RAM));
        let regions = core.memory_regions();
        let mut covered = Vec::new();
        for region in &regions {
            assert!(region.length.is_power_of_two(), "{region:?}");
            assert_eq!(region.start % region.length, 0, "{region:?}");
            assert!(region.buffer.len() >= region.length, "{region:?}");
            assert_eq!(region.start & !region.select(), 0);
            assert_eq!(region.select() & (region.length - 1), 0);
            covered.push(region.start..region.start + region.length);
        }
        covered.sort_by_key(|range| range.start);
        for pair in covered.windows(2) {
            assert!(pair[0].end <= pair[1].start, "{pair:?} overlap");
        }
        // Where rcheevos reads a Game Boy (Color), as rc_consoles.c lists it.
        let mut read = vec![
            0x0000..0x8000,
            0x8000..0xA000,
            0xA000..0xC000,
            0xC000..0xE000,
            0xFE00..0xFEA0,
            0xFF00..0x1_0000,
            0x1_6000..0x3_4000,
        ];
        if cgb {
            read.push(0x1_0000..0x1_6000);
        }
        for range in read {
            for address in range.clone().step_by(0x10) {
                assert!(
                    covered.iter().any(|region| region.contains(&address)),
                    "{address:#X} isn't covered (cgb: {cgb})"
                );
            }
        }
    }
}

#[test]
fn the_memory_regions_hold_what_the_game_wrote_after_each_frame() {
    let mut core = booted(MBC1_RAM_BATTERY, RAM_8_KIB, RAM);
    core.save_ram()[0] = 0x6B;
    run(&mut core, 2);
    let regions = core.memory_regions();
    let read = |address: usize| {
        let region = regions
            .iter()
            .find(|region| (region.start..region.start + region.length).contains(&address))
            .unwrap();
        region.buffer[address - region.start]
    };
    assert_eq!(read(0xC000), 0x6B, "work RAM");
    assert_eq!(read(0xA000), 0x6B, "cartridge RAM");
    assert_eq!(read(0x0147), MBC1_RAM_BATTERY, "the ROM header");
    assert_eq!(read(0x0200), ROM_BYTE_AT_0200);
    assert_eq!(read(0xFF40) & 0x80, 0x80, "LCDC: the screen is on");
}

/// The buffer and offset of `address` in the regions the memory map
/// describes.
fn region_byte(core: &mut Core, address: usize) -> &mut u8 {
    let region = core
        .memory_regions()
        .into_iter()
        .find(|region| (region.start..region.start + region.length).contains(&address))
        .unwrap();
    &mut region.buffer[address - region.start]
}

#[test]
fn what_retroarch_writes_to_work_ram_reaches_the_game() {
    let mut core = booted(ROM_ONLY, 0, CHEATS);
    core.system_ram()[0x100] = 0x42;
    run(&mut core, 2);
    assert_eq!(cheated(&mut core), (0x42, ROM_BYTE_AT_0200));
    assert_eq!(core.system_ram()[0x100], 0x42, "it stays");

    *region_byte(&mut core, 0xC100) = 0x43;
    run(&mut core, 2);
    assert_eq!(cheated(&mut core).0, 0x43);
}

#[test]
fn what_retroarch_writes_to_banked_memory_reaches_the_bank() {
    let mut core = load(&cartridge(MBC1_RAM_BATTERY, RAM_32_KIB, true, RAM));
    run(&mut core, BOOT_FRAMES);
    *region_byte(&mut core, 0x1_4123) = 0x61; // work RAM bank 6
    *region_byte(&mut core, 0x1_6123) = 0x62; // cartridge RAM bank 1
    *region_byte(&mut core, 0x8010) = 0x63;
    *region_byte(&mut core, 0xFE04) = 0x64;
    *region_byte(&mut core, 0xFF90) = 0x65;
    run(&mut core, 1);

    let emulator = core.emulator();
    assert_eq!(emulator.work_ram()[0x6123], 0x61);
    assert_eq!(emulator.cartridge_ram()[0x2123], 0x62);
    assert_eq!(emulator.high_ram()[0x10], 0x65);
    let (video_ram, oam) = emulator.video_memory();
    assert_eq!((video_ram[0x10], oam[0x04]), (0x63, 0x64));
}

#[test]
fn what_retroarch_writes_to_battery_ram_reaches_the_game_and_the_save() {
    let mut core = booted(MBC1_RAM_BATTERY, RAM_8_KIB, RAM);
    *region_byte(&mut core, 0xA000) = 0x6B;
    run(&mut core, 2);
    assert_eq!(work_ram(&mut core, 0xC000), 0x6B, "the game read it");
    assert_eq!(core.save_ram()[0], 0x6B, "the .srm keeps it");
}

#[test]
fn ram_shorter_than_a_bank_repeats_through_its_window_for_reads_and_writes() {
    let mut core = booted(MBC1_RAM_BATTERY, RAM_2_KIB, RAM);
    *region_byte(&mut core, 0xA800) = 0x6B;
    run(&mut core, 2);
    assert_eq!(
        work_ram(&mut core, 0xC000),
        0x6B,
        "the game read it at 0xA000"
    );
    assert_eq!(*region_byte(&mut core, 0xA000), 0x6B);
    let counter = *region_byte(&mut core, 0xA001);
    assert_eq!(*region_byte(&mut core, 0xB801), counter);
    assert_eq!(core.save_ram()[0], 0x6B);
}

#[test]
fn a_state_saved_after_retroarch_loaded_the_save_and_wrote_memory_keeps_both() {
    let mut core = booted(MBC1_RAM_BATTERY, RAM_8_KIB, RAM);
    core.save_ram()[1] = 0x55;
    *region_byte(&mut core, 0xA000) = 0x6B;
    let mut state = vec![0; core.serialize_size()];
    assert!(core.serialize(&mut state));
    assert_eq!(
        &core.save_ram()[..2],
        &[0x6B, 0x55],
        "the loaded .srm survives"
    );
    assert_eq!(core.emulator().cartridge_ram()[..2], [0x6B, 0x55]);
}

#[test]
fn hardware_registers_ie_and_memory_past_the_cartridge_ram_stay_read_only() {
    let mut core = booted(ROM_ONLY, 0, JOYPAD);
    let lcdc = *region_byte(&mut core, 0xFF40);
    let interrupt_enable = *region_byte(&mut core, 0xFFFF);
    *region_byte(&mut core, 0xFF40) = !lcdc;
    *region_byte(&mut core, 0xFFFF) = !interrupt_enable;
    *region_byte(&mut core, 0x3_0000) = 0x77; // no cartridge RAM at all
    run(&mut core, 2);
    assert_eq!(*region_byte(&mut core, 0xFF40), lcdc);
    assert_eq!(*region_byte(&mut core, 0xFFFF), interrupt_enable);
    assert_eq!(*region_byte(&mut core, 0x3_0000), 0);
    assert_eq!(core.emulator().read_memory(0xFF40), lcdc);
}

#[test]
fn a_state_saved_after_retroarch_wrote_memory_holds_the_write() {
    let mut core = booted(ROM_ONLY, 0, CHEATS);
    core.system_ram()[0x100] = 0x42;
    let mut state = vec![0; core.serialize_size()];
    assert!(core.serialize(&mut state));
    core.system_ram()[0x100] = 0x00;
    run(&mut core, 2);
    assert!(core.unserialize(&state).is_ok());
    assert_eq!(core.system_ram()[0x100], 0x42);
    run(&mut core, 2);
    assert_eq!(cheated(&mut core).0, 0x42);
}

#[test]
fn a_game_nobody_writes_to_runs_exactly_as_before() {
    let rom = cartridge(MBC1_RAM_BATTERY, RAM_8_KIB, true, RAM);
    let mut with_reads = load(&rom);
    let mut without = load(&rom);
    for _ in 0..120 {
        with_reads.run(0);
        let _ = with_reads.memory_regions().len();
        without.run(0);
    }
    assert_eq!(frame(&with_reads), frame(&without));
    let mut first = vec![0; with_reads.serialize_size()];
    let mut second = vec![0; without.serialize_size()];
    assert!(with_reads.serialize(&mut first));
    assert!(without.serialize(&mut second));
    assert!(first == second);
}

#[test]
fn rumble_cartridges_rumble_and_others_do_not() {
    let core = booted(MBC5_RUMBLE, 0, RUMBLE);
    assert!(core.has_rumble());
    assert_eq!(core.rumble(), u16::MAX);

    let core = booted(MBC1_RAM_BATTERY, RAM_8_KIB, RAM);
    assert!(!core.has_rumble());
    assert_eq!(core.rumble(), 0);
}
