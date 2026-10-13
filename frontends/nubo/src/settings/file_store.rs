//! Settings stored in `settings.toml` in the per-user config folder (native
//! build): `~/Library/Application Support/Nubo` on macOS,
//! `%APPDATA%\Nubo\config` on Windows and `$XDG_CONFIG_HOME/nubo`
//! (usually `~/.config/nubo`) on Linux.

use crate::storage::files::{project_directories, write_atomically};
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

/// Reads the settings file, or returns `None` if there isn't one yet.
pub fn read() -> Result<Option<String>, String> {
    match fs::read_to_string(path()?) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

/// Replaces the settings file's contents.
pub fn write(contents: &str) -> Result<(), String> {
    write_atomically(&path()?, contents.as_bytes()).map_err(|error| error.to_string())
}

fn path() -> Result<PathBuf, String> {
    project_directories()
        .map(|directories| directories.config_dir().join("settings.toml"))
        .ok_or_else(|| String::from("the system has no config folder for us"))
}
