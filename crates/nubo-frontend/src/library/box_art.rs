//! Box art for the library, downloaded from the libretro thumbnail
//! collection (<https://thumbnails.libretro.com>), which names each picture
//! after the game's official No-Intro name, such as
//! `Tetris (World) (Rev 1).png`.
//!
//! Many ROM files are named that way already. For the others, the No-Intro
//! game lists from the libretro database give the official name of a file
//! name or of the ROM's CRC32 checksum. The lists are downloaded once.
//!
//! Only the desktop build (the `desktop` feature) downloads anything, and
//! only when the player turns box art on in the settings.

/// Whether this build can download box art: only the desktop build can.
pub const AVAILABLE: bool = cfg!(feature = "desktop");

#[cfg(feature = "desktop")]
pub use download::{GameList, find, load_game_list};
#[cfg(not(feature = "desktop"))]
pub use unavailable::{GameList, find, load_game_list};

#[cfg(feature = "desktop")]
mod download;
#[cfg(not(feature = "desktop"))]
mod unavailable;
