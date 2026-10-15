//! ROM files: a game's contents and the path it came from, such as the
//! command line or a file dropped on the window. Picking a file is up to the
//! app, which makes a [`Rom`] of what the player picked.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A ROM file's contents and the path it came from. The path names the
/// battery save files.
#[derive(Debug, Clone)]
pub struct Rom {
    /// Where the ROM came from. In the browser, which hides file paths, only
    /// the file name.
    pub path: PathBuf,
    /// The file's contents: the ROM itself, or a zip file holding it.
    pub bytes: Arc<[u8]>,
}

impl Rom {
    /// Reads the ROM at `path`, or says why it couldn't.
    pub fn read(path: &Path) -> Result<Rom, String> {
        match fs::read(path) {
            Ok(bytes) => Ok(Rom {
                path: path.to_path_buf(),
                bytes: Arc::from(bytes),
            }),
            Err(error) => Err(format!("Couldn't open {}: {error}", display_name(path))),
        }
    }

    /// The game's name, made from its file name (see [`game_name`]), such
    /// as "Tetris" for `roms/Tetris (World) (Rev 1).gb`, used in the window.
    pub fn name(&self) -> String {
        display_name(&self.path)
    }
}

/// Whether this build keeps the last game played, to start it again
/// without picking its file: only the browser build does, since the desktop
/// has the library.
pub const REMEMBERS_LAST_GAME: bool = cfg!(all(feature = "web", not(feature = "desktop")));

/// The largest game kept as the last game played. The browser keeps about
/// five megabytes for a site, and the battery saves need room too.
const MAX_REMEMBERED_SIZE: usize = 2 * 1024 * 1024;

/// Keeps `rom` as the last game played, if this build keeps one. The file
/// name goes first, then a zero byte, then the file's contents. A game too
/// big to keep, or one that doesn't fit, forgets the game kept before, so
/// the next start doesn't offer an older game than the last one played.
pub fn remember(rom: &Rom) -> Result<(), String> {
    if !REMEMBERS_LAST_GAME {
        return Ok(());
    }
    let storage = crate::storage::platform();
    let forget = || storage.write(crate::storage::Entry::LastGame, &[]);
    if rom.bytes.len() > MAX_REMEMBERED_SIZE {
        return forget();
    }
    let name = rom.path.file_name().unwrap_or_default().to_string_lossy();
    let mut contents = Vec::with_capacity(name.len() + 1 + rom.bytes.len());
    contents.extend_from_slice(name.as_bytes());
    contents.push(0);
    contents.extend_from_slice(&rom.bytes);
    storage
        .write(crate::storage::Entry::LastGame, &contents)
        .map_err(|error| {
            let _ = forget();
            format!("Couldn't keep the game for next time: {error}")
        })
}

/// Forgets the last game played, if this build keeps one, so the next
/// start doesn't offer it.
pub fn forget() -> Result<(), String> {
    if !REMEMBERS_LAST_GAME {
        return Ok(());
    }
    crate::storage::platform()
        .write(crate::storage::Entry::LastGame, &[])
        .map_err(|error| format!("Couldn't forget the last game played: {error}"))
}

/// The last game played, if this build keeps one and it was kept (an empty
/// entry is a forgotten one).
pub fn remembered() -> Option<Rom> {
    if !REMEMBERS_LAST_GAME {
        return None;
    }
    let contents = crate::storage::platform()
        .read(crate::storage::Entry::LastGame)
        .ok()??;
    let split = contents.iter().position(|&byte| byte == 0)?;
    let name = String::from_utf8_lossy(&contents[..split]).into_owned();
    Some(Rom {
        path: PathBuf::from(name),
        bytes: Arc::from(&contents[split + 1..]),
    })
}

/// The game's name for the ROM at `path`, made from its file name.
pub fn display_name(path: &Path) -> String {
    game_name(
        &path
            .file_stem()
            .unwrap_or(path.as_os_str())
            .to_string_lossy(),
    )
}

/// The name to show for a game whose file is called `file_stem` (without its
/// extension). Files are often named the No-Intro way, such as
/// "Legend of Zelda, The - Link's Awakening DX (USA, Europe) (Rev 2)", so
/// this drops the bracketed tags, puts an article moved to the end back in
/// front ("The Legend of Zelda - Link's Awakening DX") and turns underscores
/// into spaces. A name that would come out empty stays as it was.
pub fn game_name(file_stem: &str) -> String {
    let mut name = String::new();
    let mut depth = 0_usize;
    for character in file_stem.chars() {
        match character {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            '_' if depth == 0 => name.push(' '),
            _ if depth == 0 => name.push(character),
            _ => {}
        }
    }
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let name = name.trim_end_matches([' ', '-', ',']).to_owned();
    if name.is_empty() {
        return file_stem.to_owned();
    }

    let (title, rest) = match name.split_once(" - ") {
        Some((title, rest)) => (title, Some(rest)),
        None => (name.as_str(), None),
    };
    let title = ["The", "A", "An"]
        .iter()
        .find_map(|article| {
            title
                .strip_suffix(&format!(", {article}"))
                .map(|start| format!("{article} {start}"))
        })
        .unwrap_or_else(|| title.to_owned());

    match rest {
        Some(rest) => format!("{title} - {rest}"),
        None => title,
    }
}

#[cfg(test)]
mod tests;
