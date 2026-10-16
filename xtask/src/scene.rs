//! `cargo xtask scene`: makes scenes (`nubo_frontend::scenes`) without
//! playing by hand, for screenshots of the documentation (see
//! `docs/SCREENSHOTS_PLAN.md`).
//!
//! `explore` plays a game headless, from power-on or from a scene's state,
//! pressing buttons as an input script says (`script`), and saves candidate
//! scenes where the script marks them and every so often: each with its
//! save state and a preview of the game's picture, run from that state the
//! way the app's `capture` command runs it, with the default settings. A
//! contact sheet (`sheet`) shows them all, numbered. Each run goes into a
//! new folder in `target/scenes/`, and any candidate can be the start of
//! another run. `keep` copies a candidate into `images/scenes/` under a
//! name, for `images/screenshots.toml` to use.
//!
//! The games come from `images/scenes/games.toml`, as for the screenshots,
//! downloaded into `target/screenshots/games/`.

use crate::screenshots::lists::GameList;
use crate::screenshots::{GAMES_FILE, SCENES_FOLDER};
use crate::{Result, target_dir, workspace_root};
use nubo_core::{Emulator, Key, Model, NoSaves, RtcClock};
use nubo_frontend::lcd::ScreenEffects;
use nubo_frontend::model::ModelPreference;
use nubo_frontend::palettes::DmgPalette;
use nubo_frontend::picture::Frame;
use nubo_frontend::scenes::{self, Scene, SceneGame};
use script::Step;
use std::fs;
use std::path::{Path, PathBuf};

/// What `cargo xtask scene` does.
#[derive(clap::Subcommand)]
pub enum Command {
    /// Play a game headless with an input script and save candidate scenes in a new folder in
    /// target/scenes, with a contact sheet of them
    Explore(ExploreArguments),
    /// Copy a candidate scene into images/scenes under a name
    Keep {
        /// The candidate's folder
        candidate: PathBuf,
        /// The scene's name, such as tobu-tobu-pause-menu
        name: String,
    },
}

/// The options of `cargo xtask scene explore`.
#[derive(clap::Args)]
pub struct ExploreArguments {
    /// A game from images/scenes/games.toml by its file name, or a scene's folder to go on
    /// from
    start: String,
    /// The input script, such as "wait 5s; press start; hold right; wait 2s; mark" (see
    /// xtask/src/scene/script.rs)
    #[arg(long, default_value = "")]
    script: String,
    /// Read the input script from this file
    #[arg(long, value_name = "FILE", conflicts_with = "script")]
    script_file: Option<PathBuf>,
    /// Save a candidate this often too, such as 2s
    #[arg(long, value_name = "TIME", value_parser = script::parse_frames)]
    every: Option<u32>,
    /// The hardware model for a game started from power-on, instead of the one the app picks
    /// for it (dmg, mgb, sgb, sgb2, cgb, agb and the others the app knows)
    #[arg(long, value_name = "NAME")]
    model: Option<Model>,
}

/// Does what `command` asks.
pub fn run(command: Command) -> Result {
    match command {
        Command::Explore(arguments) => explore(&arguments),
        Command::Keep { candidate, name } => keep(&candidate, &name),
    }
}

/// A game being explored.
struct Exploration {
    emulator: Emulator,
    rom: Vec<u8>,
    game: SceneGame,
    /// Where the candidates go.
    folder: PathBuf,
    /// The candidates saved so far, with their numbers.
    candidates: Vec<(u32, image::RgbaImage)>,
    /// The frames run since the start.
    frames: u32,
    /// The frame the last candidate was saved at.
    last_candidate: Option<u32>,
}

/// `cargo xtask scene explore` (see the module's documentation).
fn explore(arguments: &ExploreArguments) -> Result {
    let script = match &arguments.script_file {
        Some(path) => fs::read_to_string(path)
            .map_err(|error| format!("couldn't read {}: {error}", path.display()))?,
        None => arguments.script.clone(),
    };
    let steps = script::parse(&script)?;
    let mut exploration = Exploration::start(&arguments.start, arguments.model)?;
    println!(
        "Exploring {} on the {} into {}",
        exploration.game.file,
        exploration.game.model,
        exploration.folder.display()
    );

    for step in steps {
        match step {
            Step::Wait(frames) => {
                for _ in 0..frames {
                    exploration.run_frame();
                    if arguments
                        .every
                        .is_some_and(|every| every > 0 && exploration.frames % every == 0)
                    {
                        exploration.save_candidate()?;
                    }
                }
            }
            Step::Press(keys) => keys
                .into_iter()
                .for_each(|key| exploration.emulator.press_key(key)),
            Step::Release(keys) if keys.is_empty() => {
                for key in [
                    Key::Up,
                    Key::Down,
                    Key::Left,
                    Key::Right,
                    Key::A,
                    Key::B,
                    Key::Start,
                    Key::Select,
                ] {
                    exploration.emulator.release_key(key);
                }
            }
            Step::Release(keys) => keys
                .into_iter()
                .for_each(|key| exploration.emulator.release_key(key)),
            Step::Mark => exploration.save_candidate()?,
        }
    }
    if exploration.last_candidate != Some(exploration.frames) {
        exploration.save_candidate()?;
    }

    let sheet = sheet::draw(&exploration.candidates);
    let sheet_path = exploration.folder.join("contact-sheet.png");
    sheet
        .save(&sheet_path)
        .map_err(|error| format!("couldn't write {}: {error}", sheet_path.display()))?;
    let count = exploration.candidates.len();
    println!(
        "Saved {count} {}; the contact sheet is {}",
        if count == 1 {
            "candidate"
        } else {
            "candidates"
        },
        sheet_path.display()
    );
    Ok(())
}

impl Exploration {
    /// Starts the game `start` names (from power-on, on `model` or the one
    /// the app picks), or the game of the scene in the folder `start`
    /// names (from its state), with a new folder for the candidates.
    fn start(start: &str, model: Option<Model>) -> Result<Exploration> {
        let games = GameList::read(&workspace_root().join(GAMES_FILE))?;
        let scene_folder = Path::new(start);
        let (game, state, rom) = if scene_folder.join(scenes::SCENE_FILE).is_file() {
            let scene = Scene::read(scene_folder)?;
            let game = scene
                .game
                .ok_or("a scene of the library has no game to go on with")?;
            if model.is_some() {
                return Err(String::from(
                    "a scene goes on on its own model; leave out --model",
                ));
            }
            let state = fs::read(scene_folder.join(scenes::STATE_FILE))
                .map_err(|error| format!("couldn't read the scene's state: {error}"))?;
            let rom = fs::read(download(&games, &game.file)?).map_err(|error| error.to_string())?;
            if nubo_download::sha256_hex(&rom) != game.sha256 {
                return Err(format!(
                    "{GAMES_FILE} downloads another {} than the scene was made with",
                    game.file
                ));
            }
            (game, Some(state), rom)
        } else {
            let entry = games.game(start).ok_or_else(|| {
                format!("{start} is neither a scene's folder nor a game in {GAMES_FILE}")
            })?;
            let rom =
                fs::read(download(&games, &entry.file)?).map_err(|error| error.to_string())?;
            let header = Emulator::read_rom_header(&rom).map_err(|error| error.to_string())?;
            let model = model.unwrap_or_else(|| ModelPreference::Automatic.model_for(&header));
            let game = SceneGame {
                file: entry.file.clone(),
                sha256: entry.sha256.clone(),
                model: model.name().to_owned(),
                frames: scenes::RECORDED_FRAMES,
            };
            (game, None, rom)
        };

        let emulator = new_emulator(&rom, game.model()?, state.as_deref())?;
        let folder = free_folder(&target_dir().join("scenes"), &game.file);
        fs::create_dir_all(&folder)
            .map_err(|error| format!("couldn't create {}: {error}", folder.display()))?;
        Ok(Exploration {
            emulator,
            rom,
            game,
            folder,
            candidates: Vec::new(),
            frames: 0,
            last_candidate: None,
        })
    }

    /// Runs one frame, as the app's `capture` command runs them.
    fn run_frame(&mut self) {
        let (mut left, mut right) = (Vec::new(), Vec::new());
        self.emulator.run_frame(&mut left, &mut right);
        self.frames += 1;
    }

    /// Saves the moment as the next candidate scene: its state, its
    /// `scene.toml` with the default settings, and the game's picture run
    /// from the state as its preview.
    fn save_candidate(&mut self) -> Result {
        let number = u32::try_from(self.candidates.len()).unwrap_or(u32::MAX) + 1;
        let state = self.emulator.save_state();
        let preview = preview(&self.rom, self.game.model()?, &state, self.game.frames)?;
        let mut png = Vec::new();
        preview
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|error| error.to_string())?;

        let scene = Scene {
            game: Some(self.game.clone()),
            window: None,
            view: scenes::View::default(),
            library: None,
            settings: toml::Table::new(),
        };
        let folder = self.folder.join(format!("{number:02}"));
        scene.write(&folder, Some(&state), Some(&png))?;
        println!("  {number:02}: {:.1} s in", script::seconds(self.frames));
        self.candidates.push((number, preview));
        self.last_candidate = Some(self.frames);
        Ok(())
    }
}

/// A new emulator running `rom` on `model`, from `state` if given, keeping
/// no saves and with the cartridge clock on emulated time, as the app's
/// `capture` command runs a scene.
fn new_emulator(rom: &[u8], model: Model, state: Option<&[u8]>) -> Result<Emulator> {
    let mut emulator = Emulator::new(model);
    emulator
        .load_rom(
            rom,
            Box::new(NoSaves {
                rtc_clock: RtcClock::EmulatedTime,
            }),
        )
        .map_err(|error| error.to_string())?;
    if let Some(state) = state {
        emulator
            .load_state(state)
            .map_err(|error| error.to_string())?;
    }
    Ok(emulator)
}

/// The game's picture `frames` frames after `state`, as the app shows it
/// with the default settings: with the original screen look (see
/// `lcd::ScreenEffects`), in plain grays on the original Game Boy, and with
/// a Super Game Boy's border. The last frame is blended with the one before
/// it as the app's `capture` command blends them, from the frames it runs
/// alone.
fn preview(rom: &[u8], model: Model, state: &[u8], frames: u32) -> Result<image::RgbaImage> {
    let mut emulator = new_emulator(rom, model, Some(state))?;
    let (mut left, mut right) = (Vec::new(), Vec::new());
    let mut finished: Vec<Frame> = Vec::new();
    for _ in 0..frames {
        emulator.run_frame(&mut left, &mut right);
        if emulator.take_frame().is_some() {
            finished.push(Frame::latest(&emulator));
        }
    }
    let mut frame = finished.pop().unwrap_or_else(|| Frame::latest(&emulator));
    let previous = finished.pop();
    ScreenEffects::of(model, true).apply(
        &mut frame,
        previous.as_ref().map(|frame| frame.pixels.as_slice()),
    );
    let picture = frame.shown(DmgPalette::default(), true);
    image::RgbaImage::from_raw(picture.width(), picture.height(), picture.pixels().to_vec())
        .ok_or_else(|| String::from("the game's picture has the wrong size"))
}

/// Downloads the game saved as `file` (see `screenshots`), returning
/// where it is.
fn download(games: &GameList, file: &str) -> Result<PathBuf> {
    let game = games
        .game(file)
        .ok_or_else(|| format!("{GAMES_FILE} doesn't say where to download {file}"))?;
    let folder = target_dir().join("screenshots").join("games");
    fs::create_dir_all(&folder)
        .map_err(|error| format!("couldn't create {}: {error}", folder.display()))?;
    let path = folder.join(file);
    crate::screenshots::download_game(game, &path)?;
    Ok(path)
}

/// A folder in `parent` named after the game's file that doesn't exist
/// yet, such as `tobu-tobu-girl-deluxe-2`.
fn free_folder(parent: &Path, file: &str) -> PathBuf {
    let stem = Path::new(file)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let base: String = stem
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    (1..)
        .map(|number| parent.join(format!("{}-{number}", base.trim_matches('-'))))
        .find(|folder| !folder.exists())
        .expect("some number is free")
}

/// `cargo xtask scene keep`: copies the candidate's `scene.toml`, state and
/// preview into `images/scenes/<name>/`, which mustn't exist yet.
fn keep(candidate: &Path, name: &str) -> Result {
    Scene::read(candidate)?;
    if name.is_empty()
        || !name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
    {
        return Err(String::from(
            "a scene's name has lowercase letters, digits and dashes only",
        ));
    }
    let folder = workspace_root().join(SCENES_FOLDER).join(name);
    if folder.exists() {
        return Err(format!("{} exists already", folder.display()));
    }
    fs::create_dir_all(&folder)
        .map_err(|error| format!("couldn't create {}: {error}", folder.display()))?;
    for file in [scenes::SCENE_FILE, scenes::STATE_FILE, scenes::PREVIEW_FILE] {
        let source = candidate.join(file);
        if source.exists() {
            fs::copy(&source, folder.join(file))
                .map_err(|error| format!("couldn't copy {}: {error}", source.display()))?;
        }
    }
    println!("Kept the scene as {}", folder.display());
    Ok(())
}

mod script;
mod sheet;
#[cfg(test)]
mod tests;
