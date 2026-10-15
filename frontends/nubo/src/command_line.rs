//! The options the app takes on the command line, read with clap:
//!
//! ```text
//! nubo [rom] [--cgb | --model <name>] [--scale <n>] [--fresh] [--speed <n>]
//! nubo measure-efficiency <rom> [--cgb | --model <name>]
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
//!
//! The `measure-efficiency` command opens no window: it measures how often
//! the emulator thread wakes up while the ROM plays (`efficiency`) and
//! prints it as JSON, for the nightly CI. `--help` leaves it out, as it
//! isn't for players. Since a first argument that names it is the command,
//! a ROM file named exactly `measure-efficiency` opens as
//! `./measure-efficiency`.
//!
//! `--scale`, `--fresh` and `--speed` are what the GB Emulator Shootout's
//! harness runs the app with (`docs/SHOOTOUT_PLAN.md`). The browser build
//! gets no command line, so it starts without any of them and doesn't
//! include clap.

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
}

#[cfg(not(target_arch = "wasm32"))]
pub use parser::{Request, parse};

/// Reading the command line with clap, which only the desktop build has.
#[cfg(not(target_arch = "wasm32"))]
mod parser {
    use super::Options;
    use clap::error::ErrorKind;
    use clap::{CommandFactory, Parser, Subcommand};
    use nubo_core::Model;
    use nubo_frontend::settings::FastForwardSpeed;
    use std::ffi::OsString;
    use std::path::PathBuf;

    /// The largest `--scale`.
    const MAX_SCALE: u32 = 8;

    /// What the command line asked for.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Request {
        /// Open the app's window.
        Play(Options),
        /// `measure-efficiency`: measure the emulator thread on a ROM and
        /// print the result, without a window.
        MeasureEfficiency {
            /// The game to measure.
            rom_path: PathBuf,
            /// The model to run it on: DMG unless `--cgb` or `--model` says
            /// otherwise.
            model: Model,
        },
    }

    /// The command line as clap reads it. The doc comments on its fields
    /// are the help that `--help` prints.
    #[derive(Parser, Debug)]
    #[command(
        name = "nubo",
        version,
        about = "Nubo, a Game Boy and Game Boy Color emulator",
        long_about = None,
        disable_help_subcommand = true,
    )]
    struct Arguments {
        /// The game to start (.gb, .gbc or .zip); without one, the library
        /// opens
        rom: Option<PathBuf>,

        /// Start the game on a Game Boy Color, the same as --model cgb
        #[arg(long, conflicts_with = "model")]
        cgb: bool,

        /// Start the game on this hardware model instead of the one the
        /// settings pick: dmg-0, dmg, mgb, sgb, sgb2, cgb-0 to cgb-e (or
        /// cgb), agb
        #[arg(long, value_name = "NAME")]
        model: Option<Model>,

        /// Show only the game screen, this many times its size (1 to 8)
        #[arg(
            long,
            value_name = "N",
            requires = "rom",
            value_parser = clap::value_parser!(u32).range(1..=i64::from(MAX_SCALE)),
        )]
        scale: Option<u32>,

        /// Start like a new install and leave nothing behind: default
        /// settings, no library, saves or screenshots
        #[arg(long)]
        fresh: bool,

        /// Start the game fast forwarding at this many times real time: 2,
        /// 3, 4 or 8
        #[arg(long, value_name = "N", value_parser = parse_speed)]
        speed: Option<FastForwardSpeed>,

        #[command(subcommand)]
        command: Option<Command>,
    }

    /// What the app can run instead of opening its window.
    #[derive(Subcommand, Debug)]
    enum Command {
        /// Measure how often the emulator thread wakes up while a game
        /// plays and print it as JSON, without a window
        #[command(hide = true)]
        MeasureEfficiency {
            /// The game to measure (.gb, .gbc or .zip)
            rom: PathBuf,

            /// Run it on a Game Boy Color, the same as --model cgb
            #[arg(long, conflicts_with = "model")]
            cgb: bool,

            /// Run it on this hardware model instead of the DMG
            #[arg(long, value_name = "NAME")]
            model: Option<Model>,
        },
    }

    /// Reads the command line's arguments, starting with the program's
    /// name. A mistake in them, or `--help` or `--version`, comes back as
    /// clap's error, whose `exit` prints it and ends the program.
    pub fn parse<T>(args: impl IntoIterator<Item = T>) -> Result<Request, clap::Error>
    where
        T: Into<OsString> + Clone,
    {
        let arguments = Arguments::try_parse_from(args)?;
        // clap's `args_conflicts_with_subcommands` would catch this too, but
        // it calls a second ROM a subcommand.
        if arguments.command.is_some() && arguments.opens_the_window() {
            return Err(Arguments::command().error(
                ErrorKind::ArgumentConflict,
                "measure-efficiency comes first and takes only its own options",
            ));
        }
        Ok(match arguments.command {
            Some(Command::MeasureEfficiency { rom, cgb, model }) => Request::MeasureEfficiency {
                rom_path: absolute(rom),
                model: chosen_model(cgb, model).unwrap_or(Model::Dmg),
            },
            None => Request::Play(Options {
                rom_path: arguments.rom.map(absolute),
                model: chosen_model(arguments.cgb, arguments.model),
                scale: arguments.scale,
                fresh: arguments.fresh,
                speed: arguments.speed,
            }),
        })
    }

    impl Arguments {
        /// Whether any of the arguments for opening the window is given.
        fn opens_the_window(&self) -> bool {
            self.rom.is_some()
                || self.cgb
                || self.model.is_some()
                || self.scale.is_some()
                || self.fresh
                || self.speed.is_some()
        }
    }

    /// The model `--cgb` or `--model` asks for, if either is given.
    fn chosen_model(cgb: bool, model: Option<Model>) -> Option<Model> {
        if cgb { Some(Model::CgbE) } else { model }
    }

    /// `path` as an absolute path. The library keeps the path and watches
    /// its folder, so it mustn't depend on the folder the app was started
    /// in.
    fn absolute(path: PathBuf) -> PathBuf {
        std::path::absolute(&path).unwrap_or(path)
    }

    /// Reads the value of `--speed`: one of the fast forward speeds.
    fn parse_speed(value: &str) -> Result<FastForwardSpeed, String> {
        let multiplier: u32 = value
            .parse()
            .map_err(|_| format!("{value} isn't a number"))?;
        FastForwardSpeed::try_from(multiplier)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
