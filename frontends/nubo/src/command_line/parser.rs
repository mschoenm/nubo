//! Reading the command line with clap, which only the desktop build has.

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
