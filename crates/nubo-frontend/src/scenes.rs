//! Scenes: recorded moments of a game, kept so a screenshot of the app can be
//! made again exactly, with any later version of Nubo and any settings. The
//! README and the Flathub listing show such screenshots (see
//! `docs/SCREENSHOTS_PLAN.md`).
//!
//! A scene is a folder holding:
//!
//! - `scene.toml`: the game (its file name and SHA-256 checksum), the
//!   hardware model, how many frames to run from the save state, the
//!   window's size, what is open on screen (the game, the library, the pause
//!   menu, a settings tab) and the settings, in the same words as
//!   `settings.toml`;
//! - `game.state`: a save state of the game, which the frames run from;
//! - `preview.png`: the window as it looked when the scene was made, to
//!   compare new screenshots with.
//!
//! A scene of the library has no game: it names the games the library holds
//! and the ones it lists as played. The desktop app makes scenes with its
//! hidden `--record-scenes` option (Ctrl+F12) and shows them with its hidden
//! `capture` command; `cargo xtask scene explore` makes them headless, and
//! `cargo xtask screenshots` turns them into the published screenshots.

use nubo_core::Model;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The file in a scene's folder that describes it.
pub const SCENE_FILE: &str = "scene.toml";
/// The file in a scene's folder that holds the game's save state.
pub const STATE_FILE: &str = "game.state";
/// The file in a scene's folder that shows how it looked when it was made.
pub const PREVIEW_FILE: &str = "preview.png";
/// How many frames a recorded scene runs from its save state: two, so that
/// frame blending, which mixes each frame with the one before, has both.
pub const RECORDED_FRAMES: u32 = 2;

/// What `scene.toml` holds (see the module's documentation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    /// The game running in the scene, which the scene's save state is of.
    /// A scene of the library has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<SceneGame>,
    /// The size of the window's inside, in logical pixels, or `None` for the
    /// size the app opens at without saved settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowSize>,
    /// What is open on screen.
    #[serde(default)]
    pub view: View,
    /// The library a scene of the library shows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<SceneLibrary>,
    /// The settings, by the names `settings.toml` uses. Settings left out
    /// have their defaults; the library folders and the window's size are
    /// never kept here.
    #[serde(default)]
    pub settings: toml::Table,
}

/// The game of a scene and how far to run it from its save state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneGame {
    /// The ROM's file name, such as `tobudx.gb`. The game lists that
    /// download games for scenes (`images/scenes/games.toml`) are looked up
    /// by it.
    pub file: String,
    /// The SHA-256 checksum of the ROM file, in lowercase hexadecimal, so a
    /// scene never runs on another revision of the game.
    pub sha256: String,
    /// The hardware model, by its short name (such as `CGB-E`, see
    /// `Model::name`). The save state is of this model.
    pub model: String,
    /// How many frames to run from the save state before the picture is
    /// taken (see `Emulator::run_frame`).
    pub frames: u32,
}

impl SceneGame {
    /// The hardware model the scene runs on.
    pub fn model(&self) -> Result<Model, String> {
        self.model.parse()
    }
}

/// The size of a window's inside, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowSize {
    /// How wide the window's inside is.
    pub width: f32,
    /// How high the window's inside is.
    pub height: f32,
}

/// What is open on screen in a scene.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    /// The screen under everything else.
    #[serde(default)]
    pub screen: Screen,
    /// The page of the pause menu that is open over the game, by its name
    /// in `scene.toml`: `main`, `save`, `load`, `cheats` or
    /// `super-game-boy`. `None` while the game plays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_menu: Option<String>,
    /// The settings tab that is open, by its name in `scene.toml`:
    /// `display`, `playing`, `controls`, `library` or `about`. `None` while
    /// the settings are closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings_tab: Option<String>,
    /// Whether the window filled the screen when the scene was recorded.
    /// The screenshots made of it decide for themselves.
    #[serde(default, skip_serializing_if = "is_false")]
    pub fullscreen: bool,
}

/// The screen under everything else in a scene.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Screen {
    /// The game, which the scene's save state is of.
    #[default]
    Game,
    /// The game library.
    Library,
}

/// The library of a scene of the library.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneLibrary {
    /// The ROM files in the library's folder, by file name, as the game
    /// lists name them.
    pub games: Vec<String>,
    /// The games listed as played ("Continue playing"), each with how long
    /// ago, newest first.
    #[serde(default)]
    pub played: Vec<PlayedGame>,
}

/// A game a scene's library lists as played.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayedGame {
    /// The ROM file, by file name.
    pub file: String,
    /// How many minutes before the picture is taken the game was played,
    /// which its card shows (such as "2 hours ago").
    pub minutes_ago: u64,
}

/// Whether `value` is false, which `scene.toml` leaves out.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
fn is_false(value: &bool) -> bool {
    !*value
}

impl Scene {
    /// Reads `scene.toml` from the scene's `folder`.
    pub fn read(folder: &Path) -> Result<Scene, String> {
        let path = folder.join(SCENE_FILE);
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("couldn't read {}: {error}", path.display()))?;
        Scene::from_toml(&text).map_err(|error| format!("{}: {error}", path.display()))
    }

    /// Reads a scene from the text of a `scene.toml`, checking that it
    /// names a game unless it shows the library.
    pub fn from_toml(text: &str) -> Result<Scene, String> {
        let scene: Scene = toml::from_str(text).map_err(|error| error.to_string())?;
        match (&scene.game, scene.view.screen, &scene.library) {
            (None, Screen::Game, _) => Err(String::from("a scene of a game needs [game]")),
            (_, Screen::Library, None) => {
                Err(String::from("a scene of the library needs [library]"))
            }
            _ => Ok(scene),
        }
    }

    /// The text of the scene's `scene.toml`.
    pub fn to_toml(&self) -> String {
        toml::to_string(self).expect("a scene is always valid TOML")
    }

    /// Writes the scene into `folder`, creating it: its `scene.toml`, the
    /// game's save state if it has one and the preview picture as PNG data.
    pub fn write(
        &self,
        folder: &Path,
        state: Option<&[u8]>,
        preview_png: Option<&[u8]>,
    ) -> Result<(), String> {
        let write = |name: &str, contents: &[u8]| {
            let path = folder.join(name);
            std::fs::write(&path, contents)
                .map_err(|error| format!("couldn't write {}: {error}", path.display()))
        };
        std::fs::create_dir_all(folder)
            .map_err(|error| format!("couldn't create {}: {error}", folder.display()))?;
        write(SCENE_FILE, self.to_toml().as_bytes())?;
        if let Some(state) = state {
            write(STATE_FILE, state)?;
        }
        if let Some(preview) = preview_png {
            write(PREVIEW_FILE, preview)?;
        }
        Ok(())
    }
}

/// Changes `settings` (the text of a `settings.toml` read into a table) as
/// `changes` say: each is a key and a value, such as `appearance` and
/// `dark`. A key with dots names a value inside a table, such as
/// `controls.keyboard.a`. The value is read as TOML (`true`, `0.5`,
/// `"dark"`), and as text when it isn't valid TOML, so `dark` works without
/// quotes.
pub fn change_settings(
    settings: &mut toml::Table,
    changes: &[(String, String)],
) -> Result<(), String> {
    for (key, value) in changes {
        let value = parse_value(value);
        let mut parts: Vec<&str> = key.split('.').collect();
        let last = parts
            .pop()
            .filter(|last| !last.is_empty())
            .ok_or_else(|| format!("'{key}' names no setting"))?;
        let mut table = &mut *settings;
        for part in parts {
            table = table
                .entry(part)
                .or_insert_with(|| toml::Value::Table(toml::Table::new()))
                .as_table_mut()
                .ok_or_else(|| format!("'{part}' in '{key}' isn't a table of settings"))?;
        }
        table.insert(last.to_owned(), value);
    }
    Ok(())
}

/// Reads one value of a setting as TOML, or as text when it isn't TOML.
fn parse_value(value: &str) -> toml::Value {
    toml::from_str::<toml::Table>(&format!("value = {value}"))
        .ok()
        .and_then(|mut table| table.remove("value"))
        .unwrap_or_else(|| toml::Value::String(value.to_owned()))
}

/// Reads a change of a setting written as `key=value`, as the app's
/// `capture` command and `cargo xtask screenshots` take them.
pub fn parse_change(text: &str) -> Result<(String, String), String> {
    let (key, value) = text
        .split_once('=')
        .ok_or_else(|| format!("'{text}' isn't written as key=value"))?;
    Ok((key.trim().to_owned(), value.trim().to_owned()))
}

/// The SHA-256 checksum of `bytes` in lowercase hexadecimal, which scenes
/// name their game's ROM by.
#[cfg(feature = "desktop")]
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;

    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

#[cfg(test)]
mod tests;
