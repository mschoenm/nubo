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

/// Reading the command line with clap, which only the desktop build has.
#[cfg(not(target_arch = "wasm32"))]
mod parser {
    use super::{CaptureRequest, Options};
    use clap::error::ErrorKind;
    use clap::{CommandFactory, Parser, Subcommand};
    use nubo_core::Model;
    use nubo_frontend::settings::FastForwardSpeed;
    use std::ffi::OsString;
    use std::path::PathBuf;

    /// The largest `--scale`.
    const MAX_SCALE: u32 = 8;

    /// What the command line asked for.
    #[derive(Debug, Clone, PartialEq)]
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
        /// `capture`: show a scene and take a picture of the window.
        Capture(CaptureRequest),
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

        /// Show the Game Boy screen as the emulator draws it, without the
        /// settings' palette and screen effects
        #[arg(long)]
        raw_screen: bool,

        /// Let Ctrl+F12 save the moment on screen as a scene in this folder,
        /// for the documentation's screenshots
        #[arg(
            long,
            value_name = "FOLDER",
            hide = true,
            conflicts_with_all = ["scale", "raw_screen", "fresh"],
        )]
        record_scenes: Option<PathBuf>,

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
        /// Show a scene and save a picture of the window, for the
        /// documentation's screenshots
        #[command(hide = true)]
        Capture {
            /// The scene's folder (with scene.toml)
            scene: PathBuf,

            /// The folder with the scene's games, by their file names
            #[arg(long, value_name = "FOLDER")]
            games: PathBuf,

            /// A folder for this run's settings and data, instead of the
            /// player's
            #[arg(long, value_name = "FOLDER")]
            home: PathBuf,

            /// Change a setting from the scene's, such as appearance=dark
            /// (may be given more than once)
            #[arg(long = "set", value_name = "KEY=VALUE", value_parser = parse_change)]
            changes: Vec<(String, String)>,

            /// The window's inside in logical pixels, such as 1000x780,
            /// instead of the scene's size
            #[arg(long, value_name = "WxH", value_parser = parse_size)]
            size: Option<(f32, f32)>,

            /// Fill the screen
            #[arg(long, conflicts_with = "scale")]
            fullscreen: bool,

            /// Show only the game's picture, this many times its size (1 to 8)
            #[arg(
                long,
                value_name = "N",
                conflicts_with = "size",
                value_parser = clap::value_parser!(u32).range(1..=i64::from(MAX_SCALE)),
            )]
            scale: Option<u32>,

            /// Open this page of the pause menu instead of the scene's: main,
            /// save, load, cheats, super-game-boy, or none
            #[arg(long, value_name = "PAGE")]
            pause_menu: Option<String>,

            /// Open this settings tab instead of the scene's: display,
            /// playing, controls, library, about, or none
            #[arg(long, value_name = "TAB")]
            settings_tab: Option<String>,

            /// Save the window's picture here as PNG and quit; without it,
            /// print "ready" once the scene shows and keep showing it
            #[arg(long, value_name = "PNG")]
            output: Option<PathBuf>,
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
                "a command comes first and takes only its own options",
            ));
        }
        Ok(match arguments.command {
            Some(Command::MeasureEfficiency { rom, cgb, model }) => Request::MeasureEfficiency {
                rom_path: absolute(rom),
                model: chosen_model(cgb, model).unwrap_or(Model::Dmg),
            },
            Some(Command::Capture {
                scene,
                games,
                home,
                changes,
                size,
                fullscreen,
                scale,
                pause_menu,
                settings_tab,
                output,
            }) => Request::Capture(CaptureRequest {
                scene: absolute(scene),
                games: absolute(games),
                home: absolute(home),
                changes,
                size,
                fullscreen,
                scale,
                pause_menu,
                settings_tab,
                output: output.map(absolute),
            }),
            None => Request::Play(Options {
                rom_path: arguments.rom.map(absolute),
                model: chosen_model(arguments.cgb, arguments.model),
                scale: arguments.scale,
                fresh: arguments.fresh,
                speed: arguments.speed,
                raw_screen: arguments.raw_screen,
                record_scenes: arguments.record_scenes.map(absolute),
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
                || self.raw_screen
                || self.record_scenes.is_some()
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

    /// Reads a value of `capture`'s `--set`: a setting's key and its new
    /// value, written as `key=value`.
    fn parse_change(value: &str) -> Result<(String, String), String> {
        nubo_frontend::scenes::parse_change(value)
    }

    /// Reads the value of `capture`'s `--size`: a width and a height in
    /// logical pixels, written as `1000x780`.
    fn parse_size(value: &str) -> Result<(f32, f32), String> {
        let mistake = || format!("{value} isn't a size such as 1000x780");
        let (width, height) = value.split_once(['x', 'X']).ok_or_else(mistake)?;
        let length = |text: &str| -> Result<f32, String> {
            match text.trim().parse::<u32>() {
                Ok(length) if (1..=16_384).contains(&length) => Ok(length as f32),
                _ => Err(mistake()),
            }
        };
        Ok((length(width)?, length(height)?))
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
