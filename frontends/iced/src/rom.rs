//! ROM files: reading them from a path (the command line or a file dropped
//! on the window) or from the file picker.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The file extensions offered by the file picker.
const EXTENSIONS: [&str; 3] = ["gb", "gbc", "zip"];

/// A ROM file's contents and the path it came from. The path names the
/// battery save files.
#[derive(Debug, Clone)]
pub struct Rom {
    pub path: PathBuf,
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

fn display_name(path: &Path) -> String {
    path.file_stem()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

/// Opens the file picker and reads the chosen ROM, or returns `None` if the
/// user cancels.
pub async fn pick() -> Option<Rom> {
    let file = rfd::AsyncFileDialog::new()
        .add_filter("Game Boy ROMs", &EXTENSIONS)
        .set_title("Open a Game Boy ROM")
        .pick_file()
        .await?;

    Some(Rom {
        path: picked_path(&file),
        bytes: Arc::from(file.read().await),
    })
}

/// The path of the picked file, whose name identifies its battery saves.
#[cfg(not(target_arch = "wasm32"))]
fn picked_path(file: &rfd::FileHandle) -> PathBuf {
    file.path().to_path_buf()
}

/// Browsers hide file paths, so the file name alone identifies the saves.
#[cfg(target_arch = "wasm32")]
fn picked_path(file: &rfd::FileHandle) -> PathBuf {
    PathBuf::from(file.file_name())
}
