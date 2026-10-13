//! Nubo's emulator core: a cycle-accurate Game Boy and Game Boy Color
//! emulator without any platform dependencies.
//!
//! [`Emulator`] is the entry point. Frontends choose the hardware when they
//! create it with [`Emulator::new`], give it a ROM with [`Emulator::load_rom`], call
//! [`Emulator::step_until_next_audio_buffer`] in a loop, play the returned
//! audio, and draw [`Emulator::take_frame`] on screen. Player input goes in
//! through [`Emulator::press_key`] and [`Emulator::set_controller_state`].

pub use crate::address_bus::cheats::{validate_gamegenie_code, validate_gameshark_code};
pub use crate::address_bus::{CartridgeEffects, CartridgeHeader, NoSaves, RtcClock, RtcState};
pub use crate::controller::{ControllerButton, ControllerMapping, ControllerState};
pub use crate::cpu::{BusActivityEntry, BusActivityKind, Registers};
pub use crate::emulator::{BUILD_COMMIT, Emulator, SaveStateHeader};
pub use crate::error::{CheatError, LoadRomError, SaveStateError};
pub use crate::gpu::DMG_SHADES;
pub use crate::joypad::Key;
pub use crate::model::Model;
pub use crate::rom_archive::extract_rom;

mod address_bus;
mod apu;
mod bios;
mod clock;
mod controller;
mod cpu;
mod emulator;
mod error;
mod gpu;
mod joypad;
mod model;
mod rom_archive;
mod serial;
mod serializable;
mod utils;
