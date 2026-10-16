//! `cargo xtask screenshots`: makes the screenshots the README and the
//! Flathub listing show from scenes (`nubo_frontend::scenes`), recorded
//! moments of a game kept in `images/scenes/`, so every one of them can be
//! made again with the newest Nubo and any settings (see
//! `docs/SCREENSHOTS_PLAN.md`).
//!
//! `images/screenshots.toml` lists the screenshots (`lists`): each names
//! its file, its scene, how much of the app it shows (the window's inside,
//! the window with GNOME's border, the whole screen, or the game's picture
//! alone), changes to the scene's settings, and maybe variants that go side
//! by side. `images/scenes/games.toml` says where to download each game the
//! scenes use, with its checksum and license.
//!
//! The task builds the app, downloads the games into
//! `target/screenshots/games/` and runs the app's hidden `capture` command
//! for each picture in a headless GNOME Shell (`gnome`), through XWayland,
//! so GNOME draws and captures the window's border. The app draws with
//! Iced's software renderer there, so a scene gives the same pixels every
//! time. Then the pictures of variants go side by side (`pictures`), and
//! each screenshot whose pixels changed is written; one whose pixels are
//! the same is left alone, so git only stores pictures that really changed.
//! With `--check` nothing in `images/` changes: the task says which
//! screenshots differ, and leaves the new pictures and their differences in
//! `target/screenshots/check/`.
//!
//! It runs on Linux only, with GNOME Shell, Xwayland, xdotool and D-Bus's
//! tools installed (it prints the `apt` line when they aren't), as root in
//! a container such as a cloud session or as a user on a Linux desktop.

use crate::{Result, cargo, run, target_dir, workspace_root};
use lists::{Frame, Game, GameList, ScreenshotList, Tile};
use nubo_core::{SGB_FRAME_HEIGHT, SGB_FRAME_WIDTH};
use nubo_frontend::scenes::Scene;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The list of screenshots, from the workspace's root.
pub const SCREENSHOTS_FILE: &str = "images/screenshots.toml";
/// The folder of the scenes, from the workspace's root.
pub const SCENES_FOLDER: &str = "images/scenes";
/// The list of the games the scenes use, from the workspace's root.
pub const GAMES_FILE: &str = "images/scenes/games.toml";

/// The size of the screen a fullscreen screenshot fills, unless it says.
const FULLSCREEN_SIZE: (u32, u32) = (1920, 1080);
/// How many times its size the game's picture alone is shown, unless a
/// screenshot says.
const GAME_SCALE: u32 = 4;

/// The options of `cargo xtask screenshots`.
#[derive(clap::Args)]
pub struct Arguments {
    /// Only the screenshots whose file name contains this
    filter: Option<String>,
    /// Write nothing in images/: say which screenshots differ from the committed ones, and
    /// leave the new pictures and their differences in target/screenshots/check
    #[arg(long)]
    check: bool,
}

/// Makes the screenshots as the module's documentation says.
pub fn run_screenshots(arguments: &Arguments) -> Result {
    let root = workspace_root();
    let list = ScreenshotList::read(&root.join(SCREENSHOTS_FILE))?;
    let games = GameList::read(&root.join(GAMES_FILE))?;
    check_credits(&root, &list, &games)?;
    let screenshots: Vec<_> = list
        .screenshots
        .iter()
        .filter(|screenshot| {
            arguments
                .filter
                .as_ref()
                .is_none_or(|filter| screenshot.file.contains(filter.as_str()))
        })
        .collect();
    if screenshots.is_empty() {
        println!("No screenshots to make: {SCREENSHOTS_FILE} lists none that match.");
        return Ok(());
    }

    gnome::check_tools()?;
    let work = target_dir().join("screenshots");
    let games_folder = download_games(&screenshots, &games, &work.join("games"))?;
    let app = build_app()?;

    let captures = work.join("captures");
    let check_folder = work.join("check");
    for folder in [&captures, &check_folder] {
        if folder.exists() {
            fs::remove_dir_all(folder)
                .map_err(|error| format!("couldn't clear {}: {error}", folder.display()))?;
        }
    }
    let mut plan = gnome::Plan::default();
    let mut tiles_of = Vec::new();
    for (index, screenshot) in screenshots.iter().enumerate() {
        let mut outputs = Vec::new();
        for (tile_index, tile) in screenshot.tiles().iter().enumerate() {
            let output = captures.join(format!("{index}-{tile_index}.png"));
            plan.add(capture_of(
                tile,
                &root,
                &app,
                &games_folder,
                output.clone(),
            )?);
            outputs.push(output);
        }
        tiles_of.push(outputs);
    }

    println!(
        "Taking {} pictures in a headless GNOME Shell...",
        plan.len()
    );
    gnome::run_plan(&plan, &work)?;

    let mut differ = Vec::new();
    for (screenshot, outputs) in screenshots.iter().zip(&tiles_of) {
        let picture = pictures::compose(screenshot, outputs)?;
        let path = root.join(&screenshot.file);
        let same = pictures::read(&path).is_ok_and(|committed| committed == picture);
        if same {
            println!("  {}: unchanged", screenshot.file);
        } else if arguments.check {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            pictures::write(&check_folder.join(&name), &picture)?;
            if let Ok(committed) = pictures::read(&path) {
                let difference = pictures::difference(&committed, &picture);
                pictures::write(
                    &check_folder.join(name.replace(".png", ".diff.png")),
                    &difference,
                )?;
            }
            println!("  {}: differs", screenshot.file);
            differ.push(screenshot.file.clone());
        } else {
            pictures::write(&path, &picture)?;
            println!("  {}: written", screenshot.file);
        }
    }

    if differ.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} screenshot(s) differ from the committed ones; the new pictures and their \
             differences are in {}",
            differ.len(),
            check_folder.display()
        ))
    }
}

/// What the app's `capture` command is asked for one tile of a screenshot:
/// its arguments, and how the GNOME session takes the picture.
fn capture_of(
    tile: &Tile,
    root: &Path,
    app: &Path,
    games_folder: &Path,
    output: PathBuf,
) -> Result<gnome::Capture> {
    let scene_folder = root.join(SCENES_FOLDER).join(&tile.scene);
    let scene = Scene::read(&scene_folder)?;
    let (changes, dark) = tile.changes(&scene);

    let home = output.with_extension("home");
    let mut arguments: Vec<String> = vec![
        String::from("capture"),
        scene_folder.display().to_string(),
        String::from("--games"),
        games_folder.display().to_string(),
        String::from("--home"),
        home.display().to_string(),
    ];
    for change in changes {
        arguments.push(String::from("--set"));
        arguments.push(change);
    }
    for (option, value) in [
        ("--pause-menu", &tile.pause_menu),
        ("--settings-tab", &tile.settings_tab),
    ] {
        if let Some(value) = value {
            arguments.push(option.to_owned());
            arguments.push(value.clone());
        }
    }

    let window = tile
        .size
        .map(|[width, height]| (width, height))
        .or_else(|| {
            scene
                .window
                .map(|window| (window.width.round() as u32, window.height.round() as u32))
        });
    let monitor = match tile.frame {
        Frame::Fullscreen => {
            arguments.push(String::from("--fullscreen"));
            tile.size
                .map_or(FULLSCREEN_SIZE, |[width, height]| (width, height))
        }
        Frame::Game => {
            let scale = tile.scale.unwrap_or(GAME_SCALE);
            arguments.push(String::from("--scale"));
            arguments.push(scale.to_string());
            game_monitor(scale)
        }
        Frame::None | Frame::Gnome => {
            if let Some((width, height)) = tile.size.map(|[width, height]| (width, height)) {
                arguments.push(String::from("--size"));
                arguments.push(format!("{width}x{height}"));
            }
            let (width, height) = window.unwrap_or((1000, 800));
            (width.max(1520) + 400, height.max(900) + 300)
        }
    };
    if tile.frame != Frame::Gnome {
        arguments.push(String::from("--output"));
        arguments.push(output.display().to_string());
    }

    Ok(gnome::Capture {
        app: app.to_path_buf(),
        arguments,
        framed: tile.frame == Frame::Gnome,
        output,
        dark,
        monitor,
    })
}

/// The screen a picture of the game alone, `scale` times its size, is taken
/// on: 1920×1200, or bigger when the largest picture, a Super Game Boy's with
/// its border, wouldn't fit on it.
fn game_monitor(scale: u32) -> (u32, u32) {
    (
        (SGB_FRAME_WIDTH * scale).max(1920),
        (SGB_FRAME_HEIGHT * scale).max(1200),
    )
}

/// Downloads every game the screenshots' scenes use into `folder`, under
/// the file names the scenes know them by, unless it is there with the
/// right checksum, and returns the folder.
fn download_games(
    screenshots: &[&lists::Screenshot],
    games: &GameList,
    folder: &Path,
) -> Result<PathBuf> {
    let root = workspace_root();
    let mut needed = BTreeMap::new();
    for screenshot in screenshots {
        for tile in screenshot.tiles() {
            let scene = Scene::read(&root.join(SCENES_FOLDER).join(&tile.scene))?;
            for file in scene_games(&scene) {
                let game = games.game(&file).ok_or_else(|| {
                    format!("{GAMES_FILE} doesn't say where to download {file}, which the scene {} uses", tile.scene)
                })?;
                if let Some(scene_game) = &scene.game
                    && scene_game.file == file
                    && scene_game.sha256 != game.sha256
                {
                    return Err(format!(
                        "the scene {} was made with another {file} than {GAMES_FILE} downloads",
                        tile.scene
                    ));
                }
                needed.insert(file, game);
            }
        }
    }

    fs::create_dir_all(folder)
        .map_err(|error| format!("couldn't create {}: {error}", folder.display()))?;
    for (file, game) in needed {
        download_game(game, &folder.join(file))?;
    }
    Ok(folder.to_path_buf())
}

/// Checks that the README credits every game a listed screenshot shows,
/// by the title `games.toml` gives it, as the games' licenses ask.
pub fn check_credits(root: &Path, list: &ScreenshotList, games: &GameList) -> Result {
    let readme = fs::read_to_string(root.join("README.md"))
        .map_err(|error| format!("couldn't read the README: {error}"))?;
    let mut missing = Vec::new();
    for screenshot in &list.screenshots {
        for tile in screenshot.tiles() {
            let scene = Scene::read(&root.join(SCENES_FOLDER).join(&tile.scene))?;
            for file in scene_games(&scene) {
                if let Some(game) = games.game(&file)
                    && !readme.contains(&game.title)
                {
                    missing.push(format!(
                        "{} by {} ({})",
                        game.title, game.authors, game.license
                    ));
                }
            }
        }
    }
    missing.sort();
    missing.dedup();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "the README's credits don't name every game in the screenshots; add {}",
            missing.join(", ")
        ))
    }
}

/// The file names of the games `scene` uses: its game, or its library's.
pub fn scene_games(scene: &Scene) -> Vec<String> {
    scene
        .game
        .iter()
        .map(|game| game.file.clone())
        .chain(
            scene
                .library
                .iter()
                .flat_map(|library| library.games.clone()),
        )
        .collect()
}

/// Downloads `game` to `path`, unless it is there with the right checksum.
pub fn download_game(game: &Game, path: &Path) -> Result {
    if nubo_download::has_checksum(path, &game.sha256) {
        return Ok(());
    }
    println!("Downloading {}...", game.file);
    nubo_download::download(&game.url, path)?;
    if nubo_download::has_checksum(path, &game.sha256) {
        Ok(())
    } else {
        Err(format!(
            "{} downloaded from {} doesn't have the checksum {GAMES_FILE} gives",
            game.file, game.url
        ))
    }
}

/// Builds the app in release mode and returns the path of its executable.
fn build_app() -> Result<PathBuf> {
    run(cargo()
        .args([
            "build",
            "--release",
            "--locked",
            "--package",
            crate::ICED_PACKAGE,
        ])
        .current_dir(workspace_root()))?;
    Ok(target_dir().join("release").join(crate::ICED_PACKAGE))
}

pub mod gnome;
pub mod lists;
mod pictures;
#[cfg(test)]
mod tests;
