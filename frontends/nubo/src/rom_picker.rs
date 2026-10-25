//! The file pickers for opening a ROM and adding a ROM folder to the
//! library, with rfd: the system's file dialog on the desktop and the
//! browser's file picker on the web. The picked files are read with the same
//! size limit and errors as every other way of opening a game. Reading a ROM from a path is in
//! `nubo_frontend::rom`, and scanning a folder in `nubo_frontend::library`.

use nubo_frontend::rom::{EXTENSIONS, Rom};
use std::path::PathBuf;

/// Opens the file picker and reads the chosen ROM, or says why it couldn't
/// (see [`read`]), or returns `None` if the user cancels. In the browser this
/// is rfd's browser file picker, which hands over the file's bytes, since a
/// web page can't open files by path.
pub async fn pick() -> Option<Result<Rom, String>> {
    let file = rfd::AsyncFileDialog::new()
        .add_filter("Game Boy games", &EXTENSIONS)
        .set_title("Open a Game Boy game")
        .pick_file()
        .await?;

    Some(read(file).await)
}

/// Opens the file picker for several games at once, as the browser's
/// library adds them, and reads the chosen ones (see [`read`]); none if the
/// user cancels.
pub async fn pick_many() -> Vec<Result<Rom, String>> {
    let Some(files) = rfd::AsyncFileDialog::new()
        .add_filter("Game Boy games", &EXTENSIONS)
        .set_title("Add Game Boy games")
        .pick_files()
        .await
    else {
        return Vec::new();
    };

    let mut roms = Vec::with_capacity(files.len());
    for file in files {
        roms.push(read(file).await);
    }
    roms
}

/// Opens the folder picker, returning the chosen folder or `None` if the
/// user cancels.
#[cfg(not(target_arch = "wasm32"))]
pub async fn pick_folder() -> Option<PathBuf> {
    let folder = rfd::AsyncFileDialog::new()
        .set_title("Add a folder of games")
        .pick_folder()
        .await?;
    Some(folder.path().to_path_buf())
}

/// Browsers have no folder picker, so the library has no folders.
#[cfg(target_arch = "wasm32")]
pub async fn pick_folder() -> Option<PathBuf> {
    None
}

/// Reads the picked file as every other way of opening a game does
/// (`Rom::read`, on a thread of its own), with its size limit and an error
/// instead of rfd's own read, which panics when the file can't be read.
#[cfg(not(target_arch = "wasm32"))]
async fn read(file: rfd::FileHandle) -> Result<Rom, String> {
    nubo_frontend::rom::load(file.path().to_path_buf()).await
}

/// Reads the picked file, unless it is bigger than any game, which isn't
/// copied into the page's memory. Browsers hide file paths, so the file name
/// alone identifies the saves.
#[cfg(target_arch = "wasm32")]
async fn read(file: rfd::FileHandle) -> Result<Rom, String> {
    let path = PathBuf::from(file.file_name());
    if file.inner().size() > nubo_frontend::rom::MAX_FILE_SIZE as f64 {
        return Err(format!(
            "Couldn't open {}: it is too big to be a Game Boy game",
            nubo_frontend::rom::display_name(&path)
        ));
    }
    Ok(Rom {
        path,
        bytes: std::sync::Arc::from(file.read().await),
    })
}
