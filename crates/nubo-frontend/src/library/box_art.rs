//! Box art for the library, downloaded from the libretro thumbnail
//! collection (<https://thumbnails.libretro.com>), which names each picture
//! after the game's official No-Intro name, such as
//! `Tetris (World) (Rev 1).png`.
//!
//! Many ROM files are named that way already. For the others, the No-Intro
//! game lists from the libretro database give the official name of a file
//! name or of the ROM's CRC32 checksum. The lists are downloaded once.
//!
//! The desktop build (the `desktop` feature) downloads with `ureq` from
//! thumbnails.libretro.com. The browser build (the `web` feature) downloads
//! with the browser's `fetch` from the collection's copy on GitHub, since
//! browsers only let a page read pictures from servers that allow it. Both
//! download only when the player turns box art on in the settings.

pub use names::GameList;
/// Looks up the box art of a game and downloads it (see the store of this
/// build): on the desktop it blocks, in the browser it is a future.
pub use store::find;

/// Whether this build can download box art: the desktop and browser builds
/// can.
pub const AVAILABLE: bool = cfg!(any(feature = "desktop", feature = "web"));

/// Where this build downloads box art from, for the settings to tell the
/// player.
pub const SERVER: &str = if cfg!(feature = "desktop") {
    "thumbnails.libretro.com"
} else {
    "GitHub"
};

/// Reads the No-Intro game lists, downloading the ones that aren't kept
/// yet: on the desktop on a thread of its own, in the browser with its
/// `fetch`.
pub fn load_game_list() -> impl Future<Output = Result<GameList, String>> + use<> {
    #[cfg(not(all(feature = "web", not(feature = "desktop"))))]
    {
        let work = crate::background::run(store::load_game_list);
        async move { work.await.and_then(|result| result) }
    }
    #[cfg(all(feature = "web", not(feature = "desktop")))]
    store::load_game_list()
}

mod names;
#[cfg(feature = "desktop")]
#[path = "box_art/download.rs"]
mod store;
#[cfg(all(feature = "web", not(feature = "desktop")))]
#[path = "box_art/fetch.rs"]
mod store;
#[cfg(not(any(feature = "desktop", feature = "web")))]
#[path = "box_art/unavailable.rs"]
mod store;
