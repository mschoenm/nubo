//! Save slots stored as files in the user's data folder (native build).

use crate::storage::files::{project_directories, write_atomically};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// A file in the saves folder named after the ROM, such as `game.sav` for
/// `game.gb`.
pub struct SaveSlot {
    /// `None` when the operating system has no data folder for us, for
    /// example because the home directory is unknown.
    path: Option<PathBuf>,
    file_name: PathBuf,
}

impl SaveSlot {
    /// The slot for `rom_path` with its extension replaced by `extension`, in
    /// the saves folder that [`saves_directory`] picks for this platform.
    pub fn for_rom(rom_path: &Path, extension: &str) -> SaveSlot {
        Self::in_directory(saves_directory().as_deref(), rom_path, extension)
    }

    /// The slot for `rom_path` with its extension replaced by `extension`, in
    /// `directory`.
    pub fn in_directory(directory: Option<&Path>, rom_path: &Path, extension: &str) -> SaveSlot {
        let file_name =
            PathBuf::from(rom_path.file_name().unwrap_or_default()).with_extension(extension);

        SaveSlot {
            path: directory.map(|directory| directory.join(&file_name)),
            file_name,
        }
    }

    /// Reads the file, or returns `None` if it doesn't exist or can't be read.
    pub fn read(&self) -> Option<Vec<u8>> {
        fs::read(self.path.as_ref()?).ok()
    }

    /// Replaces the file's contents, creating the saves folder if needed. It
    /// writes a temporary file first and renames it, so a crash halfway never
    /// leaves a half-written save.
    pub fn write(&self, contents: &[u8]) -> Result<(), String> {
        let path = self
            .path
            .as_ref()
            .ok_or("the system has no data folder to keep saves in")?;

        write_atomically(path, contents).map_err(|error| error.to_string())
    }
}

impl fmt::Display for SaveSlot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self.path.as_ref().unwrap_or(&self.file_name);
        write!(formatter, "{}", path.display())
    }
}

/// The folder for battery saves inside the per-user data folder of each
/// operating system: `~/Library/Application Support/Nubo/saves` on
/// macOS, `%APPDATA%\Nubo\data\saves` on Windows and
/// `$XDG_DATA_HOME/nubo/saves` (usually `~/.local/share/nubo/saves`)
/// on Linux.
fn saves_directory() -> Option<PathBuf> {
    project_directories().map(|directories| directories.data_dir().join("saves"))
}
