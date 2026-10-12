//! The user's preferences. They are kept between runs as TOML in
//! `settings.toml` in the per-user config folder (or in localStorage in the
//! browser build), and saved as soon as they change.

use crate::controls::Controls;
use crate::palettes::DmgPalette;
use nubo_core::{CartridgeHeader, Model};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// Whether the window uses light or dark colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    /// Follow the operating system's light or dark mode.
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    /// Every appearance, in the order the settings show them.
    pub const ALL: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];
}

impl fmt::Display for Appearance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Appearance::System => "System",
            Appearance::Light => "Light",
            Appearance::Dark => "Dark",
        })
    }
}

/// Which hardware a game starts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum ModelPreference {
    /// Game Boy Color games start on a Game Boy Color (CGB-E), other games
    /// on the original Game Boy (DMG).
    #[default]
    Automatic,
    /// Every game starts on this model.
    Fixed(Model),
}

impl ModelPreference {
    /// The model to start the game with this header on.
    pub fn model_for(self, header: &CartridgeHeader) -> Model {
        match self {
            ModelPreference::Automatic if header.supports_cgb => Model::CgbE,
            ModelPreference::Automatic => Model::Dmg,
            ModelPreference::Fixed(model) => model,
        }
    }
}

/// Saved as "auto" or as the model's short name, such as "CGB-E".
impl From<ModelPreference> for String {
    fn from(preference: ModelPreference) -> String {
        match preference {
            ModelPreference::Automatic => String::from("auto"),
            ModelPreference::Fixed(model) => String::from(model.name()),
        }
    }
}

impl TryFrom<String> for ModelPreference {
    type Error = String;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        if name.eq_ignore_ascii_case("auto") {
            Ok(ModelPreference::Automatic)
        } else {
            name.parse().map(ModelPreference::Fixed)
        }
    }
}

/// A folder the library looks for ROMs in.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LibraryFolder {
    pub path: PathBuf,
    /// Whether to look in the folders inside it too, all the way down.
    pub include_subfolders: bool,
}

/// Everything the user can change in the Settings window. Fields missing
/// from the saved file get their default, so older files keep working.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub appearance: Appearance,
    /// The hardware that games start on.
    pub model: ModelPreference,
    pub library_folders: Vec<LibraryFolder>,
    /// Whether to download box art for the library and show it on the
    /// cards. On until the player turns it off; the Settings page says
    /// that it sends the games' names to the libretro servers.
    pub download_box_art: bool,
    /// Whether leaving a game saves its state, so it continues there the
    /// next time it starts. Off until the player turns it on.
    pub resume_games: bool,
    pub fast_forward_speed: FastForwardSpeed,
    /// How loud to play, from 0 (silent) to 1 (as loud as the game).
    pub volume: f32,
    pub muted: bool,
    pub controls: Controls,
    /// The colors original Game Boy games are shown in.
    pub dmg_palette: DmgPalette,
    /// Whether the screen only grows in whole steps (2×, 3×, …), which keeps
    /// every pixel the same size.
    pub integer_scaling: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            appearance: Appearance::default(),
            model: ModelPreference::default(),
            library_folders: Vec::new(),
            download_box_art: true,
            resume_games: false,
            fast_forward_speed: FastForwardSpeed::default(),
            volume: 1.0,
            muted: false,
            controls: Controls::default(),
            dmg_palette: DmgPalette::default(),
            integer_scaling: false,
        }
    }
}

impl Settings {
    /// The volume to play at, taking mute into account.
    pub fn effective_volume(&self) -> f32 {
        if self.muted { 0.0 } else { self.volume }
    }
}

/// How many times faster than real time fast forward runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct FastForwardSpeed(u32);

impl FastForwardSpeed {
    /// The speeds the settings offer.
    pub const ALL: [FastForwardSpeed; 4] = [
        FastForwardSpeed(2),
        FastForwardSpeed(3),
        FastForwardSpeed(4),
        FastForwardSpeed(8),
    ];

    pub fn multiplier(self) -> u32 {
        self.0
    }
}

impl Default for FastForwardSpeed {
    fn default() -> Self {
        FastForwardSpeed(4)
    }
}

impl From<FastForwardSpeed> for u32 {
    fn from(speed: FastForwardSpeed) -> u32 {
        speed.0
    }
}

impl TryFrom<u32> for FastForwardSpeed {
    type Error = String;

    fn try_from(multiplier: u32) -> Result<Self, Self::Error> {
        FastForwardSpeed::ALL
            .into_iter()
            .find(|speed| speed.0 == multiplier)
            .ok_or_else(|| format!("{multiplier}× isn't a fast forward speed"))
    }
}

impl fmt::Display for FastForwardSpeed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}×", self.0)
    }
}

impl Settings {
    /// Reads the saved settings. Without saved settings this returns the
    /// defaults; if they can't be read it returns the defaults together with
    /// the reason.
    pub fn load() -> (Settings, Option<String>) {
        match store::read() {
            Ok(None) => (Settings::default(), None),
            Ok(Some(contents)) => match toml::from_str(&contents) {
                Ok(settings) => (settings, None),
                Err(error) => (
                    Settings::default(),
                    Some(format!(
                        "Your settings couldn't be read and were reset: {error}"
                    )),
                ),
            },
            Err(error) => (
                Settings::default(),
                Some(format!("Your settings couldn't be read: {error}")),
            ),
        }
    }

    /// Saves the settings, returning why if that failed.
    pub fn save(&self) -> Result<(), String> {
        let contents = toml::to_string(self).map_err(|error| error.to_string())?;
        store::write(&contents).map_err(|error| format!("Your settings couldn't be saved: {error}"))
    }
}

#[cfg_attr(target_arch = "wasm32", path = "settings/local_storage_store.rs")]
#[cfg_attr(not(target_arch = "wasm32"), path = "settings/file_store.rs")]
mod store;
#[cfg(test)]
mod tests;
