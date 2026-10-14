//! Everything a Nubo app needs that isn't drawing, shared by the apps so
//! each only has to draw and handle input (see `docs/RESTRUCTURE_PLAN.md`).
//!
//! It doesn't depend on Iced or any other UI toolkit. It holds:
//!
//! - what the RetroArch core needs too: the palettes for original Game Boy
//!   games ([`palettes`]), the Super Game Boy menu's choices ([`sgb_menu`])
//!   and the rule that picks the hardware a game starts on ([`model`]);
//! - neutral building blocks: clocks that work in the browser too ([`time`]),
//!   pictures as plain RGBA pixels ([`picture`]), slow work on a thread of its
//!   own ([`background`]) and problems for the app to show ([`notices`]);
//! - what an app keeps between runs, through [`storage`]: the settings
//!   ([`settings`], with the key bindings of [`controls`]), battery saves
//!   ([`battery`]), save states ([`save_states`]), cheats ([`cheats`]) and
//!   screenshots ([`screenshots`]), and the ROM files themselves ([`rom`]);
//! - the game library ([`library`]): the games in the player's folders,
//!   scanned in the background and cached between runs with pictures of
//!   their title screens, their last screen played and their box art;
//! - running a game (`session`): the emulator with its sound, paced by the
//!   sound card on the desktop and by the screen's refreshes in the
//!   browser, and what an app shows and keeps of the game (playback,
//!   cheats, palette, save state slots and the picture on screen);
//! - game controllers (`gamepads`), combined into one controller state.
//!
//! # Features
//!
//! Each platform's storage and sound, and the controllers, are behind
//! features, so a build only takes the dependencies it can use:
//!
//! - **`desktop`**: files in the user's folders (`storage::files`) and the
//!   desktop's sound (cpal). Save states, cheats, screenshots and the
//!   library are only kept with it, and only it watches the library
//!   folders and downloads box art.
//! - **`web`**: the browser's localStorage (`storage::local_storage`), which
//!   keeps the settings and battery saves, and the browser's sound (cpal's
//!   WebAudio backend).
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
#[cfg(feature = "gamepads")]
pub mod gamepads;
pub mod library;
pub mod model;
pub mod notices;
pub mod palettes;
pub mod picture;
pub mod rom;
pub mod save_states;
pub mod screenshots;
#[cfg(any(feature = "desktop", feature = "web"))]
pub mod session;
pub mod settings;
pub mod sgb_menu;
pub mod storage;
pub mod time;
