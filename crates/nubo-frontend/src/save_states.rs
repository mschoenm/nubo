//! Save states: snapshots of a running game that the player can return to.
//! Each game has a quick save slot, three numbered slots and an automatic
//! slot that keeps the game where the player left it.
//!
//! The desktop build keeps them in `states/<rom file name>/` in the user's
//! data folder: `<slot>.state` holds the snapshot and `<slot>.png` the screen
//! at that moment. A state that can't be loaded is renamed to
//! `<slot>.kept-<number>.state`, so saving to the slot again can't overwrite
//! it. The browser build keeps the same entries under the same names in its
//! IndexedDB (`storage::indexed_db`), where a slot's time is when its entry
//! was written. Builds with neither feature keep none.
//!
//! The functions that end in `_in` do the same in a folder of the caller's
//! choosing instead of the player's, for tests and measurements.

use crate::picture::Picture;
use crate::time::SystemTime;
use nubo_core::Model;
use std::fmt;
use std::path::Path;

#[cfg(feature = "desktop")]
pub use store::{delete_all_in, keep_aside_in, load_in, save_in, slot_info_in, thumbnail_in};

/// Whether this build can keep save states: the desktop and browser builds
/// can.
pub const AVAILABLE: bool = cfg!(any(feature = "desktop", feature = "web"));

/// What `.state` files of Nubo 0.3.0 and earlier started with, before the
/// core's save states had a format that later versions keep loading.
#[cfg(any(feature = "desktop", feature = "web", test))]
const EARLY_STATE_PREFIX: &[u8] = b"Nubo state ";

/// A place to keep one save state of a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// Written as the player plays a game and when they leave it, and read
    /// when it starts again.
    ///
    /// Only used while "Continue games where you left off" is on (see
    /// `Settings::resume_games`).
    Resume,
    /// The slot the quick save and quick load keys use.
    Quick,
    /// One of the slots the player numbers 1 to 3.
    Numbered(u8),
}

impl Slot {
    /// The slots the player can save to, in the order the menu lists them.
    pub const SAVABLE: [Slot; 4] = [
        Slot::Quick,
        Slot::Numbered(1),
        Slot::Numbered(2),
        Slot::Numbered(3),
    ];

    /// The slots the player can load from, in the order the menu lists them.
    pub const LOADABLE: [Slot; 5] = [
        Slot::Resume,
        Slot::Quick,
        Slot::Numbered(1),
        Slot::Numbered(2),
        Slot::Numbered(3),
    ];
}

impl fmt::Display for Slot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Slot::Resume => formatter.write_str("Where you left off"),
            Slot::Quick => formatter.write_str("Quick save"),
            Slot::Numbered(number) => write!(formatter, "Slot {number}"),
        }
    }
}

/// A snapshot of a game and the model it ran on, which it must be restored
/// on.
#[derive(Debug, Clone)]
pub struct SaveState {
    /// The hardware the game ran on.
    pub model: Model,
    /// The core's save state (`Emulator::save_state`).
    pub data: Vec<u8>,
}

/// What the menu shows about a slot.
#[derive(Debug, Clone)]
pub struct SlotInfo {
    /// The slot this describes.
    pub slot: Slot,
    /// When the slot was saved, or `None` if it is empty.
    pub saved_at: Option<SystemTime>,
    /// The screen when the state was saved, once [`thumbnails`] read it.
    pub thumbnail: Option<Picture>,
}

/// Saves `state` in `slot` of the game at `rom_path`, with `pixels` (the
/// screen at that moment) as its picture.
pub fn save(
    rom_path: &Path,
    slot: Slot,
    state: &SaveState,
    pixels: Option<&[u8]>,
) -> Result<(), String> {
    store::save(rom_path, slot, state, pixels)
        .map_err(|error| format!("Couldn't save the state: {error}"))
}

/// Saves `state` in `slot` of the game at `rom_path` with `png` as its
/// picture, written as it is, such as the bytes [`picture_png`] read from
/// the slot before, or without a picture. This puts back exactly what a slot
/// held, for undoing a save over it.
pub fn save_with_png(
    rom_path: &Path,
    slot: Slot,
    state: &SaveState,
    png: Option<&[u8]>,
) -> Result<(), String> {
    store::save_png(rom_path, slot, state, png)
        .map_err(|error| format!("Couldn't save the state: {error}"))
}

/// Reads the state in `slot` of the game at `rom_path`, or `None` if the slot
/// is empty.
pub fn load(rom_path: &Path, slot: Slot) -> Result<Option<SaveState>, String> {
    store::load(rom_path, slot).map_err(|error| format!("Couldn't read the save state: {error}"))
}

/// Deletes the state in `slot` of the game at `rom_path` and its picture,
/// leaving the slot empty.
pub fn delete(rom_path: &Path, slot: Slot) -> Result<(), String> {
    store::delete(rom_path, slot)
        .map_err(|error| format!("Couldn't delete the save state: {error}"))
}

/// Deletes the save states of every game, with their pictures and the
/// states kept aside because they couldn't be loaded. A running game would
/// save where it was left again, so apps only offer this while no game
/// runs.
pub fn delete_all() -> Result<(), String> {
    store::delete_all().map_err(|error| format!("Couldn't delete the save states: {error}"))
}

/// Whether any of the game at `rom_path`'s slots holds a state.
pub fn any_saved(rom_path: &Path) -> bool {
    Slot::LOADABLE
        .iter()
        .any(|&slot| store::slot_info(rom_path, slot).saved_at.is_some())
}

/// Moves the state in `slot` of the game at `rom_path`, and its picture, to a
/// name of its own, so a state that couldn't be loaded isn't overwritten by
/// the next save to the slot. A newer Nubo, or the one it was made with, may
/// still load it. Returns the state's new file name.
pub fn keep_aside(rom_path: &Path, slot: Slot) -> Result<String, String> {
    store::keep_aside(rom_path, slot)
        .map(|path| {
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
        .map_err(|error| format!("Couldn't keep the save state that didn't load: {error}"))
}

/// Describes each of `slots` of the game at `rom_path`, without their
/// pictures, which [`thumbnails`] reads.
pub fn slot_infos(rom_path: &Path, slots: &[Slot]) -> Vec<SlotInfo> {
    slots
        .iter()
        .map(|&slot| store::slot_info(rom_path, slot))
        .collect()
}

/// Reads and decodes the pictures of `slots` of the game at `rom_path`.
/// Decoding is slow work, so the app runs it with `background::run`.
pub fn thumbnails(rom_path: &Path, slots: &[Slot]) -> Vec<(Slot, Option<Picture>)> {
    slots
        .iter()
        .map(|&slot| (slot, store::thumbnail(rom_path, slot)))
        .collect()
}

/// Reads the picture of `slot` of the game at `rom_path` as it is stored, a
/// PNG file, without decoding it, or `None` if it has none. Quick enough
/// for the UI thread, unlike [`thumbnails`].
pub fn picture_png(rom_path: &Path, slot: Slot) -> Option<Vec<u8>> {
    store::picture_png(rom_path, slot)
}

/// How long ago `time` was, in words, such as "5 minutes ago".
pub fn time_ago(time: SystemTime) -> String {
    let seconds = SystemTime::now()
        .duration_since(time)
        .map_or(0, |duration| duration.as_secs());

    let (amount, unit) = match seconds {
        0..60 => return String::from("Just now"),
        60..3_600 => (seconds / 60, "minute"),
        3_600..86_400 => (seconds / 3_600, "hour"),
        _ => (seconds / 86_400, "day"),
    };

    if amount == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{amount} {unit}s ago")
    }
}

/// Reads a kept `.state` entry, which holds exactly the core's save state
/// (`Emulator::save_state`), whose header names the model it was made on.
/// It is the same on every platform, so there is one format to keep loading
/// in other versions of Nubo.
#[cfg(any(feature = "desktop", feature = "web", test))]
fn parse(contents: Vec<u8>) -> Result<SaveState, String> {
    if contents.starts_with(EARLY_STATE_PREFIX) {
        return Err(String::from(
            "the state was made by Nubo 0.3.0 or earlier, whose save states later versions \
             can't read",
        ));
    }
    let header = nubo_core::SaveStateHeader::read(&contents).map_err(|error| match error {
        nubo_core::SaveStateError::InvalidFormat => {
            String::from("the file isn't a Nubo save state")
        }
        error => error.to_string(),
    })?;
    Ok(SaveState {
        model: header.model,
        data: contents,
    })
}

/// The name of a slot's entries, without extension.
#[cfg(any(feature = "desktop", feature = "web", test))]
fn file_stem(slot: Slot) -> String {
    match slot {
        Slot::Resume => String::from("resume"),
        Slot::Quick => String::from("quick"),
        Slot::Numbered(number) => format!("slot-{number}"),
    }
}

#[cfg(any(test, all(feature = "web", not(feature = "desktop"))))]
mod browser_store;
#[cfg(all(feature = "web", not(feature = "desktop")))]
use browser_store as store;
#[cfg(feature = "desktop")]
#[path = "save_states/file_store.rs"]
mod store;
#[cfg(not(any(feature = "desktop", feature = "web")))]
#[path = "save_states/no_store.rs"]
mod store;
#[cfg(all(test, feature = "desktop"))]
mod tests;
