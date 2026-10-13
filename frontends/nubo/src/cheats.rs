//! Cheat codes: GameShark codes, which change a byte of RAM, and Game Genie
//! codes, which change a byte of the ROM. Each game keeps its own list, in
//! `cheats/<rom file name>.toml` in the user's data folder. The browser
//! build has no cheats yet.

use nubo_core::{Emulator, validate_gamegenie_code, validate_gameshark_code};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Whether this build can keep cheats.
pub const AVAILABLE: bool = cfg!(not(target_arch = "wasm32"));

/// Which kind of device a code is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheatKind {
    GameShark,
    GameGenie,
}

/// A cheat in a game's list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cheat {
    pub name: String,
    /// The code, in capitals, such as `01FF56D0` or `00A-17B-C49`. It also
    /// identifies the cheat to the emulator.
    pub code: String,
    pub enabled: bool,
}

impl Cheat {
    /// Checks a code the player typed and makes a cheat of it, named `name`
    /// (or after the code if the name is empty), or says what is wrong with
    /// the code.
    pub fn new(name: &str, code: &str) -> Result<Cheat, String> {
        let code = normalize(code);
        kind_of(&code)?;

        let name = name.trim();
        Ok(Cheat {
            name: if name.is_empty() {
                code.clone()
            } else {
                name.to_owned()
            },
            code,
            enabled: true,
        })
    }

    /// The kind of code, which was checked when the cheat was made.
    pub fn kind(&self) -> CheatKind {
        kind_of(&self.code).unwrap_or(CheatKind::GameShark)
    }

    /// Turns the cheat on or off in `emulator`, as `enabled` says. The code
    /// was checked when the cheat was made, so turning it on doesn't fail.
    pub fn apply_to(&self, emulator: &mut Emulator) {
        emulator.unregister_cheat(&self.code);

        if self.enabled {
            let _ = match self.kind() {
                CheatKind::GameShark => emulator.register_gameshark_cheat(&self.code, &self.code),
                CheatKind::GameGenie => emulator.register_gamegenie_cheat(&self.code, &self.code),
            };
        }
    }
}

/// Puts a code in the form the emulator expects: capitals without spaces,
/// and Game Genie codes typed without dashes get them (`00A17B` becomes
/// `00A-17B`).
fn normalize(code: &str) -> String {
    let code: String = code
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>()
        .to_uppercase();

    let is_hex = code.chars().all(|character| character.is_ascii_hexdigit());
    match code.len() {
        6 if is_hex => format!("{}-{}", &code[..3], &code[3..]),
        9 if is_hex => format!("{}-{}-{}", &code[..3], &code[3..6], &code[6..]),
        _ => code,
    }
}

/// Tells a GameShark code (eight hex digits) from a Game Genie code (six or
/// nine hex digits, usually with dashes), or says why it is neither.
fn kind_of(code: &str) -> Result<CheatKind, String> {
    if code.is_empty() {
        return Err(String::from("Type a GameShark or Game Genie code"));
    }

    let gameshark = validate_gameshark_code(code);
    if gameshark.is_ok() {
        return Ok(CheatKind::GameShark);
    }

    let gamegenie = validate_gamegenie_code(code);
    if gamegenie.is_ok() {
        return Ok(CheatKind::GameGenie);
    }

    // Report the error of the kind the code looks most like.
    let error = if code.contains('-') {
        gamegenie.err().map(|error| error.to_string())
    } else {
        gameshark.err().map(|error| error.to_string())
    };
    Err(error.unwrap_or_else(|| String::from("That isn't a valid code")))
}

/// The cheats of the game at `rom_path`; none if it has no list yet.
pub fn load(rom_path: &Path) -> Result<Vec<Cheat>, String> {
    store::load(rom_path).map_err(|error| format!("The cheats couldn't be read: {error}"))
}

/// Saves the cheats of the game at `rom_path`.
pub fn save(rom_path: &Path, cheats: &[Cheat]) -> Result<(), String> {
    store::save(rom_path, cheats).map_err(|error| format!("The cheats couldn't be saved: {error}"))
}

#[cfg_attr(target_arch = "wasm32", path = "cheats/no_store.rs")]
#[cfg_attr(not(target_arch = "wasm32"), path = "cheats/file_store.rs")]
mod store;
#[cfg(test)]
mod tests;
