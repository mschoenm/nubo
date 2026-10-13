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

    /// The file name without its extension, such as "Tetris" for
    /// `roms/Tetris.gb`, used as the game's name in the window.
    pub fn name(&self) -> String {
        display_name(&self.path)
    }
}

/// The file name of `path` without its extension.
fn display_name(path: &Path) -> String {
    path.file_stem()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}
