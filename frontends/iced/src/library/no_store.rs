//! The browser build keeps no library, since it can't scan folders.

use super::{LibraryEntry, PictureKind};
use std::path::Path;

#[expect(clippy::unnecessary_wraps, reason = "matches the file store")]
pub fn load_entries() -> Result<Vec<LibraryEntry>, String> {
    Ok(Vec::new())
}

#[expect(clippy::unnecessary_wraps, reason = "matches the file store")]
pub fn save_entries(_entries: &[LibraryEntry]) -> Result<(), String> {
    Ok(())
}

#[expect(clippy::unnecessary_wraps, reason = "matches the file store")]
pub fn save_thumbnail(_rom_path: &Path, _pixels: &[u8]) -> Result<(), String> {
    Ok(())
}

pub fn save_title_screen(_rom_path: &Path, _pixels: &[u8]) -> Result<(), String> {
    Err(String::from(
        "title screens aren't available in the browser",
    ))
}

pub fn read_picture(_kind: PictureKind, _rom_path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    None
}

pub fn current_picture(_kind: PictureKind, _rom_path: &Path) -> Option<Vec<u8>> {
    None
}

pub fn decode_png(_png: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    None
}

pub fn save_box_art(_rom_path: &Path, _png: &[u8]) -> Result<(), String> {
    Err(String::from("box art isn't available in the browser"))
}
