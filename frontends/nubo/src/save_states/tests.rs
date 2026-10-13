//! Tests for save state files. They write real files in the temporary
//! directory, so they only run in the native build.

use super::store::{keep_aside_in, load_in, save_in, slot_info_in, thumbnail_in};
use super::*;
use nubo_frontend::time::Duration;
use std::fs;
use std::path::PathBuf;
use std::process;

/// An empty folder in the temporary directory.
fn temporary_folder(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("nubo-states-{}-{}", name, process::id()));
    let _ = fs::remove_dir_all(&folder);
    folder
}

/// A real save state of `model`, made after `steps` instructions so states
/// made with different numbers differ.
fn state(model: Model, steps: u32) -> SaveState {
    let mut emulator = nubo_core::Emulator::new(model);
    for _ in 0..steps {
        emulator.step();
    }
    SaveState {
        model,
        data: emulator.save_state(),
    }
}

#[test]
fn a_saved_state_loads_with_its_model() {
    let directory = temporary_folder("round-trip");
    let rom = Path::new("roms/Other.gbc");
    let saved = state(Model::CgbE, 10);

    save_in(&directory, rom, Slot::Quick, &saved, None).unwrap();
    let loaded = load_in(&directory, rom, Slot::Quick).unwrap().unwrap();

    assert_eq!(loaded.model, Model::CgbE);
    assert_eq!(loaded.data, saved.data);
}

#[test]
fn empty_slots_load_nothing() {
    let directory = temporary_folder("empty");
    let rom = Path::new("roms/Example.gb");

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
    let example = Path::new("Example.gb");
    let other = Path::new("Other.gb");

    save_in(
        &directory,
        example,
        Slot::Numbered(1),
        &state(Model::Dmg, 1),
        None,
    )
    .unwrap();
    save_in(
        &directory,
        example,
        Slot::Numbered(2),
        &state(Model::Mgb, 2),
        None,
    )
    .unwrap();

    assert_eq!(
        load_in(&directory, example, Slot::Numbered(1))
            .unwrap()
            .unwrap()
            .model,
        Model::Dmg
    );
    assert_eq!(
        load_in(&directory, example, Slot::Numbered(2))
            .unwrap()
            .unwrap()
            .data,
        state(Model::Mgb, 2).data
    );
    assert!(
        load_in(&directory, other, Slot::Numbered(1))
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_slot_with_a_picture_reports_it() {
    let directory = temporary_folder("picture");
    let rom = Path::new("Example.gb");
    let pixels = vec![0xFF; 160 * 144 * 4];

    save_in(
        &directory,
        rom,
        Slot::Resume,
        &state(Model::Dmg, 0),
        Some(&pixels),
    )
    .unwrap();
    let info = slot_info_in(&directory, rom, Slot::Resume);

    assert!(info.saved_at.is_some());
    let thumbnail = thumbnail_in(&directory, rom, Slot::Resume).unwrap();
    assert_eq!((thumbnail.width(), thumbnail.height()), (160, 144));
    assert_eq!(thumbnail.pixels(), &pixels[..]);
}

#[test]
fn other_files_are_rejected() {
    let directory = temporary_folder("invalid");
    let rom = Path::new("Example.gb");
    save_in(&directory, rom, Slot::Quick, &state(Model::Dmg, 0), None).unwrap();
    let path = directory.join("Example.gb").join("quick.state");
    fs::write(&path, b"not a state").unwrap();
    assert_eq!(
        load_in(&directory, rom, Slot::Quick).unwrap_err(),
        "the file isn't a Nubo save state"
    );

    fs::write(&path, b"Nubo state DMG\nHEADER\x1b").unwrap();
    assert!(
        load_in(&directory, rom, Slot::Quick)
            .unwrap_err()
            .contains("Nubo 0.3.0 or earlier")
    );
}

#[test]
fn a_state_file_is_the_cores_save_state_as_it_is() {
    let directory = temporary_folder("plain");
    let rom = Path::new("Example.gb");
    let saved = state(Model::CgbE, 3);
    save_in(&directory, rom, Slot::Quick, &saved, None).unwrap();
    assert_eq!(
        fs::read(directory.join("Example.gb").join("quick.state")).unwrap(),
        saved.data
    );
}

#[test]
fn a_state_kept_aside_survives_the_next_save_to_its_slot() {
    let directory = temporary_folder("kept");
    let rom = Path::new("roms/Example.gb");
    let folder = directory.join("Example.gb");
    let pixels = vec![0; 160 * 144 * 4];
    let (old, new) = (state(Model::Dmg, 1), state(Model::Dmg, 2));
    save_in(&directory, rom, Slot::Resume, &old, Some(&pixels)).unwrap();

    let kept = keep_aside_in(&directory, rom, Slot::Resume).unwrap();
    assert_eq!(kept, folder.join("resume.kept-1.state"));
    assert!(folder.join("resume.kept-1.png").exists());
    assert!(load_in(&directory, rom, Slot::Resume).unwrap().is_none());

    save_in(&directory, rom, Slot::Resume, &new, None).unwrap();
    assert_eq!(fs::read(&kept).unwrap(), old.data);

    let kept_again = keep_aside_in(&directory, rom, Slot::Resume).unwrap();
    assert_eq!(kept_again, folder.join("resume.kept-2.state"));
    assert_eq!(fs::read(&kept).unwrap(), old.data);
    assert_eq!(fs::read(&kept_again).unwrap(), new.data);
}

#[test]
fn keeping_an_empty_slot_aside_fails() {
    let directory = temporary_folder("kept-empty");
    assert!(keep_aside_in(&directory, Path::new("Example.gb"), Slot::Quick).is_err());
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
