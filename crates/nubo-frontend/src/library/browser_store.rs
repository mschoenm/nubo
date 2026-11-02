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
use crate::storage::{Unreadable, indexed_db, kept_name};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::path::Path;

/// The entry that lists the known games.
const LIBRARY: &str = "library.toml";

/// The contents of `library.toml`. Saving borrows the games, so they
/// aren't copied first.
#[derive(Serialize, Deserialize)]
struct LibraryFile<'a> {
    games: Cow<'a, [LibraryEntry]>,
}

/// Reads the known games, or none if there is no library entry yet. An
/// entry that can't be parsed is kept aside first, under the first free
/// `library.kept-<number>.toml`, and the error says where it went (or that
/// saving over it isn't safe, when that failed).
pub fn load_entries(_cache: &Cache) -> Result<Vec<LibraryEntry>, Unreadable> {
    let Some(record) = indexed_db::read(LIBRARY) else {
        return Ok(Vec::new());
    };
    std::str::from_utf8(&record.bytes)
        .map_err(|error| error.to_string())
        .and_then(|contents| {
            toml::from_str::<LibraryFile>(contents).map_err(|error| error.to_string())
        })
        .map(|file| file.games.into_owned())
        .map_err(|error| super::unreadable_library(&error, Some(keep_aside())))
}

/// Gives the library's entry the first free `library.kept-<number>.toml`
/// name, so the next save can't replace it, and returns that name.
fn keep_aside() -> Result<String, String> {
    let kept = (1..)
        .map(|number| kept_name(LIBRARY, number))
        .find(|kept| indexed_db::read(kept).is_none())
        .unwrap_or_default();
    if indexed_db::rename(LIBRARY, &kept)? {
        Ok(kept)
    } else {
        Err(String::from("there is nothing to keep"))
    }
}

/// Writes the known games to `library.toml`.
pub fn save_entries(_cache: &Cache, entries: &[LibraryEntry]) -> Result<(), String> {
    let file = LibraryFile {
        games: Cow::Borrowed(entries),
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

/// The saved picture kept under `pictures_as`'s name as a PNG, if there is
/// one; the browser's games go by their own path (`Library::pictures_as`).
/// Adding another version under the same name (another size; a game with
/// another header title is kept beside it, see `Library::add_game`)
/// deletes its pictures (see [`delete_pictures`]), so any that is left is of
/// the file kept now.
pub fn current_picture(
    _cache: &Cache,
    kind: PictureKind,
    pictures_as: &Path,
    _rom_path: &Path,
) -> Option<Vec<u8>> {
    Some(
        indexed_db::read(&picture_key(kind, pictures_as))?
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

/// Whether the file of the game at `rom_path` is kept: saved, and not
/// refused by the browser.
pub fn has_rom(rom_path: &Path) -> bool {
    indexed_db::has_rom(&rom_key(rom_path))
}

/// Deletes the kept file of the game at `rom_path`.
pub fn delete_rom(rom_path: &Path) -> Result<(), String> {
    indexed_db::remove_rom(&rom_key(rom_path))
}

/// How many bytes the kept games' files and the library's pictures take,
/// roughly: the files' sizes as the library knows them, the pictures and the
/// box art game lists (kept under `box_art/lists/`).
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
