//! Tests for the emulator's command handling, shared by the desktop and the
//! browser sessions.

use super::*;
use crate::storage::{Entry, Storage};
use nubo_core::CartridgeEffects;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

/// A storage that keeps its entries in memory, by file name, so the tests
/// don't depend on the platform's storage.
#[derive(Default)]
struct Memory {
    entries: Mutex<HashMap<String, Vec<u8>>>,
}

impl Memory {
    /// What was written for `entry`, if anything.
    fn written(&self, entry: Entry<'_>) -> Option<Vec<u8>> {
        self.entries
            .lock()
            .unwrap()
            .get(&self.location(entry))
            .cloned()
    }
}

impl Storage for Memory {
    fn read(&self, entry: Entry<'_>) -> Result<Option<Vec<u8>>, String> {
        Ok(self.written(entry))
    }

    fn write(&self, entry: Entry<'_>, contents: &[u8]) -> Result<(), String> {
        self.entries
            .lock()
            .unwrap()
            .insert(self.location(entry), contents.to_vec());
        Ok(())
    }

    fn location(&self, entry: Entry<'_>) -> String {
        entry.file_name().display().to_string()
    }
}

const NORMAL_SPEED: Playback = Playback {
    running: true,
    fast_forward: None,
    volume: 1.0,
    rewinding: false,
};

/// A machine running an empty cartridge, without a sound device, keeping its
/// battery saves in `storage`, and those saves.
fn machine_with(storage: Arc<Memory>) -> (Machine, Arc<BatterySaves>) {
    let saves = BatterySaves::in_storage(storage, Path::new("game.gb"), Notices::default());
    let machine = Machine::new(
        &[0; 0x8000],
        Saves::Kept(saves.clone()),
        AudioOutput::without_device(),
        Model::Dmg,
        NORMAL_SPEED,
    )
    .unwrap();
    (machine, saves)
}

/// A machine running an empty cartridge, without a sound device.
fn machine() -> Machine {
    machine_with(Arc::default()).0
}

fn save_state(machine: &mut Machine) -> Vec<u8> {
    ask(
        |command| {
            machine.handle(command);
        },
        Command::SaveState,
    )
    .unwrap()
}

#[test]
fn a_loaded_state_takes_the_machine_back_to_when_it_was_saved() {
    let mut machine = machine();
    machine.play_buffer();
    let saved = save_state(&mut machine);

    machine.play_buffer();
    machine.skip_buffer();
    let state = saved.clone();
    ask(
        |command| {
            machine.handle(command);
        },
        |reply| Command::LoadState(state, reply),
    )
    .unwrap()
    .unwrap();

    assert_eq!(save_state(&mut machine), saved);
}

#[test]
fn a_state_that_isnt_one_is_refused() {
    let mut machine = machine();

    let result = ask(
        |command| {
            machine.handle(command);
        },
        |reply| Command::LoadState(vec![1, 2, 3], reply),
    )
    .unwrap();

    assert!(result.is_err());
}

#[test]
fn pausing_writes_the_battery_saves_still_waiting() {
    let storage = Arc::new(Memory::default());
    let (mut machine, saves) = machine_with(storage.clone());
    let mut cartridge = BatteryCartridgeEffects::new(saves);
    cartridge.save_ram(&[0x42; 0x2000]);

    machine.handle(Command::SetPlayback(Playback {
        running: false,
        ..NORMAL_SPEED
    }));

    let saved = storage.written(Entry::BatteryRam(Path::new("game.gb")));
    assert_eq!(saved, Some(vec![0x42; 0x2000]));
}

#[test]
fn discarded_saves_keep_nothing_and_count_emulated_time() {
    let saves = Saves::Discarded;
    let mut cartridge = saves.cartridge_effects();

    cartridge.save_ram(&[0x42; 0x2000]);
    saves.flush();

    assert_eq!(cartridge.load_ram(), None);
    assert_eq!(cartridge.rtc_clock(), RtcClock::EmulatedTime);
    assert_eq!(
        Saves::Kept(BatterySaves::in_storage(
            Arc::new(Memory::default()),
            Path::new("game.gb"),
            Notices::default()
        ))
        .cartridge_effects()
        .rtc_clock(),
        RtcClock::WallClock
    );
}

#[test]
fn a_played_buffer_says_how_long_its_sound_plays() {
    let mut machine = machine();

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

#[test]
fn rewinding_steps_back_through_the_states_kept_while_playing() {
    let (mut machine, _) = machine_with(Arc::new(Memory::default()));
    let mut frames = 0;
    while frames < history::FRAMES_PER_STATE * 10 {
        if machine.play_buffer().1 {
            frames += 1;
        }
    }
    let kept = machine.history.len();
    assert!(kept >= 9, "{kept}");

    machine.handle(Command::SetPlayback(Playback {
        rewinding: true,
        ..NORMAL_SPEED
    }));
    let mut shown = 0;
    for _ in 0..200 {
        if machine.rewind_buffer().1 {
            shown += 1;
        }
    }

    assert_eq!(shown, kept);
    assert_eq!(machine.history.len(), 0);
}

#[test]
fn rewinding_keeps_its_speed_whatever_the_buffers_length() {
    let (mut machine, _) = machine_with(Arc::new(Memory::default()));
    let mut frames = 0;
    while frames < history::FRAMES_PER_STATE * 20 {
        if machine.play_buffer().1 {
            frames += 1;
        }
    }

    machine.handle(Command::SetPlayback(Playback {
        rewinding: true,
        ..NORMAL_SPEED
    }));
    let step = FRAME_DURATION.mul_f64(f64::from(history::FRAMES_PER_STATE) / REWIND_SPEED);
    let (mut time, mut buffer, mut shown) = (Duration::ZERO, Duration::ZERO, 0);
    while shown < 16 {
        let (playing_time, new_frame) = machine.rewind_buffer();
        time += playing_time;
        buffer = playing_time;
        if new_frame {
            shown += 1;
        }
    }

    assert!(
        time < step * 16 + buffer,
        "{time:?} for 16 steps of {step:?}"
    );
}

#[test]
fn loading_a_state_forgets_the_history_before_it() {
    let (mut machine, _) = machine_with(Arc::new(Memory::default()));
    let state = machine.emulator.save_state();
    let mut frames = 0;
    while frames < history::FRAMES_PER_STATE * 3 {
        if machine.play_buffer().1 {
            frames += 1;
        }
    }
    assert!(machine.history.len() > 0);

    let (reply, answer) = mpsc::channel();
    machine.handle(Command::LoadState(state, reply));

    assert!(answer.recv().unwrap().is_ok());
    assert_eq!(machine.history.len(), 0);
}
