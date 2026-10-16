//! Screenshots of the game's picture as shown (with a Super Game Boy's
//! border when it shows one), saved as PNG files. On the desktop (the
//! `desktop` feature) they go to a `Nubo` folder in the user's Pictures
//! folder, or to the data folder's `screenshots/` when the system has no
//! Pictures folder or the run keeps its data in a folder of its own
//! (`storage::files::use_folder`). In the browser (the `web` feature) the
//! page hands each one to the browser as a download.

use crate::picture::Picture;
use std::path::PathBuf;

/// Whether this build can save screenshots: the desktop and browser builds
/// can.
pub const AVAILABLE: bool = cfg!(any(feature = "desktop", feature = "web"));

/// Whether a screenshot is a download, which the browser puts where it puts
/// downloads, rather than a file the app saved itself.
pub const DOWNLOADED: bool = cfg!(all(feature = "web", not(feature = "desktop")));

/// Saves `picture` as the next free `<game name> <number>.png` and returns
/// where it went.
#[cfg(feature = "desktop")]
pub fn save(game_name: &str, picture: &Picture) -> Result<PathBuf, String> {
    use crate::picture::encode_png;
    use crate::storage::files::{data_folder, folders_replaced, write_atomically};
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

/// Hands `picture` to the browser as the download
/// `<game name> <date> <time>.png`, and returns that name. The page can't
/// see the downloads folder, so the time keeps the names apart.
#[cfg(all(feature = "web", not(feature = "desktop")))]
pub fn save(game_name: &str, picture: &Picture) -> Result<PathBuf, String> {
    use crate::picture::encode_png;

    let png = encode_png(picture.width(), picture.height(), picture.pixels())?;
    let time = chrono::Local::now().format("%Y-%m-%d %H.%M.%S");
    let file_name = format!("{game_name} {time}.png");
    crate::links::download(&file_name, "image/png", &png)
        .map_err(|error| format!("Couldn't save the screenshot: {error}"))?;
    Ok(PathBuf::from(file_name))
}

/// Without the `desktop` or `web` feature there is nowhere to save
/// screenshots.
#[cfg(not(any(feature = "desktop", feature = "web")))]
pub fn save(_game_name: &str, _picture: &Picture) -> Result<PathBuf, String> {
    Err(String::from("this build saves no screenshots"))
}
