//! Tests for save state files. They write real files in the temporary
//! directory, so they only run in the native build.

use super::store::{load_in, save_in, slot_info_in};
use super::*;
use iced::time::Duration;
use std::fs;
use std::path::PathBuf;
use std::process;

/// An empty folder in the temporary directory.
fn temporary_folder(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("nubo-states-{}-{}", name, process::id()));
    let _ = fs::remove_dir_all(&folder);
    folder
}

fn state(model: Model, data: &[u8]) -> SaveState {
    SaveState {
        model,
        data: data.to_vec(),
    }
}

#[test]
fn a_saved_state_loads_with_its_model() {
    let directory = temporary_folder("round-trip");
    let rom = Path::new("roms/Zelda.gbc");

    save_in(
        &directory,
        rom,
        Slot::Quick,
        &state(Model::CgbE, b"\nstate\n"),
        None,
    )
    .unwrap();
    let loaded = load_in(&directory, rom, Slot::Quick).unwrap().unwrap();

    assert_eq!(loaded.model, Model::CgbE);
    assert_eq!(loaded.data, b"\nstate\n");
}

#[test]
fn empty_slots_load_nothing() {
    let directory = temporary_folder("empty");
    let rom = Path::new("roms/Tetris.gb");

    assert!(
        load_in(&directory, rom, Slot::Numbered(2))
            .unwrap()
            .is_none()
    );
    assert!(
        slot_info_in(&directory, rom, Slot::Numbered(2))
            .saved_at
            .is_none()
    );
}

#[test]
fn slots_and_games_are_kept_apart() {
    let directory = temporary_folder("apart");
    let tetris = Path::new("Tetris.gb");
    let zelda = Path::new("Zelda.gb");

    save_in(
        &directory,
        tetris,
        Slot::Numbered(1),
        &state(Model::Dmg, b"one"),
        None,
    )
    .unwrap();
    save_in(
        &directory,
        tetris,
        Slot::Numbered(2),
        &state(Model::Mgb, b"two"),
        None,
    )
    .unwrap();

    assert_eq!(
        load_in(&directory, tetris, Slot::Numbered(1))
            .unwrap()
            .unwrap()
            .data,
        b"one"
    );
    assert_eq!(
        load_in(&directory, tetris, Slot::Numbered(2))
            .unwrap()
            .unwrap()
            .data,
        b"two"
    );
    assert!(
        load_in(&directory, zelda, Slot::Numbered(1))
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_slot_with_a_picture_reports_it() {
    let directory = temporary_folder("picture");
    let rom = Path::new("Tetris.gb");
    let pixels = vec![0xFF; 160 * 144 * 4];

    save_in(
        &directory,
        rom,
        Slot::Resume,
        &state(Model::Dmg, b"x"),
        Some(&pixels),
    )
    .unwrap();
    let info = slot_info_in(&directory, rom, Slot::Resume);

    assert!(info.saved_at.is_some());
    assert!(info.thumbnail.is_some());
}

#[test]
fn other_files_are_rejected() {
    let directory = temporary_folder("invalid");
    let rom = Path::new("Tetris.gb");
    save_in(&directory, rom, Slot::Quick, &state(Model::Dmg, b"x"), None).unwrap();
    let path = directory.join("Tetris.gb").join("quick.state");
    fs::write(path, b"not a state").unwrap();

    assert!(load_in(&directory, rom, Slot::Quick).is_err());
}

#[test]
fn time_ago_reads_naturally() {
    let now = SystemTime::now();

    assert_eq!(time_ago(now), "Just now");
    assert_eq!(time_ago(now - Duration::from_secs(60)), "1 minute ago");
    assert_eq!(
        time_ago(now - Duration::from_secs(3 * 3_600)),
        "3 hours ago"
    );
    assert_eq!(
        time_ago(now - Duration::from_secs(2 * 86_400)),
        "2 days ago"
    );
}
