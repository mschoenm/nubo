//! Tests for battery saves. They write real files in the temporary
//! directory, so they only run in the native build.

use super::*;
use nubo_core::{Emulator, Model};
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

const CART_TYPE_MBC1_RAM_BATTERY: u8 = 0x03;
const ROM_SIZE_32KB: u8 = 0x00;
const RAM_SIZE_8KB: u8 = 0x02;

/// A saves folder in the temporary directory that doesn't exist yet.
fn temporary_saves_directory(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("nubo-battery-{}-{}", name, process::id()));
    let _ = fs::remove_dir_all(&directory);
    directory.join("saves")
}

/// The battery saves of `roms/game.gb`, kept in `directory`.
fn saves_in(directory: &Path) -> Arc<BatterySaves> {
    BatterySaves::in_directory(directory, &Path::new("roms").join("game.gb"))
}

fn battery_backed_rom() -> Vec<u8> {
    let mut rom = vec![0; 0x8000];
    rom[0x134..0x13B].copy_from_slice(b"BATTERY");
    rom[0x147] = CART_TYPE_MBC1_RAM_BATTERY;
    rom[0x148] = ROM_SIZE_32KB;
    rom[0x149] = RAM_SIZE_8KB;
    rom
}

fn sample_rtc() -> RtcState {
    RtcState {
        milliseconds: 999,
        seconds: 59,
        minutes: 30,
        hours: 23,
        days: 511,
        base_timestamp: 1_791_633_740_867.5,
        halted: true,
        day_carry: true,
    }
}

#[test]
fn rtc_state_round_trips_through_encoding() {
    let decoded = decode_rtc(&encode_rtc(&sample_rtc())).unwrap();

    assert_eq!(decoded.milliseconds, 999);
    assert_eq!(decoded.seconds, 59);
    assert_eq!(decoded.minutes, 30);
    assert_eq!(decoded.hours, 23);
    assert_eq!(decoded.days, 511);
    assert_eq!(decoded.base_timestamp, 1_791_633_740_867.5);
    assert!(decoded.halted);
    assert!(decoded.day_carry);
}

#[test]
fn rtc_file_with_wrong_size_is_ignored() {
    assert!(decode_rtc(&[0; RTC_FILE_SIZE - 1]).is_none());
}

#[test]
fn flush_creates_saves_directory_and_writes_queued_ram_and_rtc() {
    let directory = temporary_saves_directory("flush");
    let saves = saves_in(&directory);

    let now = Instant::now();
    saves.queue_ram(&[1, 2, 3, 4], now);
    saves.queue_ram(&[5, 6, 7, 8], now);
    saves.queue_rtc(&sample_rtc(), now);
    saves.flush();

    assert_eq!(
        fs::read(directory.join("game.sav")).unwrap(),
        vec![5, 6, 7, 8]
    );
    assert_eq!(saves_in(&directory).load_rtc().unwrap().days, 511);
}

#[test]
fn a_save_is_written_once_the_game_stops_writing() {
    let directory = temporary_saves_directory("settle");
    let saves = saves_in(&directory);
    let start = Instant::now();
    let later = |millis| start + Duration::from_millis(millis);

    saves.queue_ram(&[1; RAM_SIZE_GRANULARITY], start);
    saves.queue_ram(&[2; RAM_SIZE_GRANULARITY], later(200));
    saves.flush_due(later(400));
    assert!(!directory.join("game.sav").exists());

    saves.flush_due(later(200) + SETTLE_TIME);
    assert_eq!(
        fs::read(directory.join("game.sav")).unwrap(),
        [2; RAM_SIZE_GRANULARITY]
    );
}

#[test]
fn a_game_that_keeps_writing_is_saved_after_the_longest_delay() {
    let directory = temporary_saves_directory("busy");
    let saves = saves_in(&directory);
    let start = Instant::now();
    let mut now = start;

    while now < start + LONGEST_DELAY {
        saves.queue_ram(&[0xAB; RAM_SIZE_GRANULARITY], now);
        saves.flush_due(now);
        assert!(!directory.join("game.sav").exists());
        now += SETTLE_TIME / 2;
    }

    saves.flush_due(start + LONGEST_DELAY);
    assert!(directory.join("game.sav").exists());
}

#[test]
fn a_clock_that_keeps_changing_doesnt_hold_up_the_ram() {
    let directory = temporary_saves_directory("clock");
    let saves = saves_in(&directory);
    let start = Instant::now();

    saves.queue_ram(&[0xCD; RAM_SIZE_GRANULARITY], start);
    saves.queue_rtc(&sample_rtc(), start + SETTLE_TIME);
    saves.flush_due(start + SETTLE_TIME);

    assert!(directory.join("game.sav").exists());
    assert!(!directory.join("game.rtc").exists());
}

#[test]
fn load_ram_returns_none_without_save_file() {
    let directory = temporary_saves_directory("missing");

    assert!(saves_in(&directory).load_ram().is_none());
}

#[test]
fn write_fails_without_saves_directory() {
    let slot = SaveSlot::in_directory(None, Path::new("game.gb"), "sav");

    assert!(slot.read().is_none());
    assert!(slot.write(&[1, 2, 3]).is_err());
}

#[test]
fn load_ram_drops_trailing_rtc_footer_from_other_emulators() {
    let directory = temporary_saves_directory("footer");
    let mut contents = vec![0x42; 0x2000];
    contents.extend_from_slice(&[0xFF; 48]);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("game.sav"), contents).unwrap();

    let ram = saves_in(&directory).load_ram().unwrap();

    assert_eq!(ram, vec![0x42; 0x2000]);
}

#[test]
fn emulator_starts_with_ram_from_existing_save_file() {
    let directory = temporary_saves_directory("emulator");
    let saved_ram: Vec<u8> = (0..0x2000).map(|index| index as u8).collect();
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("game.sav"), &saved_ram).unwrap();

    let mut emulator = Emulator::new(Model::Dmg);
    let effects = BatteryCartridgeEffects::new(saves_in(&directory));
    emulator
        .load_rom(&battery_backed_rom(), Box::new(effects))
        .unwrap();

    assert_eq!(emulator.cartridge_ram(), saved_ram);
}
