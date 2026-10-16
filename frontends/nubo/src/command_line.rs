//! The options the app takes on the command line, read with clap:
//!
//! ```text
//! nubo [rom] [--cgb | --model <name>] [--scale <n>] [--fresh] [--speed <n>] [--raw-screen]
//! nubo [rom] [--cgb | --model <name>] --record-scenes <folder>
//! nubo measure-efficiency <rom> [--cgb | --model <name>]
//! nubo capture <scene> --games <folder> --home <folder> [--set <key=value>]... [--size <WxH> | --fullscreen | --scale <n>]
//!              [--pause-menu <page>] [--settings-tab <tab>] [--output <png>]
//! nubo --help | --version
//! ```
//!
//! - `--cgb` and `--model <name>` start the game on that hardware model
//!   instead of the one the settings pick. Only one of them may be given,
//!   once.
//! - `--scale <n>` shows only the game screen, `n` times its size (1 to 8),
//!   in a window whose inside is exactly 160·n × 144·n pixels of the
//!   display, whatever its scaling (256·n × 224·n on a Super Game Boy, with
//!   its border). Nothing else can be brought up in it. It needs a ROM.
//! - `--fresh` starts like a new install and leaves nothing behind: default
//!   settings, no library, no battery saves, save states, cheats or
//!   screenshots, a cartridge clock that follows emulated time, and
//!   problems printed on stderr.
//! - `--speed <n>` starts the game fast forwarding at `n` times real time,
//!   one of the fast forward speeds the settings offer.
//! - `--raw-screen` shows the Game Boy screen as the emulator draws it,
//!   whatever palette and screen effects the settings pick: plain grays,
//!   no color correction or other screen effect
//!   (`nubo_frontend::settings::ScreenLook::RAW`) and a Super Game Boy
//!   game's own colors rather than ones the player picked. A Super Game Boy's
//!   border still shows or not as the settings say; it frames the screen
//!   without changing it, and the Shootout's harness crops it off.
//!
//! The `measure-efficiency` command opens no window: it measures how often
//! the emulator thread wakes up while the ROM plays (`efficiency`) and
//! prints it as JSON, for the nightly CI. `--help` leaves it out, as it
//! isn't for players. Since a first argument that names it is the command,
//! a ROM file named exactly `measure-efficiency` opens as
//! `./measure-efficiency`; the same goes for `capture`.
//!
//! Two more are for making the documentation's screenshots and are left out
//! of `--help` too (see `app::scenes` and `docs/SCREENSHOTS_PLAN.md`):
//!
//! - `--record-scenes <folder>` lets Ctrl+F12 save the moment on screen as a
//!   scene in that folder (`nubo_frontend::scenes`). It can't be combined
//!   with `--scale`, `--raw-screen` or `--fresh`, which change what the app
//!   shows, so the scene wouldn't show again as recorded.
//! - The `capture` command shows a scene with the scene's games from
//!   `--games` and its settings, changed by any `--set key=value`. It keeps
//!   its settings and data in `--home` instead of the player's folders, so
//!   it reads and writes nothing of the player's. The window's inside is
//!   `--size` (or the scene's size), or it fills the screen with
//!   `--fullscreen`, or it shows only the game's picture with `--scale`, as
//!   above. `--pause-menu` and `--settings-tab` open another page of the
//!   pause menu or another settings tab than the scene has open, or close
//!   it with `none`. With `--output` it saves a picture of the window there
//!   and quits; without, it prints `ready` once the scene shows and keeps
//!   showing it, for a harness that captures the window itself (with its
//!   border, `cargo xtask screenshots`).
//!
//! The GB Emulator Shootout's harness runs the app with `--model` and all
//! but `--cgb` (`docs/SHOOTOUT_PLAN.md`). `--raw-screen` makes its
//! screenshots judge the emulation alone, as its harness has other emulators
//! turn off their color correction and blending, while players get the
//! original screen look by default. The browser build gets no command line, so
//! it starts without any of them and doesn't include clap.

use nubo_core::Model;
use nubo_frontend::settings::FastForwardSpeed;
use std::path::PathBuf;

/// What the command line asked the app to start with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// The ROM to start, as an absolute path.
    pub rom_path: Option<PathBuf>,
    /// The model to run games on instead of the one the settings pick.
    pub model: Option<Model>,
    /// With `--scale`: show only the game screen, this many times its size.
    pub scale: Option<u32>,
    /// With `--fresh`: keep nothing and read nothing kept, like a new
    /// install.
    pub fresh: bool,
    /// With `--speed`: start the game fast forwarding at this speed.
    pub speed: Option<FastForwardSpeed>,
    /// With `--raw-screen`: show the Game Boy screen as the emulator draws
    /// it, without the settings' palette and screen effects.
    pub raw_screen: bool,
    /// With `--record-scenes`: Ctrl+F12 saves the moment on screen as a
    /// scene in this folder.
    pub record_scenes: Option<PathBuf>,
}

/// What the `capture` command asked for (see the module's documentation).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureRequest {
    /// The scene's folder.
    pub scene: PathBuf,
    /// The folder with the scene's games, by their file names.
    pub games: PathBuf,
    /// The folder the run keeps its settings and data in.
    pub home: PathBuf,
    /// Settings to change from the scene's, each a key and a value.
    pub changes: Vec<(String, String)>,
    /// The window's inside in logical pixels, instead of the scene's size.
    pub size: Option<(f32, f32)>,
    /// Whether the window fills the screen.
    pub fullscreen: bool,
    /// Show only the game's picture, this many times its size, as `--scale`
    /// does.
    pub scale: Option<u32>,
    /// The pause menu page to open instead of the scene's, or `none`.
    pub pause_menu: Option<String>,
    /// The settings tab to open instead of the scene's, or `none`.
    pub settings_tab: Option<String>,
    /// Where to save the window's picture, or `None` to keep showing the
    /// scene for a harness that captures the window itself.
    pub output: Option<PathBuf>,
}

#[cfg(not(target_arch = "wasm32"))]
pub use parser::{Request, parse};

#[cfg(not(target_arch = "wasm32"))]
mod parser;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
