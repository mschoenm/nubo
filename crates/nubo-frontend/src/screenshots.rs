//! Screenshots of the game's picture as shown (with a Super Game Boy's
//! border when it shows one), saved as PNG files in a `Nubo` folder in the
//! user's Pictures folder (only with the `desktop` feature), or in the data
//! folder's `screenshots/` when the system has no Pictures folder or the run
//! keeps its data in a folder of its own (`storage::files::use_folder`).

use crate::picture::Picture;
use std::path::PathBuf;

/// Whether this build can save screenshots: only the desktop build can.
pub const AVAILABLE: bool = cfg!(feature = "desktop");

/// Saves `picture` as the next free `<game name> <number>.png` and returns
/// where it went.
#[cfg(feature = "desktop")]
pub fn save(game_name: &str, picture: &Picture) -> Result<PathBuf, String> {
    use crate::storage::files::{data_folder, encode_png, folders_replaced, write_atomically};
    use directories::UserDirs;

    let folder = UserDirs::new()
        .filter(|_| !folders_replaced())
        .and_then(|directories| {
            directories
                .picture_dir()
                .map(|pictures| pictures.join("Nubo"))
        })
        .or_else(|| data_folder().map(|folder| folder.join("screenshots")))
        .ok_or("the system has no Pictures folder")?;

    let path = (1..)
        .map(|number| folder.join(format!("{game_name} {number}.png")))
        .find(|path| !path.exists())
        .ok_or("no free file name")?;

    write_atomically(
        &path,
        &encode_png(picture.width(), picture.height(), picture.pixels())?,
    )
    .map_err(|error| format!("Couldn't save the screenshot: {error}"))?;
    Ok(path)
}

/// Without the `desktop` feature there is nowhere to save screenshots.
#[cfg(not(feature = "desktop"))]
pub fn save(_game_name: &str, _picture: &Picture) -> Result<PathBuf, String> {
    Err(String::from("screenshots aren't available in the browser"))
}
