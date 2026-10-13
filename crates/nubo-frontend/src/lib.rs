//! Everything a Nubo app needs that isn't drawing, shared by the apps so
//! each only has to draw and handle input (see `docs/RESTRUCTURE_PLAN.md`).
//!
//! It doesn't depend on Iced or any other UI toolkit. For now it holds what
//! the RetroArch core needs too: the palettes for original Game Boy games
//! ([`palettes`]) and the rule that picks the hardware a game starts on
//! ([`model`]). It also holds the neutral building blocks the Iced app's
//! other parts already use until they move here: clocks that work in the
//! browser too ([`time`]) and pictures as plain RGBA pixels ([`picture`]).

pub mod model;
pub mod palettes;
pub mod picture;
pub mod time;
