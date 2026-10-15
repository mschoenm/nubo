//! Save states as files in `states/<rom file name>/` in the per-user data
//! folder (the `desktop` feature).
//!
//! A `.state` file is exactly the core's save state (`Emulator::save_state`),
//! whose header names the model it was made on, so there is one format to
//! keep loading in other versions of Nubo, the same on every platform.

use super::{SaveState, Slot, SlotInfo};
use crate::picture::{Picture, SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::storage::files::{data_folder, decode_png, encode_png, write_atomically};
use nubo_core::{SaveStateError, SaveStateHeader};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// What `.state` files of Nubo 0.3.0 and earlier started with, before the
/// core's save states had a format that later versions keep loading.
const EARLY_STATE_PREFIX: &[u8] = b"Nubo state ";

pub fn save(
    rom_path: &Path,
    slot: Slot,
    state: &SaveState,
    pixels: Option<&[u8]>,
) -> Result<(), String> {
    save_in(&states_directory()?, rom_path, slot, state, pixels)
}

pub fn load(rom_path: &Path, slot: Slot) -> Result<Option<SaveState>, String> {
    load_in(&states_directory()?, rom_path, slot)
}

pub fn slot_info(rom_path: &Path, slot: Slot) -> SlotInfo {
    match states_directory() {
        Ok(directory) => slot_info_in(&directory, rom_path, slot),
        Err(_) => SlotInfo {
            slot,
            saved_at: None,
            thumbnail: None,
        },
    }
}

/// Saves a state in `directory` (the folder for every game's states).
pub fn save_in(
    directory: &Path,
    rom_path: &Path,
    slot: Slot,
    state: &SaveState,
    pixels: Option<&[u8]>,
) -> Result<(), String> {
    let path = slot_path(directory, rom_path, slot, "state");
    write_atomically(&path, &state.data).map_err(|error| error.to_string())?;

    let picture = slot_path(directory, rom_path, slot, "png");
    match pixels {
        Some(pixels) => {
            write_atomically(&picture, &encode_png(SCREEN_WIDTH, SCREEN_HEIGHT, pixels)?)
                .map_err(|error| error.to_string())
        }
        None => match fs::remove_file(picture) {
            Err(error) if error.kind() != ErrorKind::NotFound => Err(error.to_string()),
            _ => Ok(()),
        },
    }
}

/// Reads a state from `directory` (the folder for every game's states).
pub fn load_in(directory: &Path, rom_path: &Path, slot: Slot) -> Result<Option<SaveState>, String> {
    let contents = match fs::read(slot_path(directory, rom_path, slot, "state")) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };

    if contents.starts_with(EARLY_STATE_PREFIX) {
        return Err(String::from(
            "the state was made by Nubo 0.3.0 or earlier, whose save states later versions \
             can't read",
        ));
    }
    let header = SaveStateHeader::read(&contents).map_err(|error| match error {
        SaveStateError::InvalidFormat => String::from("the file isn't a Nubo save state"),
        error => error.to_string(),
    })?;
    Ok(Some(SaveState {
        model: header.model,
        data: contents,
    }))
}

pub fn delete(rom_path: &Path, slot: Slot) -> Result<(), String> {
    delete_in(&states_directory()?, rom_path, slot)
}

/// Deletes a slot's state and picture in `directory` (the folder for every
/// game's states). An empty slot is fine.
pub fn delete_in(directory: &Path, rom_path: &Path, slot: Slot) -> Result<(), String> {
    for extension in ["state", "png"] {
        match fs::remove_file(slot_path(directory, rom_path, slot, extension)) {
            Err(error) if error.kind() != ErrorKind::NotFound => return Err(error.to_string()),
            _ => {}
        }
    }
    Ok(())
}

pub fn delete_all() -> Result<(), String> {
    delete_all_in(&states_directory()?)
}

/// Deletes `directory` (the folder for every game's states) and everything
/// in it. A folder that was never made is fine.
pub fn delete_all_in(directory: &Path) -> Result<(), String> {
    match fs::remove_dir_all(directory) {
        Err(error) if error.kind() != ErrorKind::NotFound => Err(error.to_string()),
        _ => Ok(()),
    }
}

pub fn keep_aside(rom_path: &Path, slot: Slot) -> Result<PathBuf, String> {
    keep_aside_in(&states_directory()?, rom_path, slot)
}

/// Moves a slot's state and picture in `directory` (the folder for every
/// game's states) to the first free `<slot>.kept-<number>` name, so saving to
/// the slot again can't overwrite them. Returns where the state went, once the
/// state has moved.
pub fn keep_aside_in(directory: &Path, rom_path: &Path, slot: Slot) -> Result<PathBuf, String> {
    let state = slot_path(directory, rom_path, slot, "state");
    let mut number = 1;
    let kept = loop {
        let kept = state.with_file_name(format!("{}.kept-{number}.state", file_stem(slot)));
        if !kept.exists() {
            break kept;
        }
        number += 1;
    };
    fs::rename(&state, &kept).map_err(|error| error.to_string())?;

    // The picture only shows in the menu next to a state, so the state counts
    // as kept even if it can't move (it may be locked on Windows): the empty
    // slot doesn't show it, and the next save to the slot replaces it.
    let picture = slot_path(directory, rom_path, slot, "png");
    let _ = fs::rename(picture, kept.with_extension("png"));
    Ok(kept)
}

/// Describes a slot in `directory` (the folder for every game's states),
/// without its picture, which [`thumbnail_in`] reads.
pub fn slot_info_in(directory: &Path, rom_path: &Path, slot: Slot) -> SlotInfo {
    let saved_at = fs::metadata(slot_path(directory, rom_path, slot, "state"))
        .and_then(|metadata| metadata.modified())
        .ok();

    SlotInfo {
        slot,
        saved_at,
        thumbnail: None,
    }
}

pub fn thumbnail(rom_path: &Path, slot: Slot) -> Option<Picture> {
    thumbnail_in(&states_directory().ok()?, rom_path, slot)
}

/// Reads and decodes the picture of a slot in `directory`, anew each time,
/// so a picture replaced by a new save shows up.
pub fn thumbnail_in(directory: &Path, rom_path: &Path, slot: Slot) -> Option<Picture> {
    let png = fs::read(slot_path(directory, rom_path, slot, "png")).ok()?;
    let (width, height, pixels) = decode_png(&png)?;
    Some(Picture::new(width, height, pixels))
}

/// The file of a slot, in a folder named after the ROM's file name.
fn slot_path(directory: &Path, rom_path: &Path, slot: Slot, extension: &str) -> PathBuf {
    directory
        .join(rom_path.file_name().unwrap_or_default())
        .join(file_stem(slot))
        .with_extension(extension)
}

/// The name of a slot's files, without extension.
fn file_stem(slot: Slot) -> String {
    match slot {
        Slot::Resume => String::from("resume"),
        Slot::Quick => String::from("quick"),
        Slot::Numbered(number) => format!("slot-{number}"),
    }
}

fn states_directory() -> Result<PathBuf, String> {
    data_folder()
        .map(|folder| folder.join("states"))
        .ok_or_else(|| String::from("the system has no data folder for us"))
}
