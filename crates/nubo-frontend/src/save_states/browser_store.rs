//! Save states in the browser's IndexedDB (the `web` feature), as entries
//! named like the desktop's files: `states/<rom file name>/<slot>.state`
//! with the core's save state and `<slot>.png` with the screen (see
//! `storage::indexed_db`).

use super::{SaveState, Slot, SlotInfo, file_stem};
use crate::picture::{Picture, SCREEN_HEIGHT, SCREEN_WIDTH, decode_png, encode_png};
use crate::storage::indexed_db;
use std::path::{Path, PathBuf};

/// The start of every save state entry's name.
const STATES: &str = "states/";

/// Saves a state in a slot, with the screen as its picture, or without a
/// picture (removing an older one) when there are no `pixels`.
pub fn save(
    rom_path: &Path,
    slot: Slot,
    state: &SaveState,
    pixels: Option<&[u8]>,
) -> Result<(), String> {
    indexed_db::write(&slot_key(rom_path, slot, "state"), &state.data)?;

    let picture = slot_key(rom_path, slot, "png");
    match pixels {
        Some(pixels) => {
            indexed_db::write(&picture, &encode_png(SCREEN_WIDTH, SCREEN_HEIGHT, pixels)?)
        }
        None => indexed_db::remove(&picture),
    }
}

/// Reads a slot's state; `None` when the slot is empty.
pub fn load(rom_path: &Path, slot: Slot) -> Result<Option<SaveState>, String> {
    indexed_db::read(&slot_key(rom_path, slot, "state"))
        .map(|record| super::parse(record.bytes.to_vec()))
        .transpose()
}

/// Deletes a slot's state and picture. An empty slot is fine.
pub fn delete(rom_path: &Path, slot: Slot) -> Result<(), String> {
    for extension in ["state", "png"] {
        indexed_db::remove(&slot_key(rom_path, slot, extension))?;
    }
    Ok(())
}

/// Deletes every game's states and pictures.
pub fn delete_all() -> Result<(), String> {
    indexed_db::remove_all(STATES)
}

/// Gives a slot's state and picture the first free `<slot>.kept-<number>`
/// name, so saving to the slot again can't overwrite them, and returns the
/// state's new name.
pub fn keep_aside(rom_path: &Path, slot: Slot) -> Result<PathBuf, String> {
    let folder = game_folder(rom_path);
    let stem = file_stem(slot);
    let kept = (1..)
        .map(|number| format!("{folder}{stem}.kept-{number}"))
        .find(|kept| indexed_db::read(&format!("{kept}.state")).is_none())
        .unwrap_or_default();

    if !indexed_db::rename(&slot_key(rom_path, slot, "state"), &format!("{kept}.state"))? {
        return Err(String::from("the slot is empty"));
    }
    indexed_db::rename(&slot_key(rom_path, slot, "png"), &format!("{kept}.png"))?;
    Ok(PathBuf::from(format!("{kept}.state")))
}

/// Describes a slot: when its state was saved, if it has one, and no
/// thumbnail (see [`thumbnail`]).
pub fn slot_info(rom_path: &Path, slot: Slot) -> SlotInfo {
    SlotInfo {
        slot,
        saved_at: indexed_db::read(&slot_key(rom_path, slot, "state"))
            .map(|record| record.modified),
        thumbnail: None,
    }
}

/// The picture saved with a slot's state, if it has one that decodes.
pub fn thumbnail(rom_path: &Path, slot: Slot) -> Option<Picture> {
    let png = indexed_db::read(&slot_key(rom_path, slot, "png"))?;
    let (width, height, pixels) = decode_png(&png.bytes)?;
    Some(Picture::new(width, height, pixels))
}

/// The start of the names of a game's entries, named after the ROM's file
/// name, such as `states/Tetris.gb/`.
fn game_folder(rom_path: &Path) -> String {
    format!(
        "{STATES}{}/",
        rom_path.file_name().unwrap_or_default().to_string_lossy()
    )
}

/// The name of a slot's entry with `extension`.
fn slot_key(rom_path: &Path, slot: Slot, extension: &str) -> String {
    format!("{}{}.{extension}", game_folder(rom_path), file_stem(slot))
}

#[cfg(test)]
mod tests;
