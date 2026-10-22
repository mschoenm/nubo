//! Everything a Nubo app needs that isn't drawing, shared by the apps so
//! each only has to draw and handle input (see `docs/RESTRUCTURE_PLAN.md`).
//!
//! It doesn't depend on Iced or any other UI toolkit. It holds:
//!
//! - what the RetroArch core needs too: the palettes for original Game Boy
//!   games ([`palettes`]), the Super Game Boy menu's choices ([`sgb_menu`])
//!   and the rule that picks the hardware a game starts on ([`model`]);
//! - neutral building blocks: clocks that work in the browser too ([`time`]),
//!   opening web pages and folders ([`links`]), the font the player picked
//!   for their desktop (`desktop_font`), the looks of the Game Boy's
//!   own screen ([`lcd`]), whether a newer Nubo is out ([`updates`]),
//!   pictures as plain RGBA pixels ([`picture`]), slow work on a thread of
//!   its own ([`background`]) and problems for the app to show
//!   ([`notices`]);
//! - what an app keeps between runs, through [`storage`]: the settings
//!   ([`settings`], with the key bindings of [`controls`]), battery saves
//!   ([`battery`]), save states ([`save_states`]), cheats ([`cheats`]) and
//!   screenshots ([`screenshots`]), and the ROM files themselves ([`rom`]);
//! - the game library ([`library`]): the games in the player's folders (in
//!   the browser, the games the player added), scanned in the background
//!   and cached between runs with pictures of their title screens, their
//!   last screen played and their box art;
//! - running a game (`session`): the emulator with its sound, paced by the
//!   sound card on the desktop and by the screen's refreshes in the
//!   browser, and what an app shows and keeps of the game (playback,
//!   cheats, palette, save state slots and the picture on screen);
//! - game controllers (`gamepads`), combined into one controller state;
//! - scenes ([`scenes`]): recorded moments of a game, which the
//!   documentation's screenshots are made from again.
//!
//! # Features
//!
//! Each platform's storage and sound, and the controllers, are behind
//! features, so a build only takes the dependencies it can use:
//!
//! - **`desktop`**: files in the user's folders (`storage::files`), the
//!   desktop's sound (cpal) and, on Linux, the desktop's font, read from the
//!   desktop portal (`desktop_font`). Only it watches the library folders.
//! - **`web`**: the browser's localStorage (`storage::local_storage`), which
//!   keeps the settings, battery saves and cheats, its IndexedDB
//!   (`storage::indexed_db`), which keeps save states, the library and the
//!   games' files, and the browser's sound (cpal's WebAudio backend).
//!   Screenshots become downloads.
//! - **`gamepads`**: game controllers, read with gilrs.
//!
//! Without `desktop` or `web`, nothing is kept ([`storage::NoStorage`]) and
//! there is no `session`, since it needs a sound output; the RetroArch core
//! builds it that way, without any feature. With both, `desktop` wins for
//! storage.

pub mod background;
pub mod battery;
pub mod cheats;
pub mod controls;
#[cfg(feature = "desktop")]
pub mod desktop_font;
#[cfg(feature = "gamepads")]
pub mod gamepads;
pub mod lcd;
pub mod library;
pub mod links;
pub mod model;
pub mod notices;
pub mod palettes;
pub mod picture;
pub mod rom;
pub mod save_states;
pub mod scenes;
pub mod screenshots;
#[cfg(any(feature = "desktop", feature = "web"))]
pub mod session;
pub mod settings;
pub mod sgb_menu;
pub mod storage;
pub mod time;
pub mod updates;
