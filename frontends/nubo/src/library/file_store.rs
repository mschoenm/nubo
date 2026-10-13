//! The library's files in the per-user data folder (native build):
//! `library.toml` with the known games, a PNG of the last screen played per
//! game in `thumbnails`, a PNG of each title screen in `title_screens`, and
//! downloaded box art in `box_art`.

pub use crate::storage::files::decode_png;

use super::{LibraryEntry, PictureKind};
use crate::storage::files::{encode_png, project_directories, write_atomically};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// The contents of `library.toml`.
#[derive(Serialize, Deserialize)]
struct LibraryFile {
    games: Vec<LibraryEntry>,
}

/// Reads the known games, or none if there is no library file yet.
pub fn load_entries() -> Result<Vec<LibraryEntry>, String> {
    let path = data_directory()?.join("library.toml");

    match fs::read_to_string(path) {
        Ok(contents) => toml::from_str::<LibraryFile>(&contents)
            .map(|file| file.games)
            .map_err(|error| error.to_string()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.to_string()),
    }
}

pub fn save_entries(entries: &[LibraryEntry]) -> Result<(), String> {
    let file = LibraryFile {
        games: entries.to_vec(),
    };
    let contents = toml::to_string(&file).map_err(|error| error.to_string())?;

    write_atomically(&data_directory()?.join("library.toml"), contents.as_bytes())
        .map_err(|error| error.to_string())
}

/// Saves a frame of the game at `rom_path` as its thumbnail.
pub fn save_thumbnail(rom_path: &Path, pixels: &[u8]) -> Result<(), String> {
    let png = encode_png(pixels)?;

    write_atomically(&thumbnail_path(rom_path)?, &png)
        .map_err(|error| format!("The game's thumbnail couldn't be saved: {error}"))
}

/// Saves the title screen of the game at `rom_path`, as the emulator drew
/// it (in gray for original Game Boy games).
pub fn save_title_screen(rom_path: &Path, pixels: &[u8]) -> Result<(), String> {
    write_atomically(&title_screen_path(rom_path)?, &encode_png(pixels)?)
        .map_err(|error| format!("A title screen couldn't be saved: {error}"))
}

/// Reads a saved picture of the game at `rom_path` as its width, height
/// and RGBA pixels, or `None` if there is none.
pub fn read_picture(kind: PictureKind, rom_path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    decode_png(&fs::read(picture_path(kind, rom_path).ok()?).ok()?)
}

fn picture_path(kind: PictureKind, rom_path: &Path) -> Result<PathBuf, String> {
    match kind {
        PictureKind::BoxArt => box_art_path(rom_path),
        PictureKind::TitleScreen => title_screen_path(rom_path),
        PictureKind::LastScreen => thumbnail_path(rom_path),
    }
}

/// The saved picture of the game at `rom_path` as a PNG, if there is one
/// made after the ROM file last changed. A picture of an older version of
/// the file is ignored, so it is made again.
pub fn current_picture(kind: PictureKind, rom_path: &Path) -> Option<Vec<u8>> {
    let path = picture_path(kind, rom_path).ok()?;
    let modified = |path: &Path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
    };

    if modified(&path)? < modified(rom_path)? {
        return None;
    }
    fs::read(path).ok()
}

/// Saves the box art (a PNG) of the game at `rom_path`.
pub fn save_box_art(rom_path: &Path, png: &[u8]) -> Result<(), String> {
    write_atomically(&box_art_path(rom_path)?, png)
        .map_err(|error| format!("Box art couldn't be saved: {error}"))
}

/// Box art is named like thumbnails, in the `box_art` folder.
fn box_art_path(rom_path: &Path) -> Result<PathBuf, String> {
    let mut file_name = rom_path.file_name().unwrap_or_default().to_os_string();
    file_name.push(".png");
    Ok(data_directory()?.join("box_art").join(file_name))
}

/// Title screens are named like thumbnails, in the `title_screens` folder.
fn title_screen_path(rom_path: &Path) -> Result<PathBuf, String> {
    let mut file_name = rom_path.file_name().unwrap_or_default().to_os_string();
    file_name.push(".png");
    Ok(data_directory()?.join("title_screens").join(file_name))
}

/// Thumbnails are named after the ROM's file name, like battery saves, such
/// as `Tetris.gb.png`.
fn thumbnail_path(rom_path: &Path) -> Result<PathBuf, String> {
    let mut file_name = rom_path.file_name().unwrap_or_default().to_os_string();
    file_name.push(".png");
    Ok(data_directory()?.join("thumbnails").join(file_name))
}

fn data_directory() -> Result<PathBuf, String> {
    project_directories()
        .map(|directories| directories.data_dir().to_path_buf())
        .ok_or_else(|| String::from("the system has no data folder for us"))
}
