//! The errors the public API can return.
//!
//! Everything a player hands the core (ROMs, zip files, save states, cheat
//! codes) may be broken or made up, so it must never make the core panic:
//! the public API returns one of these errors instead.

use crate::model::Model;
use std::error::Error;
use std::fmt;

/// The reason a ROM could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadRomError {
    /// The file is too small to hold a cartridge header.
    TooSmall,
    /// The cartridge uses hardware that Nubo does not emulate.
    /// Holds the cartridge type as written in Pan Docs, such as "MBC7".
    UnsupportedCartridge(&'static str),
    /// The cartridge header announces a RAM size that doesn't exist. Holds the
    /// RAM size byte from the header (at 0x0149).
    InvalidRamSize(u8),
    /// The file is a zip archive that could not be read.
    /// Holds a description of what went wrong.
    InvalidArchive(String),
}

impl fmt::Display for LoadRomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooSmall => write!(f, "The file is too small to contain a valid ROM."),
            Self::UnsupportedCartridge(cartridge_type) => write!(
                f,
                "Sorry, but Nubo currently only supports ROM-only, MBC1, MBC2, MBC3, MBC5, \
                 or HuC1 cartridges. The cartridge you provided is of type {cartridge_type}."
            ),
            Self::InvalidRamSize(ram_size) => {
                write!(
                    f,
                    "The ROM header has an invalid RAM size: {ram_size:#04X}."
                )
            }
            Self::InvalidArchive(reason) => write!(f, "Could not read the zip file: {reason}"),
        }
    }
}

impl Error for LoadRomError {}

/// The reason a save state could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveStateError {
    /// The data is not a Nubo save state, or it is damaged or cut off.
    InvalidFormat,
    /// The save state is in another save state format: an older one, from
    /// before values were stored with their names, or a newer one this
    /// version can't read.
    IncompatibleVersion {
        /// The format version of the save state.
        found: u8,
        /// The format version this Nubo uses.
        expected: u8,
    },
    /// The save state was made while playing a different game.
    DifferentGame {
        /// The title of the game the save state was made with.
        found: String,
        /// The title of the game that is loaded now.
        expected: String,
    },
    /// The save state was made while playing another version of the game: a
    /// game with the same title but another checksum in its cartridge header,
    /// such as another revision or a patched copy. Its code and data can be
    /// elsewhere, so the state could crash the game or spoil its own save.
    DifferentGameVersion {
        /// The title of the game.
        title: String,
    },
    /// The save state was made on different Game Boy hardware.
    DifferentModel {
        /// The model the save state was made on.
        found: Model,
        /// The model being emulated now.
        expected: Model,
    },
}

impl fmt::Display for SaveStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat => {
                write!(f, "The provided save state file is in an invalid format.")
            }
            Self::IncompatibleVersion { found, expected } if found > expected => write!(
                f,
                "The save state was made by a newer version of Nubo, which stores save states \
                 in a format this version can't read. Update Nubo to load it. Save state \
                 format: {found}, this version's format: {expected}."
            ),
            Self::IncompatibleVersion { found, expected } => write!(
                f,
                "The save state was made by an early version of Nubo, whose save states \
                 later versions can't read. Save state format: {found}, this version's \
                 format: {expected}."
            ),
            Self::DifferentGame { found, expected } => write!(
                f,
                "This save state is meant to be used for a different game. \
                 Save state game: '{found}', current game: '{expected}'."
            ),
            Self::DifferentGameVersion { title } => write!(
                f,
                "This save state was made with another version of '{title}', such as another \
                 revision or a patched copy of the game. Load the version it was made with."
            ),
            Self::DifferentModel { found, expected } => write!(
                f,
                "This save state was made on a {found}, but a {expected} is being emulated."
            ),
        }
    }
}

impl Error for SaveStateError {}

/// The reason a cheat code was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheatError {
    /// A GameShark code does not have exactly eight digits.
    InvalidGameSharkLength,
    /// A Game Genie code is not in the form `ABC-DEF` or `ABC-DEF-GHI`.
    InvalidGameGenieLength,
    /// Part of the code is not a hexadecimal number.
    InvalidDigits {
        /// Which part of the code is wrong, such as "address byte".
        field: &'static str,
        /// The text found in that part of the code.
        digits: String,
    },
    /// The code changes an address that this kind of cheat cannot change:
    /// GameShark codes change RAM (0xA000-0xDFFF), Game Genie codes change
    /// ROM (0x0000-0x7FFF).
    InvalidAddress(u16),
    /// The maximum number of cheats is already active.
    TooManyCheats {
        /// How many cheats can be active at once.
        limit: usize,
    },
}

impl fmt::Display for CheatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidGameSharkLength => {
                write!(f, "GameShark codes must be eight digits long.")
            }
            Self::InvalidGameGenieLength => {
                write!(f, "Game Genie codes must look like ABC-DEF or ABC-DEF-GHI.")
            }
            Self::InvalidDigits { field, digits } => write!(f, "Invalid {field}: {digits}"),
            Self::InvalidAddress(address) => write!(f, "Invalid address: {address:#06X}"),
            Self::TooManyCheats { limit } => {
                write!(f, "You cannot register more than {limit} cheats at a time.")
            }
        }
    }
}

impl Error for CheatError {}
