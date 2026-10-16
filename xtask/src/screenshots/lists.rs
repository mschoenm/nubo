//! The two lists `cargo xtask screenshots` works from:
//!
//! - `images/screenshots.toml`: the screenshots, each with its file, its
//!   scene, its frame, changes to the scene's settings, and maybe variants
//!   that go side by side ([`ScreenshotList`]);
//! - `images/scenes/games.toml`: the games the scenes use, each with where
//!   to download it, its checksum, its title and its license
//!   ([`GameList`]).

use crate::Result;
use nubo_frontend::scenes::Scene;
use serde::Deserialize;
use std::path::Path;

/// What `images/screenshots.toml` holds.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScreenshotList {
    /// The screenshots, as `[[screenshot]]` tables.
    #[serde(default, rename = "screenshot")]
    pub screenshots: Vec<Screenshot>,
}

/// One screenshot the README or the Flathub listing shows.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Screenshot {
    /// Where the picture goes, from the workspace's root, such as
    /// `images/screenshots/pause-menu.png`.
    pub file: String,
    /// The scene's folder in `images/scenes/`.
    pub scene: String,
    /// How much of the app the picture shows.
    #[serde(default)]
    pub frame: Frame,
    /// For `none` and `gnome`, the window's inside in logical pixels
    /// instead of the scene's; for `fullscreen`, the screen's size
    /// (1920×1080 unless given).
    pub size: Option<[u32; 2]>,
    /// For `game`, how many times its size the game's picture is shown
    /// (4 unless given).
    pub scale: Option<u32>,
    /// Changes to the scene's settings, by the names `settings.toml` uses.
    #[serde(default)]
    pub settings: toml::Table,
    /// The page of the pause menu to open instead of the scene's (`main`,
    /// `save`, `load`, `cheats`, `super-game-boy`), or `none` to close it.
    pub pause_menu: Option<String>,
    /// The settings tab to open instead of the scene's (`display`,
    /// `playing`, `controls`, `library`, `about`), or `none` to close it.
    pub settings_tab: Option<String>,
    /// GNOME's style, which colors the window's border: `light` or `dark`.
    /// Unless given, it follows the app's appearance, and an app that
    /// follows the system gets this style too.
    pub style: Option<Style>,
    /// Pictures of the same moment with other settings, or of other
    /// scenes, which go side by side in one picture. Without any, the
    /// screenshot is one picture.
    #[serde(default, rename = "variant")]
    pub variants: Vec<Variant>,
    /// How the variants are laid out.
    #[serde(default)]
    pub layout: Layout,
    /// For `grid`, how many pictures each row holds (2 unless given).
    pub columns: Option<u32>,
    /// The space between the variants, in pixels.
    #[serde(default)]
    pub gap: u32,
    /// The space around the variants, in pixels.
    #[serde(default)]
    pub padding: u32,
    /// The color behind the variants, such as `#0d0f14`; see-through
    /// unless given.
    pub background: Option<String>,
}

/// How much of the app a screenshot shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Frame {
    /// The window's inside, as the app captures it itself.
    #[default]
    None,
    /// The window with GNOME's border and shadow, as GNOME captures it.
    Gnome,
    /// The whole screen, which the app fills.
    Fullscreen,
    /// The game's picture alone, as the app's `--scale` shows it.
    Game,
}

/// GNOME's light or dark style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    Light,
    Dark,
}

/// How the variants of a screenshot are laid out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    /// Next to each other.
    #[default]
    Row,
    /// Above each other.
    Column,
    /// In rows of `columns`.
    Grid,
}

/// A variant of a screenshot: another scene or other settings.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    /// The scene, if not the screenshot's.
    pub scene: Option<String>,
    /// Changes to the settings, on top of the screenshot's.
    #[serde(default)]
    pub settings: toml::Table,
    /// The page of the pause menu to open, if not the screenshot's.
    pub pause_menu: Option<String>,
    /// The settings tab to open, if not the screenshot's.
    pub settings_tab: Option<String>,
}

/// One picture the app takes for a screenshot: the screenshot itself, or
/// one of its variants.
#[derive(Debug, Clone)]
pub struct Tile {
    pub scene: String,
    pub frame: Frame,
    pub size: Option<[u32; 2]>,
    pub scale: Option<u32>,
    /// The changes to the scene's settings, the screenshot's and the
    /// variant's together.
    pub settings: toml::Table,
    pub pause_menu: Option<String>,
    pub settings_tab: Option<String>,
    pub style: Option<Style>,
}

impl ScreenshotList {
    /// Reads `images/screenshots.toml`, or an empty list if there is none.
    pub fn read(path: &Path) -> Result<ScreenshotList> {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Ok(ScreenshotList::default());
        };
        let list: ScreenshotList =
            toml::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
        for screenshot in &list.screenshots {
            screenshot
                .check()
                .map_err(|error| format!("{}: {}: {error}", path.display(), screenshot.file))?;
        }
        Ok(list)
    }
}

impl Screenshot {
    /// Checks what serde can't: the options that only some frames and
    /// layouts take.
    fn check(&self) -> Result {
        if !self.file.ends_with(".png") {
            return Err(String::from("screenshots are PNG files"));
        }
        if self.scale.is_some() && self.frame != Frame::Game {
            return Err(String::from("only frame = \"game\" takes a scale"));
        }
        if self.size.is_some() && self.frame == Frame::Game {
            return Err(String::from("frame = \"game\" takes a scale, not a size"));
        }
        if self
            .size
            .is_some_and(|[width, height]| width == 0 || height == 0)
        {
            return Err(String::from("a size can't be 0 wide or high"));
        }
        if self.scale.is_some_and(|scale| !(1..=8).contains(&scale)) {
            return Err(String::from("the scale goes from 1 to 8"));
        }
        if self.columns.is_some() && self.layout != Layout::Grid {
            return Err(String::from("only layout = \"grid\" takes columns"));
        }
        if let Some(background) = &self.background {
            parse_color(background)?;
        }
        Ok(())
    }

    /// The pictures the app takes for this screenshot: one, or one for
    /// each variant.
    pub fn tiles(&self) -> Vec<Tile> {
        let tile = |variant: &Variant| {
            let mut settings = self.settings.clone();
            merge(&mut settings, &variant.settings);
            Tile {
                scene: variant.scene.clone().unwrap_or_else(|| self.scene.clone()),
                frame: self.frame,
                size: self.size,
                scale: self.scale,
                settings,
                pause_menu: variant
                    .pause_menu
                    .clone()
                    .or_else(|| self.pause_menu.clone()),
                settings_tab: variant
                    .settings_tab
                    .clone()
                    .or_else(|| self.settings_tab.clone()),
                style: self.style,
            }
        };
        if self.variants.is_empty() {
            vec![tile(&Variant::default())]
        } else {
            self.variants.iter().map(tile).collect()
        }
    }

    /// The color behind the variants, as RGBA.
    pub fn background(&self) -> [u8; 4] {
        self.background
            .as_deref()
            .and_then(|color| parse_color(color).ok())
            .unwrap_or([0, 0, 0, 0])
    }
}

impl Tile {
    /// The `--set key=value` changes the app's `capture` command gets for
    /// this tile of `scene`, and whether GNOME's style is dark. An app that
    /// follows the system's appearance is given the style's appearance, so
    /// the picture never depends on how the app finds out the system's.
    pub fn changes(&self, scene: &Scene) -> (Vec<String>, bool) {
        let mut settings = self.settings.clone();
        let appearance = settings
            .get("appearance")
            .or_else(|| scene.settings.get("appearance"))
            .and_then(toml::Value::as_str)
            .unwrap_or("system")
            .to_owned();
        let dark = match (self.style, appearance.as_str()) {
            (Some(style), _) => style == Style::Dark,
            (None, appearance) => appearance == "dark",
        };
        if appearance == "system" {
            let appearance = if dark { "dark" } else { "light" };
            settings.insert(String::from("appearance"), appearance.into());
        }

        let mut changes = Vec::new();
        flatten("", &settings, &mut changes);
        (changes, dark)
    }
}

/// Adds every setting in `extra` to `settings`, going into tables.
fn merge(settings: &mut toml::Table, extra: &toml::Table) {
    for (key, value) in extra {
        match (settings.get_mut(key), value) {
            (Some(toml::Value::Table(inner)), toml::Value::Table(extra_inner)) => {
                merge(inner, extra_inner);
            }
            _ => {
                settings.insert(key.clone(), value.clone());
            }
        }
    }
}

/// Writes every value in `table` as `key=value`, with the keys of values in
/// tables joined by dots, as `capture`'s `--set` takes them.
fn flatten(prefix: &str, table: &toml::Table, changes: &mut Vec<String>) {
    for (key, value) in table {
        let key = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            toml::Value::Table(inner) => flatten(&key, inner, changes),
            value => changes.push(format!("{key}={value}")),
        }
    }
}

/// Reads a color written as `#rrggbb` or `#rrggbbaa`.
fn parse_color(text: &str) -> Result<[u8; 4]> {
    let mistake = || format!("'{text}' isn't a color such as #0d0f14");
    let digits = text.strip_prefix('#').ok_or_else(mistake)?;
    if !matches!(digits.len(), 6 | 8) || !digits.is_ascii() {
        return Err(mistake());
    }
    let byte =
        |index: usize| u8::from_str_radix(&digits[index..index + 2], 16).map_err(|_| mistake());
    let alpha = if digits.len() == 8 { byte(6)? } else { 255 };
    Ok([byte(0)?, byte(2)?, byte(4)?, alpha])
}

/// What `images/scenes/games.toml` holds.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameList {
    /// The games, as `[[game]]` tables.
    #[serde(default, rename = "game")]
    pub games: Vec<Game>,
}

/// A game scenes may use. Only freely licensed games whose authors allow
/// sharing pictures of them go here, and every one in a published
/// screenshot is credited in the README.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Game {
    /// The file name the game is saved under, which the app shows as the
    /// game's name, such as `Tobu Tobu Girl Deluxe.gb`. Scenes name the
    /// game by it.
    pub file: String,
    /// The game's title, as the README's credits name it.
    pub title: String,
    /// Who made it, as the README's credits name them.
    pub authors: String,
    /// Where to download it, at a fixed version.
    pub url: String,
    /// The SHA-256 checksum of the file, in lowercase hexadecimal.
    pub sha256: String,
    /// The game's license, or its code's and its graphics'.
    pub license: String,
}

impl GameList {
    /// Reads `images/scenes/games.toml`, or an empty list if there is none.
    pub fn read(path: &Path) -> Result<GameList> {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Ok(GameList::default());
        };
        toml::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))
    }

    /// The game saved as `file`.
    pub fn game(&self, file: &str) -> Option<&Game> {
        self.games.iter().find(|game| game.file == file)
    }
}
