//! Save states: snapshots of a running game that the player can return to.
//! Each game has a quick save slot, three numbered slots and an automatic
//! slot that keeps the game where the player left it.
//!
//! The desktop build keeps them in `states/<rom file name>/` in the user's
//! data folder: `<slot>.state` holds the snapshot and `<slot>.png` the screen
//! at that moment. A state that can't be loaded is renamed to
//! `<slot>.kept-<number>.state`, so saving to the slot again can't overwrite
//! it. The browser build has no save states yet.

use iced::time::SystemTime;
use iced::widget::image;
use nubo_core::Model;
use std::fmt;
use std::path::Path;

/// Whether this build can keep save states.
pub const AVAILABLE: bool = cfg!(not(target_arch = "wasm32"));

/// A place to keep one save state of a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// Written when the player leaves a game, and read when it starts again.
    /// Only used while "Continue games where you left off" is on (see
    /// `Settings::resume_games`).
    Resume,
    /// The slot the quick save and quick load keys use.
    Quick,
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
    pub model: Model,
    pub data: Vec<u8>,
}

/// What the menu shows about a slot.
#[derive(Debug, Clone)]
pub struct SlotInfo {
    pub slot: Slot,
    /// When the slot was saved, or `None` if it is empty.
    pub saved_at: Option<SystemTime>,
    pub thumbnail: Option<image::Handle>,
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

/// Reads the state in `slot` of the game at `rom_path`, or `None` if the slot
/// is empty.
pub fn load(rom_path: &Path, slot: Slot) -> Result<Option<SaveState>, String> {
    store::load(rom_path, slot).map_err(|error| format!("Couldn't read the save state: {error}"))
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

/// Describes each of `slots` of the game at `rom_path`.
pub fn slot_infos(rom_path: &Path, slots: &[Slot]) -> Vec<SlotInfo> {
    slots
        .iter()
        .map(|&slot| store::slot_info(rom_path, slot))
        .collect()
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

#[cfg_attr(target_arch = "wasm32", path = "save_states/no_store.rs")]
#[cfg_attr(not(target_arch = "wasm32"), path = "save_states/file_store.rs")]
mod store;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
