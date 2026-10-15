//! The options the app takes on the command line:
//!
//! ```text
//! nubo [rom] [--cgb | --model <name>] [--scale <n>] [--fresh] [--speed <n>] [--raw-screen]
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
//! The GB Emulator Shootout's harness runs the app with `--model` and all
//! but `--cgb` (`docs/SHOOTOUT_PLAN.md`). `--raw-screen` makes its
//! screenshots judge the emulation alone, as its harness has other emulators
//! turn off their color correction and blending, while players get the
//! corrected colors by default. The browser build gets no command line, so
//! it starts without any of them.

use nubo_core::Model;
use nubo_frontend::settings::FastForwardSpeed;
use std::path::PathBuf;

/// How to use the options, printed with a mistake in them.
pub const USAGE: &str = "usage: nubo [rom] [--cgb | --model <name>] [--scale <1-8>] [--fresh] [--speed <2|3|4|8>] [--raw-screen]";

/// The largest `--scale`.
const MAX_SCALE: u32 = 8;

/// What the command line asked for.
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
}

/// Reads the options from the command line's arguments, without the
/// program's name. Returns what is wrong with them, such as a missing or
/// invalid value or an unknown option.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut options = Options::default();

    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));

        if matches!(arg.as_str(), "--cgb" | "--model") && options.model.is_some() {
            return Err(String::from(
                "give the model once, with --cgb or --model, not both",
            ));
        }

        match arg.as_str() {
            "--cgb" => options.model = Some(Model::CgbE),
            "--model" => options.model = Some(value()?.parse()?),
            "--scale" => options.scale = Some(parse_scale(&value()?)?),
            "--fresh" => options.fresh = true,
            "--speed" => options.speed = Some(parse_speed(&value()?)?),
            "--raw-screen" => options.raw_screen = true,
            option if option.starts_with("--") => {
                return Err(format!("{option} isn't an option"));
            }
            // The library keeps the path and watches its folder, so it
            // mustn't depend on the folder the app was started in.
            _ => options.rom_path = Some(std::path::absolute(&arg).unwrap_or(PathBuf::from(arg))),
        }
    }

    if options.scale.is_some() && options.rom_path.is_none() {
        return Err(String::from("--scale needs a ROM to show"));
    }

    Ok(options)
}

/// Reads the value of `--scale`: a whole number from 1 to `MAX_SCALE`.
fn parse_scale(value: &str) -> Result<u32, String> {
    value
        .parse()
        .ok()
        .filter(|scale| (1..=MAX_SCALE).contains(scale))
        .ok_or_else(|| format!("--scale needs a whole number from 1 to {MAX_SCALE}, not {value}"))
}

/// Reads the value of `--speed`: one of the fast forward speeds.
fn parse_speed(value: &str) -> Result<FastForwardSpeed, String> {
    let multiplier: u32 = value
        .parse()
        .map_err(|_| format!("--speed needs a number, not {value}"))?;
    FastForwardSpeed::try_from(multiplier)
}

#[cfg(test)]
mod tests;
