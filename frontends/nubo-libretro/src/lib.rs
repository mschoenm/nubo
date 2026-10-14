//! Nubo as a libretro core: a dynamic library that RetroArch, and other
//! frontends of the libretro interface, load to run Game Boy and Game Boy
//! Color games. RetroArch brings what the core doesn't have to: save state
//! slots, rewind, run-ahead, netplay, shaders, RetroAchievements, the cheat
//! list, recording and button remapping.
//!
//! The crate is split in two so that the `unsafe` code it needs stays small:
//!
//! - [`core`]: everything the core does, as a safe [`core::Core`]: loading a
//!   game, running a frame, converting the picture and the sound, the
//!   buttons, battery saves, save states, cheats and the memory
//!   RetroAchievements reads; [`options`]: the core options.
//! - `ffi`: the C functions RetroArch calls (`retro_run` and the rest of
//!   `libretro.h`), the callbacks it hands over, and turning its pointers
//!   into slices and strings. It is the only module with `unsafe` code.
//!
//! `docs/RETROARCH_PLAN.md` describes the plan this follows, and `AGENTS.md`
//! next to this crate's `Cargo.toml` how to build and test it.

pub mod core;
mod ffi;
pub mod options;
#[cfg(test)]
mod test_roms;
