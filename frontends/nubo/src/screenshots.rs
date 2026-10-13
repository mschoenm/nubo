//! Screenshots of the Game Boy screen, saved as PNG files in a `Nubo`
//! folder in the user's Pictures folder (desktop build only).

/// Whether this build can save screenshots.
pub const AVAILABLE: bool = cfg!(not(target_arch = "wasm32"));

/// Saves `pixels` as the next free `<game name> <number>.png` and returns
/// where it went.
#[cfg(not(target_arch = "wasm32"))]
pub fn save(game_name: &str, pixels: &[u8]) -> Result<std::path::PathBuf, String> {
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

#[cfg(target_arch = "wasm32")]
pub fn save(_game_name: &str, _pixels: &[u8]) -> Result<std::path::PathBuf, String> {
    Err(String::from("screenshots aren't available in the browser"))
}
