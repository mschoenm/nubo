//! The player's preferences: the ones every app shares
//! (`nubo_frontend::settings`) and two only this app has, its appearance
//! and integer scaling. They are kept together in one `settings.toml` (in
//! localStorage in the browser build), and saved as soon as they change.

use nubo_frontend::storage;
use serde::{Deserialize, Serialize};
use std::fmt;

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

/// Everything the player can change in the Settings window. Fields missing
/// from the saved file get their default, so older files keep working.
///
/// The fields are in the order earlier versions wrote them, which keeps
/// `settings.toml` the same: `appearance` first, then the shared settings,
/// then `integer_scaling`. TOML puts plain values before tables, so the
/// shared tables (the library folders and the controls) still come last.
/// Reading goes through [`Settings::from_toml`].
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Settings {
    pub appearance: Appearance,
    /// The settings every app shares, stored next to this app's own.
    #[serde(flatten)]
    pub shared: nubo_frontend::settings::Settings,
    /// Whether the screen only grows in whole steps (2×, 3×, …), which keeps
    /// every pixel the same size.
    pub integer_scaling: bool,
}

/// This app's own settings, read from the same text as the shared ones.
#[derive(Default, Deserialize)]
#[serde(default)]
struct OwnSettings {
    appearance: Appearance,
    integer_scaling: bool,
}

impl Settings {
    /// Reads the saved settings. Without saved settings this returns the
    /// defaults; if they can't be read it returns the defaults together with
    /// the reason.
    pub fn load() -> (Settings, Option<String>) {
        nubo_frontend::settings::load(&*storage::platform(), Settings::from_toml)
    }

    /// Reads the settings from the text of a `settings.toml`. The shared
    /// settings and this app's own are read separately, each skipping the
    /// other's keys, rather than with serde's flatten, which would report a
    /// mistake in the shared settings at the top of the file instead of
    /// where it is.
    pub fn from_toml(contents: &str) -> Result<Settings, toml::de::Error> {
        let own: OwnSettings = toml::from_str(contents)?;
        Ok(Settings {
            appearance: own.appearance,
            shared: toml::from_str(contents)?,
            integer_scaling: own.integer_scaling,
        })
    }

    /// Saves the settings, returning why if that failed.
    pub fn save(&self) -> Result<(), String> {
        nubo_frontend::settings::save(&*storage::platform(), self)
    }
}

#[cfg(test)]
mod tests;
