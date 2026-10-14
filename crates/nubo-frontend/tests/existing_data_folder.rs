//! A player's data folder from before the stored data moved to
//! `nubo-frontend` must keep working unchanged.
//!
//! `existing_data_folder/` was written by Nubo's app as of step 3 of
//! `docs/RESTRUCTURE_PLAN.md`, through the same functions it saves with,
//! into the folders `ProjectDirs` picked (`config/` and `data/` stand for
//! `~/.config/nubo` and `~/.local/share/nubo` on Linux). It holds the
//! settings, the battery save and clock of `Pocket Game.gbc`, two of its save
//! states (one with its picture) and its cheats. These tests read all of it
//! with this version, and check that saving the same things again writes the
//! same bytes.

#![cfg(feature = "desktop")]

use nubo_core::{CartridgeEffects, ControllerButton, Emulator, Model, RtcState};
use nubo_frontend::battery::{BatteryCartridgeEffects, BatterySaves};
use nubo_frontend::cheats::{self, Cheat};
use nubo_frontend::controls::{Action, KeyCode, PadButton};
use nubo_frontend::model::ModelPreference;
use nubo_frontend::notices::Notices;
use nubo_frontend::palettes::DmgPalette;
use nubo_frontend::save_states::{self, SaveState, Slot};
use nubo_frontend::settings::{self, LibraryFolder, Settings};
use nubo_frontend::storage::files::Files;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Where the game was when its data was saved; the file name names its data.
const ROM_PATH: &str = "/home/player/Games/Pocket Game.gbc";

/// The data folder in the tests.
fn existing() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/existing_data_folder")
}

/// The data folder as a storage, as the app finds it in the player's folders.
fn existing_files() -> Files {
    Files::in_folders(
        Some(existing().join("config")),
        Some(existing().join("data")),
    )
}

/// An empty folder in the temporary directory, to save to.
fn temporary_folder(name: &str) -> PathBuf {
    let folder =
        std::env::temp_dir().join(format!("nubo-existing-data-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    folder
}

/// The battery-backed RAM that was saved.
fn saved_ram() -> Vec<u8> {
    (0..0x2000_u32).map(|index| (index * 7) as u8).collect()
}

/// The clock that was saved.
fn saved_clock() -> RtcState {
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

/// The screen saved with the quick save state.
fn saved_screen() -> Vec<u8> {
    (0..144_u32)
        .flat_map(|y| (0..160_u32).flat_map(move |x| [x as u8, y as u8, (x ^ y) as u8, 255]))
        .collect()
}

/// The cheats that were saved.
fn saved_cheats() -> Vec<Cheat> {
    let mut disabled = Cheat::new("", "00a17bc49").unwrap();
    disabled.enabled = false;
    vec![Cheat::new("Infinite health", "01ff56d0").unwrap(), disabled]
}

#[test]
fn the_settings_read_the_same() {
    let (settings, problem) = settings::load(&existing_files(), |contents| {
        toml::from_str::<Settings>(contents)
    });

    assert_eq!(problem, None);
    assert_eq!(settings.model, ModelPreference::Fixed(Model::CgbC));
    assert_eq!(
        settings.library_folders[1],
        LibraryFolder {
            path: PathBuf::from("/media/player/USB stick/roms"),
            include_subfolders: false,
        }
    );
    assert!(!settings.download_box_art);
    assert!(settings.resume_games);
    assert_eq!(settings.fast_forward_speed.multiplier(), 3);
    assert_eq!(settings.volume, 0.65);
    assert!(settings.muted);
    assert_eq!(settings.dmg_palette, DmgPalette::Pocket);
    assert_eq!(
        settings.controls.keyboard.input(Action::Up),
        Some(KeyCode::KeyW)
    );
    assert_eq!(settings.controls.keyboard.input(Action::Screenshot), None);
    assert_eq!(
        settings.controls.controller.input(Action::PauseMenu),
        Some(PadButton(ControllerButton::RightStick))
    );
}

#[test]
fn the_battery_save_and_clock_load_the_same() {
    let saves = BatterySaves::in_storage(
        Arc::new(existing_files()),
        Path::new(ROM_PATH),
        Notices::default(),
    );
    let effects = BatteryCartridgeEffects::new(saves);

    assert_eq!(effects.load_ram(), Some(saved_ram()));
    let clock = effects.load_rtc_state().unwrap();
    assert_eq!(
        format!("{clock:?}"),
        format!("{:?}", saved_clock()),
        "the clock"
    );
}

#[test]
fn the_battery_save_and_clock_are_written_the_same() {
    let folder = temporary_folder("battery");
    let notices = Notices::default();
    let saves = BatterySaves::in_storage(
        Arc::new(Files::in_folders(None, Some(folder.clone()))),
        Path::new(ROM_PATH),
        notices.clone(),
    );
    let mut effects = BatteryCartridgeEffects::new(saves.clone());

    effects.save_ram(&saved_ram());
    effects.save_rtc_state(&saved_clock());
    saves.flush();

    assert!(notices.take().is_empty());
    for file in ["saves/Pocket Game.sav", "saves/Pocket Game.rtc"] {
        assert_eq!(
            fs::read(folder.join(file)).unwrap(),
            fs::read(existing().join("data").join(file)).unwrap(),
            "{file}"
        );
    }
}

#[test]
fn the_cheats_read_and_write_the_same() {
    let folder = temporary_folder("cheats");
    let files = Files::in_folders(None, Some(folder.clone()));
    let rom_path = Path::new(ROM_PATH);

    assert_eq!(
        cheats::load_from(&existing_files(), rom_path).unwrap(),
        saved_cheats()
    );

    cheats::save_to(&files, rom_path, &saved_cheats()).unwrap();
    let file = "cheats/Pocket Game.gbc.toml";
    assert_eq!(
        fs::read(folder.join(file)).unwrap(),
        fs::read(existing().join("data").join(file)).unwrap()
    );
}

#[test]
fn the_save_states_and_their_pictures_load_the_same() {
    let states = existing().join("data/states");
    let rom_path = Path::new(ROM_PATH);

    let quick = save_states::load_in(&states, rom_path, Slot::Quick)
        .unwrap()
        .unwrap();
    assert_eq!(quick.model, Model::Dmg);
    Emulator::new(Model::Dmg).load_state(&quick.data).unwrap();
    let thumbnail = save_states::thumbnail_in(&states, rom_path, Slot::Quick).unwrap();
    assert_eq!((thumbnail.width(), thumbnail.height()), (160, 144));
    assert_eq!(thumbnail.pixels(), &saved_screen()[..]);
    assert!(
        save_states::slot_info_in(&states, rom_path, Slot::Quick)
            .saved_at
            .is_some()
    );

    let numbered = save_states::load_in(&states, rom_path, Slot::Numbered(2)).unwrap();
    assert_eq!(numbered.unwrap().data, quick.data);
    assert!(save_states::thumbnail_in(&states, rom_path, Slot::Numbered(2)).is_none());

    assert!(
        save_states::load_in(&states, rom_path, Slot::Numbered(1))
            .unwrap()
            .is_none()
    );
    assert!(
        save_states::slot_info_in(&states, rom_path, Slot::Resume)
            .saved_at
            .is_none()
    );
}

#[test]
fn a_save_state_and_its_picture_are_written_the_same() {
    let states = existing().join("data/states");
    let folder = temporary_folder("states");
    let rom_path = Path::new(ROM_PATH);
    let quick = save_states::load_in(&states, rom_path, Slot::Quick)
        .unwrap()
        .unwrap();

    let state = SaveState {
        model: quick.model,
        data: quick.data,
    };
    save_states::save_in(
        &folder,
        rom_path,
        Slot::Quick,
        &state,
        Some(&saved_screen()),
    )
    .unwrap();

    for file in ["Pocket Game.gbc/quick.state", "Pocket Game.gbc/quick.png"] {
        assert_eq!(
            fs::read(folder.join(file)).unwrap(),
            fs::read(states.join(file)).unwrap(),
            "{file}"
        );
    }
}
