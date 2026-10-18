//! The browser's localStorage, which the browser build keeps its data in
//! instead of files. Each entry is a key that starts with `nubo:`: the
//! settings as TOML text under `nubo:settings.toml`, and battery saves as
//! base64 under the ROM's name with the save's extension, such as
//! `nubo:game.sav` for `game.gb`, each game's cheats as TOML text under
//! `nubo:cheats/` and the ROM's file name, such as
//! `nubo:cheats/game.gb.toml`, and the last game played as base64 under
//! `nubo:last-game`. An entry that couldn't be read is kept aside under the
//! same key with `.kept-<number>` before its extension, such as
//! `nubo:settings.kept-1.toml` (see [`super::kept_name`]). Bigger things,
//! such as save states and the library, are in IndexedDB
//! (`super::indexed_db`).
//!
//! The keys are worked out without the `web` feature too, so their tests run
//! in every build; only reaching the browser's localStorage needs it.

use super::Entry;
#[cfg(feature = "web")]
use super::Storage;
#[cfg(feature = "web")]
use base64::Engine;
#[cfg(feature = "web")]
use base64::engine::general_purpose::STANDARD;

/// Put in front of every key, so Nubo's entries don't clash with other
/// apps on the same site.
pub const KEY_PREFIX: &str = "nubo:";

/// [`super::Storage`] in the page's localStorage.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalStorage;

impl LocalStorage {
    /// The localStorage key of `entry`, or why the browser doesn't keep it.
    pub fn key(entry: Entry<'_>) -> Result<String, String> {
        match entry {
            Entry::Settings => Ok(format!("{KEY_PREFIX}settings.toml")),
            Entry::BatteryRam(rom_path) => Ok(format!(
                "{KEY_PREFIX}{}",
                rom_path.with_extension("sav").display()
            )),
            Entry::BatteryClock(rom_path) => Ok(format!(
                "{KEY_PREFIX}{}",
                rom_path.with_extension("rtc").display()
            )),
            Entry::Cheats(_) => Ok(format!(
                "{KEY_PREFIX}cheats/{}",
                entry.file_name().display()
            )),
            Entry::LastGame => Ok(format!("{KEY_PREFIX}last-game")),
        }
    }

    /// The key `entry` is kept aside under the `number`th time it couldn't
    /// be read (see [`super::kept_name`]), such as
    /// `nubo:settings.kept-1.toml`.
    pub fn kept_key(entry: Entry<'_>, number: u32) -> Result<String, String> {
        Ok(super::kept_name(&Self::key(entry)?, number))
    }
}

#[cfg(feature = "web")]
impl Storage for LocalStorage {
    fn read(&self, entry: Entry<'_>) -> Result<Option<Vec<u8>>, String> {
        let Ok(key) = Self::key(entry) else {
            return Ok(None);
        };
        let Some(stored) = local_storage()?
            .get_item(&key)
            .map_err(|error| format!("{error:?}"))?
        else {
            return Ok(None);
        };

        if is_text(entry) {
            Ok(Some(stored.into_bytes()))
        } else {
            STANDARD
                .decode(stored)
                .map(Some)
                .map_err(|error| error.to_string())
        }
    }

    fn write(&self, entry: Entry<'_>, contents: &[u8]) -> Result<(), String> {
        let key = Self::key(entry)?;
        let stored = if is_text(entry) {
            String::from_utf8(contents.to_vec()).map_err(|error| error.to_string())?
        } else {
            STANDARD.encode(contents)
        };

        local_storage()?
            .set_item(&key, &stored)
            .map_err(|error| format!("{error:?}"))
    }

    fn location(&self, entry: Entry<'_>) -> String {
        match Self::key(entry) {
            Ok(key) => format!("localStorage key {key}"),
            Err(_) => String::from("localStorage"),
        }
    }

    fn delete_battery_saves(&self) -> Result<(), String> {
        let storage = local_storage()?;
        let length = storage.length().map_err(|error| format!("{error:?}"))?;
        let keys: Vec<String> = (0..length)
            .filter_map(|index| storage.key(index).ok().flatten())
            .filter(|key| is_battery_save_key(key))
            .collect();

        let mut failures = 0;
        let mut first_error = None;
        for key in keys {
            if let Err(error) = storage.remove_item(&key) {
                failures += 1;
                first_error.get_or_insert_with(|| format!("{error:?}"));
            }
        }
        super::deleted(failures, first_error)
    }

    fn keep_aside(&self, entry: Entry<'_>) -> Result<String, String> {
        let key = Self::key(entry)?;
        let storage = local_storage()?;
        let kept = (1..)
            .map(|number| super::kept_name(&key, number))
            .find(|kept| storage.get_item(kept).ok().flatten().is_none())
            .unwrap_or_default();
        let stored = storage
            .get_item(&key)
            .map_err(|error| format!("{error:?}"))?
            .ok_or_else(|| String::from("there is nothing to keep"))?;

        storage
            .set_item(&kept, &stored)
            .and_then(|()| storage.remove_item(&key))
            .map_err(|error| format!("{error:?}"))?;
        Ok(format!("localStorage key {kept}"))
    }
}

/// Whether `key` holds a battery save, RAM or clock (see
/// [`LocalStorage::key`]).
pub fn is_battery_save_key(key: &str) -> bool {
    key.starts_with(KEY_PREFIX)
        && std::path::Path::new(key)
            .extension()
            .is_some_and(|extension| extension == "sav" || extension == "rtc")
}

/// Whether `entry` is kept as text as it is, rather than as base64.
#[cfg(feature = "web")]
fn is_text(entry: Entry<'_>) -> bool {
    matches!(entry, Entry::Settings | Entry::Cheats(_))
}

/// The page's localStorage, which can be missing (for example when the user
/// blocks site data).
#[cfg(feature = "web")]
fn local_storage() -> Result<web_sys::Storage, String> {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .ok_or_else(|| String::from("localStorage is unavailable"))
}

#[cfg(test)]
mod tests;
