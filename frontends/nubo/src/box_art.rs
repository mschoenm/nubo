//! Box art for the library, downloaded from the libretro thumbnail
//! collection (<https://thumbnails.libretro.com>), which names each picture
//! after the game's official No-Intro name, such as
//! `Tetris (World) (Rev 1).png`.
//!
//! Many ROM files are named that way already. For the others, the No-Intro
//! game lists from the libretro database give the official name of a file
//! name or of the ROM's CRC32 checksum. The lists are downloaded once.
//!
//! Only the desktop build downloads anything, and only when the player
//! turns box art on in the settings.

/// Whether this build can download box art.
pub const AVAILABLE: bool = cfg!(not(target_arch = "wasm32"));

#[cfg(not(target_arch = "wasm32"))]
pub use download::{GameList, find, load_game_list};
#[cfg(target_arch = "wasm32")]
pub use unavailable::{GameList, find, load_game_list};

#[cfg(not(target_arch = "wasm32"))]
mod download;
#[cfg(target_arch = "wasm32")]
mod unavailable;
