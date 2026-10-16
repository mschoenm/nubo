//! The library in the browser's IndexedDB (the `web` feature), as entries
//! named like the desktop's files: `library.toml` with the known games, and
//! PNGs of each game's last screen played in `thumbnails/`, title screen in
//! `title_screens/` and box art in `box_art/`, such as
//! `thumbnails/Tetris.gb.png` (see `storage::indexed_db`). The browser has
//! no folders to scan, so the library keeps each game's file too, in the
//! database's `roms` store under its file name.

pub use crate::picture::decode_png;

use super::{Cache, LibraryEntry, PictureKind};
use crate::picture::{SCREEN_HEIGHT, SCREEN_WIDTH, encode_png};
use crate::storage::indexed_db;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The entry that lists the known games.
const LIBRARY: &str = "library.toml";

/// The contents of `library.toml`.
#[derive(Serialize, Deserialize)]
struct LibraryFile {
    games: Vec<LibraryEntry>,
}

/// Reads the known games, or none if there is no library entry yet.
pub fn load_entries(_cache: &Cache) -> Result<Vec<LibraryEntry>, String> {
    let Some(record) = indexed_db::read(LIBRARY) else {
        return Ok(Vec::new());
    };
    let contents = std::str::from_utf8(&record.bytes).map_err(|error| error.to_string())?;
    toml::from_str::<LibraryFile>(contents)
        .map(|file| file.games)
        .map_err(|error| error.to_string())
}

/// Writes the known games to `library.toml`.
pub fn save_entries(_cache: &Cache, entries: &[LibraryEntry]) -> Result<(), String> {
    let file = LibraryFile {
        games: entries.to_vec(),
    };
    let contents = toml::to_string(&file).map_err(|error| error.to_string())?;
    indexed_db::write(LIBRARY, contents.as_bytes())
}

/// Saves a frame of the game at `rom_path` as its thumbnail.
pub fn save_thumbnail(_cache: &Cache, rom_path: &Path, pixels: &[u8]) -> Result<(), String> {
    indexed_db::write(
        &picture_key(PictureKind::LastScreen, rom_path),
        &encode_png(SCREEN_WIDTH, SCREEN_HEIGHT, pixels)?,
    )
    .map_err(|error| format!("The game's thumbnail couldn't be saved: {error}"))
}

/// Saves the title screen of the game at `rom_path`, as the emulator drew
/// it (in gray for original Game Boy games).
pub fn save_title_screen(_cache: &Cache, rom_path: &Path, pixels: &[u8]) -> Result<(), String> {
    indexed_db::write(
        &picture_key(PictureKind::TitleScreen, rom_path),
        &encode_png(SCREEN_WIDTH, SCREEN_HEIGHT, pixels)?,
    )
    .map_err(|error| format!("A title screen couldn't be saved: {error}"))
}

/// Reads a saved picture of the game at `rom_path` as its width, height
/// and RGBA pixels, or `None` if there is none.
pub fn read_picture(
    _cache: &Cache,
    kind: PictureKind,
    rom_path: &Path,
) -> Option<(u32, u32, Vec<u8>)> {
    decode_png(&indexed_db::read(&picture_key(kind, rom_path))?.bytes)
}

/// The saved picture of the game at `rom_path` as a PNG, if there is one.
/// Adding a game's file again deletes its pictures (see
/// [`delete_pictures`]), so any that is left is of the file kept now.
pub fn current_picture(_cache: &Cache, kind: PictureKind, rom_path: &Path) -> Option<Vec<u8>> {
    Some(
        indexed_db::read(&picture_key(kind, rom_path))?
            .bytes
            .to_vec(),
    )
}

/// Saves the box art (a PNG) of the game at `rom_path`.
pub fn save_box_art(_cache: &Cache, rom_path: &Path, png: &[u8]) -> Result<(), String> {
    indexed_db::write(&picture_key(PictureKind::BoxArt, rom_path), png)
        .map_err(|error| format!("Box art couldn't be saved: {error}"))
}

/// Deletes every picture of the game at `rom_path`.
pub fn delete_pictures(rom_path: &Path) -> Result<(), String> {
    for kind in [
        PictureKind::BoxArt,
        PictureKind::TitleScreen,
        PictureKind::LastScreen,
    ] {
        indexed_db::remove(&picture_key(kind, rom_path))?;
    }
    Ok(())
}

/// Keeps `bytes` as the file of the game at `rom_path`.
pub fn save_rom(rom_path: &Path, bytes: &[u8]) -> Result<(), String> {
    indexed_db::write_rom(&rom_key(rom_path), bytes)
}

/// Deletes the kept file of the game at `rom_path`.
pub fn delete_rom(rom_path: &Path) -> Result<(), String> {
    indexed_db::remove_rom(&rom_key(rom_path))
}

/// How many bytes the kept games' files and the library's pictures take,
/// roughly: the files' sizes as the library knows them, and the pictures.
pub fn size(entries: &[LibraryEntry]) -> u64 {
    let files: u64 = entries
        .iter()
        .filter(|entry| entry.in_folder)
        .map(|entry| entry.size)
        .sum();
    files
        + ["thumbnails/", "title_screens/", "box_art/"]
            .into_iter()
            .map(indexed_db::size)
            .sum::<u64>()
}

/// The key a game's file is kept under in the `roms` store: its file name.
pub fn rom_key(rom_path: &Path) -> String {
    rom_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// The entry a picture of `kind` of the game at `rom_path` is kept as, named
/// after the ROM's file name, such as `thumbnails/Tetris.gb.png`.
fn picture_key(kind: PictureKind, rom_path: &Path) -> String {
    let folder = match kind {
        PictureKind::BoxArt => "box_art",
        PictureKind::TitleScreen => "title_screens",
        PictureKind::LastScreen => "thumbnails",
    };
    format!("{folder}/{}.png", rom_key(rom_path))
}

#[cfg(test)]
mod tests;
