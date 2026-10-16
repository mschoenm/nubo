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

/// Reads the game at `path` for the app to start: on the desktop the file
/// at `path`, on a thread of its own; in the browser the file the library
/// keeps under its name (see `library::KEEPS_GAMES`).
pub async fn load(path: PathBuf) -> Result<Rom, String> {
    #[cfg(not(all(feature = "web", not(feature = "desktop"))))]
    {
        crate::background::run(move || Rom::read(&path))
            .await
            .unwrap_or_else(Err)
    }
    #[cfg(all(feature = "web", not(feature = "desktop")))]
    {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        match crate::storage::indexed_db::read_rom(&name).await {
            Ok(Some(bytes)) => Ok(Rom { path, bytes }),
            Ok(None) => Err(format!(
                "Couldn't open {}: the browser doesn't keep it any more",
                display_name(&path)
            )),
            Err(error) => Err(format!("Couldn't open {}: {error}", display_name(&path))),
        }
    }
}

/// The last game played as versions of the browser build before its
/// library kept it, if one was kept (an empty entry is a forgotten one).
/// The browser build moves it into the library once, then calls [`forget`].
pub fn remembered() -> Option<Rom> {
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

/// Forgets the last game played that [`remembered`] reads.
pub fn forget() -> Result<(), String> {
    crate::storage::platform()
        .write(crate::storage::Entry::LastGame, &[])
        .map_err(|error| format!("Couldn't forget the last game played: {error}"))
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
