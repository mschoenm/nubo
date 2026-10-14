//! Screenshots of the Game Boy screen, saved as PNG files in a `Nubo`
//! folder in the user's Pictures folder (only with the `desktop` feature).

use std::path::PathBuf;

/// Whether this build can save screenshots: only the desktop build can.
pub const AVAILABLE: bool = cfg!(feature = "desktop");

/// Saves `pixels` as the next free `<game name> <number>.png` and returns
/// where it went.
#[cfg(feature = "desktop")]
pub fn save(game_name: &str, pixels: &[u8]) -> Result<PathBuf, String> {
    use crate::storage::files::{encode_png, project_directories, write_atomically};
    use directories::UserDirs;

    let folder = UserDirs::new()
        .and_then(|directories| {
            directories
                .picture_dir()
                .map(|pictures| pictures.join("Nubo"))
        })
        .or_else(|| {
            project_directories().map(|directories| directories.data_dir().join("screenshots"))
        })
        .ok_or("the system has no Pictures folder")?;

    let path = (1..)
        .map(|number| folder.join(format!("{game_name} {number}.png")))
        .find(|path| !path.exists())
        .ok_or("no free file name")?;

    write_atomically(&path, &encode_png(pixels)?)
        .map_err(|error| format!("Couldn't save the screenshot: {error}"))?;
    Ok(path)
}

/// Without the `desktop` feature there is nowhere to save screenshots.
#[cfg(not(feature = "desktop"))]
pub fn save(_game_name: &str, _pixels: &[u8]) -> Result<PathBuf, String> {
    Err(String::from("screenshots aren't available in the browser"))
}
