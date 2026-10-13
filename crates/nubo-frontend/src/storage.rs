//! Where an app keeps data between runs. [`Storage`] is what the other
//! modules write through, and each platform has one: files in the user's
//! folders on the desktop (`files`, the `desktop` feature) and
//! localStorage in the browser (`local_storage`, the `web` feature).
//! Another platform adds its own. [`platform`] picks the one this build has,
//! and a build with neither feature keeps nothing ([`NoStorage`]).
//!
//! Save states, screenshots and the library are only kept on the desktop.
//! They need more than reading and writing whole entries (renaming, file
//! times, folders of pictures), so they use `files` directly.

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Something an app keeps between runs. Each [`Storage`] decides where it
/// goes and how, so the same entry is a file on the desktop and a
/// localStorage key in the browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry<'a> {
    /// The settings, as TOML text (`settings.toml`).
    Settings,
    /// The battery-backed RAM of the game at this path, where the game keeps
    /// its saves (`<rom name>.sav`).
    BatteryRam(&'a Path),
    /// The real-time clock (RTC) of the game at this path, which some
    /// cartridges keep running on their battery (`<rom name>.rtc`).
    BatteryClock(&'a Path),
    /// The cheat list of the game at this path, as TOML text
    /// (`<rom file name>.toml`).
    Cheats(&'a Path),
}

impl Entry<'_> {
    /// The entry's file name, such as `game.sav` for the RAM of `game.gb`.
    pub fn file_name(&self) -> PathBuf {
        match self {
            Entry::Settings => PathBuf::from("settings.toml"),
            Entry::BatteryRam(rom_path) => rom_file_name(rom_path).with_extension("sav"),
            Entry::BatteryClock(rom_path) => rom_file_name(rom_path).with_extension("rtc"),
            Entry::Cheats(rom_path) => {
                let mut file_name = rom_file_name(rom_path).into_os_string();
                file_name.push(".toml");
                PathBuf::from(file_name)
            }
        }
    }
}

/// The file name of the ROM at `rom_path`, such as `Tetris.gb`.
fn rom_file_name(rom_path: &Path) -> PathBuf {
    PathBuf::from(rom_path.file_name().unwrap_or_default())
}

/// A place to keep [`Entry`]s between runs, such as the user's folders or
/// the browser's localStorage. It is shared with the emulator thread, which
/// writes battery saves, so it must be `Send` and `Sync`.
pub trait Storage: Send + Sync {
    /// Reads `entry`, or returns `None` if nothing was kept for it yet.
    fn read(&self, entry: Entry<'_>) -> Result<Option<Vec<u8>>, String>;

    /// Replaces what is kept for `entry` with `contents`.
    fn write(&self, entry: Entry<'_>, contents: &[u8]) -> Result<(), String>;

    /// Where `entry` is kept, for messages such as a save that couldn't be
    /// written.
    fn location(&self, entry: Entry<'_>) -> String;
}

/// The storage of this build: files in the user's folders with the
/// `desktop` feature, otherwise localStorage with the `web` feature,
/// otherwise [`NoStorage`].
pub fn platform() -> Arc<dyn Storage> {
    #[cfg(feature = "desktop")]
    let storage = files::Files::for_user();
    #[cfg(all(feature = "web", not(feature = "desktop")))]
    let storage = local_storage::LocalStorage;
    #[cfg(not(any(feature = "desktop", feature = "web")))]
    let storage = NoStorage;

    Arc::new(storage)
}

/// A storage that keeps nothing: every entry reads as missing, and writing
/// fails. For builds without a platform's storage, such as the RetroArch
/// core, which keeps its data through RetroArch.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoStorage;

impl Storage for NoStorage {
    fn read(&self, _entry: Entry<'_>) -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }

    fn write(&self, _entry: Entry<'_>, _contents: &[u8]) -> Result<(), String> {
        Err(String::from("this build keeps no data"))
    }

    fn location(&self, entry: Entry<'_>) -> String {
        entry.file_name().display().to_string()
    }
}

#[cfg(feature = "desktop")]
pub mod files;
#[cfg(feature = "web")]
pub mod local_storage;
#[cfg(test)]
mod tests;
