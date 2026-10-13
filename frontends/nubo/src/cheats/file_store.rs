//! Cheat lists as TOML files in the per-user data folder (native build).

use super::Cheat;
use crate::storage::files::{project_directories, write_atomically};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// The contents of a cheats file.
#[derive(Serialize, Deserialize)]
struct CheatsFile {
    cheats: Vec<Cheat>,
}

pub fn load(rom_path: &Path) -> Result<Vec<Cheat>, String> {
    match fs::read_to_string(path(rom_path)?) {
        Ok(contents) => toml::from_str::<CheatsFile>(&contents)
            .map(|file| file.cheats)
            .map_err(|error| error.to_string()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.to_string()),
    }
}

pub fn save(rom_path: &Path, cheats: &[Cheat]) -> Result<(), String> {
    let file = CheatsFile {
        cheats: cheats.to_vec(),
    };
    let contents = toml::to_string(&file).map_err(|error| error.to_string())?;
    write_atomically(&path(rom_path)?, contents.as_bytes()).map_err(|error| error.to_string())
}

/// `cheats/<rom file name>.toml`, such as `cheats/Tetris.gb.toml`.
fn path(rom_path: &Path) -> Result<PathBuf, String> {
    let mut file_name = rom_path.file_name().unwrap_or_default().to_os_string();
    file_name.push(".toml");

    project_directories()
        .map(|directories| directories.data_dir().join("cheats").join(file_name))
        .ok_or_else(|| String::from("the system has no data folder for us"))
}
