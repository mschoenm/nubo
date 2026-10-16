//! Builds without the `desktop` or `web` feature, such as the RetroArch
//! core's, keep no library.

use super::{Cache, LibraryEntry, PictureKind};
use std::path::Path;

#[expect(clippy::unnecessary_wraps, reason = "matches the file store")]
pub fn load_entries(_cache: &Cache) -> Result<Vec<LibraryEntry>, String> {
    Ok(Vec::new())
}

#[expect(clippy::unnecessary_wraps, reason = "matches the file store")]
pub fn save_entries(_cache: &Cache, _entries: &[LibraryEntry]) -> Result<(), String> {
    Ok(())
}

#[expect(clippy::unnecessary_wraps, reason = "matches the file store")]
pub fn save_thumbnail(_cache: &Cache, _rom_path: &Path, _pixels: &[u8]) -> Result<(), String> {
    Ok(())
}

pub fn save_title_screen(_cache: &Cache, _rom_path: &Path, _pixels: &[u8]) -> Result<(), String> {
    Err(String::from("this build keeps no library"))
}

pub fn read_picture(
    _cache: &Cache,
    _kind: PictureKind,
    _rom_path: &Path,
) -> Option<(u32, u32, Vec<u8>)> {
    None
}

pub fn current_picture(_cache: &Cache, _kind: PictureKind, _rom_path: &Path) -> Option<Vec<u8>> {
    None
}

pub fn decode_png(_png: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    None
}

pub fn save_box_art(_cache: &Cache, _rom_path: &Path, _png: &[u8]) -> Result<(), String> {
    Err(String::from("this build keeps no library"))
}
