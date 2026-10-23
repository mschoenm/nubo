//! Tests for save states in the browser's storage, which keeps its entries
//! in memory outside a browser (see `storage::indexed_db`), one set per test
//! thread.

use super::*;
use crate::storage::indexed_db;
use nubo_core::Model;

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
fn entries_are_named_like_the_desktops_files() {
    let rom = Path::new("Pocket Game.gbc");

    assert_eq!(
        slot_key(rom, Slot::Quick, "state"),
        "states/Pocket Game.gbc/quick.state"
    );
    assert_eq!(
        slot_key(rom, Slot::Numbered(2), "png"),
        "states/Pocket Game.gbc/slot-2.png"
    );
    assert_eq!(
        slot_key(rom, Slot::Resume, "state"),
        "states/Pocket Game.gbc/resume.state"
    );
}

#[test]
fn a_saved_state_loads_with_its_model_and_picture() {
    let rom = Path::new("Other.gbc");
    let saved = state(Model::CgbE, 10);
    let pixels = vec![200; (SCREEN_WIDTH * SCREEN_HEIGHT * 4) as usize];

    save(rom, Slot::Quick, &saved, Some(&pixels)).unwrap();
    let loaded = load(rom, Slot::Quick).unwrap().unwrap();

    assert_eq!(loaded.model, Model::CgbE);
    assert_eq!(loaded.data, saved.data);
    assert!(slot_info(rom, Slot::Quick).saved_at.is_some());
    assert_eq!(thumbnail(rom, Slot::Quick).unwrap().pixels(), &pixels[..]);
}

#[test]
fn a_picture_entry_is_read_and_written_back_as_it_is() {
    let rom = Path::new("Picture.gb");
    let png = b"any picture entry".to_vec();

    save_png(rom, Slot::Numbered(1), &state(Model::Dmg, 3), Some(&png)).unwrap();

    assert_eq!(
        indexed_db::read("states/Picture.gb/slot-1.png")
            .unwrap()
            .bytes
            .to_vec(),
        png
    );
    assert_eq!(picture_png(rom, Slot::Numbered(1)), Some(png));
    save_png(rom, Slot::Numbered(1), &state(Model::Dmg, 3), None).unwrap();
    assert_eq!(picture_png(rom, Slot::Numbered(1)), None);
}

#[test]
fn empty_slots_load_nothing() {
    let rom = Path::new("Example.gb");

    assert!(load(rom, Slot::Numbered(3)).unwrap().is_none());
    assert!(slot_info(rom, Slot::Numbered(3)).saved_at.is_none());
    assert!(thumbnail(rom, Slot::Numbered(3)).is_none());
}

#[test]
fn something_else_in_a_slot_says_so() {
    let rom = Path::new("Example.gb");
    indexed_db::write("states/Example.gb/quick.state", b"not a state").unwrap();

    let error = load(rom, Slot::Quick).unwrap_err();

    assert_eq!(error, "the file isn't a Nubo save state");
}

#[test]
fn a_state_kept_aside_frees_its_slot() {
    let rom = Path::new("Example.gb");
    save(rom, Slot::Quick, &state(Model::Dmg, 1), None).unwrap();

    let first = keep_aside(rom, Slot::Quick).unwrap();
    save(rom, Slot::Quick, &state(Model::Dmg, 2), None).unwrap();
    let second = keep_aside(rom, Slot::Quick).unwrap();

    assert_eq!(first, Path::new("states/Example.gb/quick.kept-1.state"));
    assert_eq!(second, Path::new("states/Example.gb/quick.kept-2.state"));
    assert!(load(rom, Slot::Quick).unwrap().is_none());
    assert!(keep_aside(rom, Slot::Quick).is_err());
}

#[test]
fn deleting_empties_the_slot_and_delete_all_empties_every_game() {
    let example = Path::new("Example.gb");
    let other = Path::new("Other.gb");
    save(example, Slot::Numbered(1), &state(Model::Dmg, 1), None).unwrap();
    save(example, Slot::Numbered(2), &state(Model::Dmg, 2), None).unwrap();
    save(other, Slot::Quick, &state(Model::Dmg, 3), None).unwrap();

    delete(example, Slot::Numbered(1)).unwrap();

    assert!(load(example, Slot::Numbered(1)).unwrap().is_none());
    assert!(load(example, Slot::Numbered(2)).unwrap().is_some());

    delete_all().unwrap();

    assert!(load(example, Slot::Numbered(2)).unwrap().is_none());
    assert!(load(other, Slot::Quick).unwrap().is_none());
}
