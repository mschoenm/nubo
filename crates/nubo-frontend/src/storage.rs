//! Where an app keeps data between runs. [`Storage`] is what the other
//! modules write through, and each platform has one: files in the user's
//! folders on the desktop (`files`, the `desktop` feature) and
//! localStorage in the browser (`local_storage`, the `web` feature).
//! Another platform adds its own. [`platform`] picks the one this build has,
//! and a build with neither feature keeps nothing ([`NoStorage`]).
//!
//! Save states and the library need more than reading and writing whole
//! entries (renaming, file times, folders of pictures), so they use `files`
//! directly on the desktop, and the browser's IndexedDB (`indexed_db`, the
//! `web` feature) in the browser, which holds more than localStorage's few
//! megabytes.
//!
//! A stored entry that can't be read any more, such as settings a newer Nubo
//! wrote, is never written over: it is kept aside under the first free
//! `<name>.kept-<number>.<extension>` name ([`kept_name`],
//! [`Storage::keep_aside`]), such as `settings.kept-1.toml`, and the player
//! is told where. Those names are permanent too.

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
    /// The last game played, which versions of the browser build before its
    /// library kept so it could start again without picking its file (see
    /// `rom::remembered`). Only the browser has it (`last-game`).
    LastGame,
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
            Entry::LastGame => PathBuf::from("last-game"),
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

    /// Deletes the battery saves of every game, RAM and clock alike, which
    /// the player asks for when they reset the settings. It goes on past an
    /// entry it can't delete, and then says how many it couldn't.
    fn delete_battery_saves(&self) -> Result<(), String>;

    /// Moves what is kept for `entry` to the first free [`kept_name`], so
    /// writing `entry` again can't lose it, and returns where it went (as
    /// [`Storage::location`] says it). For an entry that can't be read any
    /// more, such as settings with a value only a newer Nubo knows.
    fn keep_aside(&self, entry: Entry<'_>) -> Result<String, String>;
}

/// The name an entry called `name` is kept aside under (see
/// [`Storage::keep_aside`]): `.kept-<number>` goes before the last
/// extension, so `settings.toml` becomes `settings.kept-1.toml` and
/// `cheats/Tetris.gb.toml` becomes `cheats/Tetris.gb.kept-1.toml`.
pub fn kept_name(name: &str, number: u32) -> String {
    let file_name_start = name.rfind('/').map_or(0, |slash| slash + 1);
    match name[file_name_start..].rfind('.') {
        Some(dot) if dot > 0 => {
            let (stem, extension) = name.split_at(file_name_start + dot);
            format!("{stem}.kept-{number}{extension}")
        }
        _ => format!("{name}.kept-{number}"),
    }
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

    fn delete_battery_saves(&self) -> Result<(), String> {
        Ok(())
    }

    fn keep_aside(&self, _entry: Entry<'_>) -> Result<String, String> {
        Err(String::from("this build keeps no data"))
    }
}

/// Why some entries couldn't be deleted: how many, and the first reason.
/// `Ok` when every one was.
#[cfg(any(feature = "desktop", feature = "web"))]
fn deleted(failures: usize, first_error: Option<String>) -> Result<(), String> {
    match first_error {
        None => Ok(()),
        Some(error) if failures == 1 => Err(error),
        Some(error) => Err(format!(
            "{failures} couldn't be deleted, the first because {error}"
        )),
    }
}

#[cfg(feature = "desktop")]
pub mod files;
#[cfg(any(feature = "web", test))]
pub mod indexed_db;
pub mod local_storage;
#[cfg(test)]
mod tests;
