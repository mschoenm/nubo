//! Save slots stored in the browser's localStorage (browser build).

use crate::storage::local_storage::{KEY_PREFIX, local_storage};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use std::fmt;
use std::path::Path;

/// A localStorage entry holding base64-encoded save data, keyed by the ROM's
/// file name, such as `nubo:game.sav` for `game.gb`.
pub struct SaveSlot {
    key: String,
}

impl SaveSlot {
    /// The slot for `rom_path` with its extension replaced by `extension`.
    pub fn for_rom(rom_path: &Path, extension: &str) -> SaveSlot {
        SaveSlot {
            key: format!(
                "{KEY_PREFIX}{}",
                rom_path.with_extension(extension).display()
            ),
        }
    }

    /// Reads and decodes the entry, or returns `None` if it is missing or
    /// unreadable.
    pub fn read(&self) -> Option<Vec<u8>> {
        let encoded = local_storage().ok()?.get_item(&self.key).ok()??;
        STANDARD.decode(encoded).ok()
    }

    /// Replaces the entry's contents.
    pub fn write(&self, contents: &[u8]) -> Result<(), String> {
        local_storage()?
            .set_item(&self.key, &STANDARD.encode(contents))
            .map_err(|error| format!("{error:?}"))
    }
}

impl fmt::Display for SaveSlot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "localStorage key {}", self.key)
    }
}
