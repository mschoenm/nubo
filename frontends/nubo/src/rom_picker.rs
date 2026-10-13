//! The file pickers for opening a ROM and adding a ROM folder to the
//! library, with rfd: the system's file dialog on the desktop and the
//! browser's file picker on the web. Reading a ROM from a path is in
//! `nubo_frontend::rom`, and scanning a folder in `nubo_frontend::library`.

use nubo_frontend::rom::Rom;
use std::path::PathBuf;
use std::sync::Arc;

/// The file extensions offered by the file picker.
const EXTENSIONS: [&str; 3] = ["gb", "gbc", "zip"];

/// Opens the file picker and reads the chosen ROM, or returns `None` if the
/// user cancels. In the browser this is rfd's browser file picker, which
/// hands over the file's bytes, since a web page can't open files by path.
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

/// Opens the folder picker, returning the chosen folder or `None` if the
/// user cancels.
#[cfg(not(target_arch = "wasm32"))]
pub async fn pick_folder() -> Option<PathBuf> {
    let folder = rfd::AsyncFileDialog::new()
        .set_title("Add a ROM folder")
        .pick_folder()
        .await?;
    Some(folder.path().to_path_buf())
}

/// Browsers have no folder picker, so the library has no folders.
#[cfg(target_arch = "wasm32")]
pub async fn pick_folder() -> Option<PathBuf> {
    None
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
