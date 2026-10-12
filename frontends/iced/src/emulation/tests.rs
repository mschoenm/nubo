//! Tests for the emulator's command handling, shared by the desktop and the
//! browser sessions.

use super::*;
use nubo_core::CartridgeEffects;
use std::fs;
use std::path::{Path, PathBuf};

/// The folder in the temporary directory for a test's battery saves.
fn saves_directory(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("nubo-emulation-{name}-{}", std::process::id()))
}

const NORMAL_SPEED: Playback = Playback {
    running: true,
    fast_forward: None,
    volume: 1.0,
};

/// A machine running an empty cartridge, without a sound device.
fn machine(name: &str) -> Machine {
    let directory = saves_directory(name);
    let _ = fs::remove_dir_all(&directory);
    let saves = BatterySaves::in_directory(&directory, Path::new("game.gb"));
    Machine::new(
        &[0; 0x8000],
        saves,
        AudioOutput::without_device(),
        Model::Dmg,
        NORMAL_SPEED,
    )
    .unwrap()
}

fn save_state(machine: &mut Machine) -> Vec<u8> {
    ask(|command| machine.handle(command), Command::SaveState).unwrap()
}

#[test]
fn a_loaded_state_takes_the_machine_back_to_when_it_was_saved() {
    let mut machine = machine("load");
    machine.play_buffer();
    let saved = save_state(&mut machine);

    machine.play_buffer();
    machine.skip_buffer();
    let state = saved.clone();
    ask(
        |command| machine.handle(command),
        |reply| Command::LoadState(state, reply),
    )
    .unwrap()
    .unwrap();

    assert_eq!(save_state(&mut machine), saved);
}

#[test]
fn a_state_that_isnt_one_is_refused() {
    let mut machine = machine("refuse");

    let result = ask(
        |command| machine.handle(command),
        |reply| Command::LoadState(vec![1, 2, 3], reply),
    )
    .unwrap();

    assert!(result.is_err());
}

#[test]
fn pausing_writes_the_battery_saves_still_waiting() {
    let mut machine = machine("pause");
    let mut cartridge = BatteryCartridgeEffects::new(machine.saves.clone());
    cartridge.save_ram(&[0x42; 0x2000]);

    machine.handle(Command::SetPlayback(Playback {
        running: false,
        ..NORMAL_SPEED
    }));

    let saved = fs::read(saves_directory("pause").join("game.sav")).unwrap();
    assert_eq!(saved, [0x42; 0x2000]);
}

#[test]
fn a_played_buffer_says_how_long_its_sound_plays() {
    let mut machine = machine("playing-time");

    let (playing_time, _) = machine.play_buffer();

    assert!(playing_time > Duration::ZERO);
    assert!(playing_time < Duration::from_millis(100));
}

#[test]
fn fast_forward_emulates_its_speed_in_buffers_per_played_one() {
    let fast = Playback {
        fast_forward: Some(FastForwardSpeed::default()),
        ..NORMAL_SPEED
    };

    assert_eq!(NORMAL_SPEED.buffers_per_played(), 1);
    assert_eq!(
        fast.buffers_per_played(),
        FastForwardSpeed::default().multiplier()
    );
}
