//! Everything a Nubo app needs that isn't drawing, shared by the apps so
//! each only has to draw and handle input (see `docs/RESTRUCTURE_PLAN.md`).
//!
//! It doesn't depend on Iced or any other UI toolkit. It holds:
//!
//! - what the RetroArch core needs too: the palettes for original Game Boy
//!   games ([`palettes`]) and the rule that picks the hardware a game starts
//!   on ([`model`]);
//! - neutral building blocks: clocks that work in the browser too ([`time`]),
//!   pictures as plain RGBA pixels ([`picture`]), slow work on a thread of its
//!   own ([`background`]) and problems for the app to show ([`notices`]);
//! - what an app keeps between runs, through [`storage`]: the settings
//!   ([`settings`], with the key bindings of [`controls`]), battery saves
//!   ([`battery`]), save states ([`save_states`]), cheats ([`cheats`]) and
//!   screenshots ([`screenshots`]), and the ROM files themselves ([`rom`]).
//!
//! # Features
//!
//! Each platform's storage is behind a feature, so a build only takes the
//! dependencies it can use:
//!
//! - **`desktop`**: files in the user's folders (`storage::files`). Save
//!   states, cheats and screenshots are only kept with it.
//! - **`web`**: the browser's localStorage (`storage::local_storage`), which
//!   keeps the settings and battery saves.
//!
//! Without either, nothing is kept ([`storage::NoStorage`]); the RetroArch
//! core builds it that way. With both, `desktop` wins.

pub mod background;
pub mod battery;
pub mod cheats;
pub mod controls;
pub mod model;
pub mod notices;
pub mod palettes;
pub mod picture;
pub mod rom;
pub mod save_states;
pub mod screenshots;
pub mod settings;
pub mod storage;
pub mod time;
