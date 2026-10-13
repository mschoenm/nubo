//! The file picker for opening a ROM, with rfd: the system's file dialog on
//! the desktop and the browser's file picker on the web. Reading a ROM from
//! a path is in `nubo_frontend::rom`.

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
