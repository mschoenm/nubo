//! Save states as files in `states/<rom file name>/` in the per-user data
//! folder (native build).
//!
//! A `.state` file starts with a line naming the format and the model, such
//! as `Nubo state CGB-E`, followed by the emulator's own save state.

use super::{SaveState, Slot, SlotInfo};
use crate::storage::files::{encode_png, project_directories, write_atomically};
use iced::widget::image;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

const MAGIC: &str = "Nubo state ";

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
    let mut contents = format!("{MAGIC}{}\n", state.model.name()).into_bytes();
    contents.extend_from_slice(&state.data);
    write_atomically(&path, &contents).map_err(|error| error.to_string())?;

    let picture = slot_path(directory, rom_path, slot, "png");
    match pixels {
        Some(pixels) => {
            write_atomically(&picture, &encode_png(pixels)?).map_err(|error| error.to_string())
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

    let invalid = || String::from("the file isn't a Nubo save state");
    let line_end = contents
        .iter()
        .position(|&byte| byte == b'\n')
        .ok_or_else(invalid)?;
    let first_line = std::str::from_utf8(&contents[..line_end]).map_err(|_| invalid())?;
    let model = first_line
        .strip_prefix(MAGIC)
        .ok_or_else(invalid)?
        .parse()?;

    Ok(Some(SaveState {
        model,
        data: contents[line_end + 1..].to_vec(),
    }))
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

/// Describes a slot in `directory` (the folder for every game's states).
pub fn slot_info_in(directory: &Path, rom_path: &Path, slot: Slot) -> SlotInfo {
    let saved_at = fs::metadata(slot_path(directory, rom_path, slot, "state"))
        .and_then(|metadata| metadata.modified())
        .ok();
    // Read as bytes so a picture replaced by a new save shows up; a handle
    // made from a path would keep showing the cached old picture.
    let thumbnail = saved_at
        .and_then(|_| fs::read(slot_path(directory, rom_path, slot, "png")).ok())
        .map(image::Handle::from_bytes);

    SlotInfo {
        slot,
        saved_at,
        thumbnail,
    }
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
    project_directories()
        .map(|directories| directories.data_dir().join("states"))
        .ok_or_else(|| String::from("the system has no data folder for us"))
}
