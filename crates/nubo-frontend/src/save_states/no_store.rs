//! Builds without the `desktop` or `web` feature, such as the RetroArch
//! core's, keep no save states.

use super::{SaveState, Slot, SlotInfo};
use crate::picture::Picture;
use std::path::{Path, PathBuf};

pub fn save(
    _rom_path: &Path,
    _slot: Slot,
    _state: &SaveState,
    _pixels: Option<&[u8]>,
) -> Result<(), String> {
    Err(String::from("this build keeps no save states"))
}

pub fn save_png(
    _rom_path: &Path,
    _slot: Slot,
    _state: &SaveState,
    _png: Option<&[u8]>,
) -> Result<(), String> {
    Err(String::from("this build keeps no save states"))
}

#[expect(clippy::unnecessary_wraps, reason = "matches the file store")]
pub fn load(_rom_path: &Path, _slot: Slot) -> Result<Option<SaveState>, String> {
    Ok(None)
}

pub fn delete(_rom_path: &Path, _slot: Slot) -> Result<(), String> {
    Err(String::from("this build keeps no save states"))
}

#[expect(clippy::unnecessary_wraps, reason = "matches the file store")]
pub fn delete_all() -> Result<(), String> {
    Ok(())
}

pub fn keep_aside(_rom_path: &Path, _slot: Slot) -> Result<PathBuf, String> {
    Err(String::from("this build keeps no save states"))
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

pub fn picture_png(_rom_path: &Path, _slot: Slot) -> Option<Vec<u8>> {
    None
}
