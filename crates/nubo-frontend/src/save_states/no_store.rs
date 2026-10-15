//! Builds without the `desktop` feature, such as the browser build, keep no
//! save states yet.

use super::{SaveState, Slot, SlotInfo};
use crate::picture::Picture;
use std::path::{Path, PathBuf};

pub fn save(
    _rom_path: &Path,
    _slot: Slot,
    _state: &SaveState,
    _pixels: Option<&[u8]>,
) -> Result<(), String> {
    Err(String::from("save states aren't available in the browser"))
}

#[expect(clippy::unnecessary_wraps, reason = "matches the file store")]
pub fn load(_rom_path: &Path, _slot: Slot) -> Result<Option<SaveState>, String> {
    Ok(None)
}

pub fn delete(_rom_path: &Path, _slot: Slot) -> Result<(), String> {
    Err(String::from("save states aren't available in the browser"))
}

pub fn keep_aside(_rom_path: &Path, _slot: Slot) -> Result<PathBuf, String> {
    Err(String::from("save states aren't available in the browser"))
}

pub fn slot_info(_rom_path: &Path, slot: Slot) -> SlotInfo {
    SlotInfo {
        slot,
        saved_at: None,
        thumbnail: None,
    }
}

pub fn thumbnail(_rom_path: &Path, _slot: Slot) -> Option<Picture> {
    None
}
