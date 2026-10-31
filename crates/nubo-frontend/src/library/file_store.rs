//! The library's files in the cache's data folder (the `desktop` feature):
//! `library.toml` with the known games, a PNG of the last screen played per
//! game in `thumbnails`, a PNG of each title screen in `title_screens`, and
//! downloaded box art in `box_art`.

pub use crate::picture::decode_png;

use super::{Cache, LibraryEntry, PictureKind};
use crate::picture::{SCREEN_HEIGHT, SCREEN_WIDTH, encode_png};
use crate::storage::Unreadable;
use crate::storage::files::{keep_aside, write_atomically};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// The contents of `library.toml`. Saving borrows the games, so they
/// aren't copied first.
#[derive(Serialize, Deserialize)]
struct LibraryFile<'a> {
    games: Cow<'a, [LibraryEntry]>,
}

/// Reads the known games, or none if there is no library file yet. A file
/// that can't be parsed is kept aside first (`library.kept-1.toml`), and the
/// error says where it went; one that can't be read at all stays where it
/// is, and the error says saving over it isn't safe.
pub fn load_entries(cache: &Cache) -> Result<Vec<LibraryEntry>, Unreadable> {
    let path = data_directory(cache)
        .map_err(|error| super::unreadable_library(&error, None))?
        .join("library.toml");

    let parsed = match fs::read(&path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|_| String::from("stream did not contain valid UTF-8"))
            .and_then(|contents| {
                toml::from_str::<LibraryFile>(&contents).map_err(|error| error.to_string())
            }),
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(super::unreadable_library(&error.to_string(), None)),
    };
    parsed.map(|file| file.games.into_owned()).map_err(|error| {
        let kept = keep_aside(&path)
            .map(|kept| kept.display().to_string())
            .map_err(|keep_error| keep_error.to_string());
        super::unreadable_library(&error, Some(kept))
    })
}

/// Writes the known games to `library.toml`.
pub fn save_entries(cache: &Cache, entries: &[LibraryEntry]) -> Result<(), String> {
    let file = LibraryFile {
        games: Cow::Borrowed(entries),
    };
    let contents = toml::to_string(&file).map_err(|error| error.to_string())?;

    write_atomically(
        &data_directory(cache)?.join("library.toml"),
        contents.as_bytes(),
    )
    .map_err(|error| error.to_string())
}

/// Saves a frame of the game at `rom_path` as its thumbnail.
pub fn save_thumbnail(cache: &Cache, rom_path: &Path, pixels: &[u8]) -> Result<(), String> {
    let png = encode_png(SCREEN_WIDTH, SCREEN_HEIGHT, pixels)?;

    write_atomically(
        &picture_path(cache, PictureKind::LastScreen, rom_path)?,
        &png,
    )
    .map_err(|error| format!("The game's thumbnail couldn't be saved: {error}"))
}

/// Saves the title screen of the game at `rom_path`, as the emulator drew
/// it (in gray for original Game Boy games).
pub fn save_title_screen(cache: &Cache, rom_path: &Path, pixels: &[u8]) -> Result<(), String> {
    write_atomically(
        &picture_path(cache, PictureKind::TitleScreen, rom_path)?,
        &encode_png(SCREEN_WIDTH, SCREEN_HEIGHT, pixels)?,
    )
    .map_err(|error| format!("A title screen couldn't be saved: {error}"))
}

/// Reads a saved picture of the game at `rom_path` as its width, height
/// and RGBA pixels, or `None` if there is none.
pub fn read_picture(
    cache: &Cache,
    kind: PictureKind,
    rom_path: &Path,
) -> Option<(u32, u32, Vec<u8>)> {
    decode_png(&fs::read(picture_path(cache, kind, rom_path).ok()?).ok()?)
}

/// Where the picture of `kind` of the game at `rom_path` is kept: named
/// after the ROM's file name, like battery saves, such as `Tetris.gb.png`,
/// in the `box_art`, `title_screens` or `thumbnails` folder. The library
/// passes the path its pictures go by (`Library::pictures_as`), whose file
/// name is the name the game owns.
fn picture_path(cache: &Cache, kind: PictureKind, rom_path: &Path) -> Result<PathBuf, String> {
    let folder = match kind {
        PictureKind::BoxArt => "box_art",
        PictureKind::TitleScreen => "title_screens",
        PictureKind::LastScreen => "thumbnails",
    };
    let mut file_name = rom_path.file_name().unwrap_or_default().to_os_string();
    file_name.push(".png");
    Ok(data_directory(cache)?.join(folder).join(file_name))
}

/// The saved picture of the game at `rom_path`, kept under `pictures_as`'s
/// name, as a PNG, if there is one made after the ROM file last changed. A
/// picture of an older version of the file is ignored, so it is made again.
pub fn current_picture(
    cache: &Cache,
    kind: PictureKind,
    pictures_as: &Path,
    rom_path: &Path,
) -> Option<Vec<u8>> {
    let path = picture_path(cache, kind, pictures_as).ok()?;
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
pub fn save_box_art(cache: &Cache, rom_path: &Path, png: &[u8]) -> Result<(), String> {
    write_atomically(&picture_path(cache, PictureKind::BoxArt, rom_path)?, png)
        .map_err(|error| format!("Box art couldn't be saved: {error}"))
}

/// Deletes every picture kept under the name of `rom_path`, so they are
/// made again. One that can't be deleted stays: it may show another game's
/// picture until then, nothing worse.
pub fn delete_pictures(cache: &Cache, rom_path: &Path) {
    for kind in [
        PictureKind::BoxArt,
        PictureKind::TitleScreen,
        PictureKind::LastScreen,
    ] {
        if let Ok(path) = picture_path(cache, kind, rom_path) {
            let _ = fs::remove_file(path);
        }
    }
}

/// The cache's data folder, or why there is none.
fn data_directory(cache: &Cache) -> Result<&Path, String> {
    cache
        .data_folder
        .as_deref()
        .ok_or_else(|| String::from("the system has no data folder for us"))
}
