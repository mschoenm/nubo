//! The player's preferences: the ones every app shares
//! (`nubo_frontend::settings`) and the ones only this app has, such as its
//! appearance, integer scaling and how the library is sorted. They are kept
//! together in one `settings.toml` (in localStorage in the browser build),
//! and saved as soon as they change.

use nubo_frontend::settings::Loaded;
use nubo_frontend::storage::{self, Entry};
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

/// The order the library lists all its games in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySort {
    /// By name, from A to Z.
    #[default]
    Name,
    /// The games played last first, then the others by name.
    RecentlyPlayed,
    /// The games added to the library last first.
    RecentlyAdded,
}

impl LibrarySort {
    /// Every order, as the sort menu lists them.
    pub const ALL: [LibrarySort; 3] = [
        LibrarySort::Name,
        LibrarySort::RecentlyPlayed,
        LibrarySort::RecentlyAdded,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LibrarySort::Name => "Name",
            LibrarySort::RecentlyPlayed => "Recently played",
            LibrarySort::RecentlyAdded => "Recently added",
        }
    }
}

/// Which of its games the library lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryFilter {
    #[default]
    All,
    /// Only games for the original Game Boy.
    GameBoy,
    /// Only games that run in color on a Game Boy Color.
    Color,
}

impl LibraryFilter {
    /// Every filter, as the sort menu lists them.
    pub const ALL: [LibraryFilter; 3] = [
        LibraryFilter::All,
        LibraryFilter::GameBoy,
        LibraryFilter::Color,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LibraryFilter::All => "All games",
            LibraryFilter::GameBoy => "Game Boy games",
            LibraryFilter::Color => "Game Boy Color games",
        }
    }

    /// Whether the filter lets a game through that does or doesn't run in
    /// color.
    pub fn allows(self, supports_cgb: bool) -> bool {
        match self {
            LibraryFilter::All => true,
            LibraryFilter::GameBoy => !supports_cgb,
            LibraryFilter::Color => supports_cgb,
        }
    }
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
/// then `integer_scaling`. Later fields are only saved once they differ
/// from their default. TOML puts plain values before tables, so the shared
/// tables (the library folders and the controls) still come last.
/// Reading goes through [`Settings::from_toml`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Settings {
    pub appearance: Appearance,
    /// The settings every app shares, stored next to this app's own.
    #[serde(flatten)]
    pub shared: nubo_frontend::settings::Settings,
    /// Whether the screen only grows in whole steps (2×, 3×, …), which keeps
    /// every pixel the same size.
    pub integer_scaling: bool,
    /// The order the library lists all games in.
    #[serde(skip_serializing_if = "is_default")]
    pub library_sort: LibrarySort,
    /// Which games the library lists.
    #[serde(skip_serializing_if = "is_default")]
    pub library_filter: LibraryFilter,
    /// Whether the game pauses while the window isn't the one in front, and
    /// goes on when it is again. On until the player turns it off, and only
    /// saved then.
    #[serde(skip_serializing_if = "is_true")]
    pub pause_in_background: bool,
    /// The window's size when the app last closed, in logical pixels, which
    /// it opens at next time. Not kept in the browser, whose window is the
    /// page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_size: Option<(f32, f32)>,
}

impl Default for Settings {
    fn default() -> Self {
        let own = OwnSettings::default();
        Settings {
            appearance: own.appearance,
            shared: nubo_frontend::settings::Settings::default(),
            integer_scaling: own.integer_scaling,
            library_sort: own.library_sort,
            library_filter: own.library_filter,
            pause_in_background: own.pause_in_background,
            window_size: own.window_size,
        }
    }
}

/// Whether `value` is true, for settings saved only when they are off.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
fn is_true(value: &bool) -> bool {
    *value
}

/// Whether `value` is its type's default, which isn't saved.
fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// This app's own settings, read from the same text as the shared ones.
/// What a file leaves out gets the default here: a file that never said
/// whether to scale in whole steps gets them, as new players do.
#[derive(Deserialize)]
#[serde(default)]
struct OwnSettings {
    appearance: Appearance,
    integer_scaling: bool,
    library_sort: LibrarySort,
    library_filter: LibraryFilter,
    pause_in_background: bool,
    window_size: Option<(f32, f32)>,
}

impl Default for OwnSettings {
    fn default() -> Self {
        OwnSettings {
            appearance: Appearance::default(),
            integer_scaling: true,
            library_sort: LibrarySort::default(),
            library_filter: LibraryFilter::default(),
            pause_in_background: true,
            window_size: None,
        }
    }
}

impl Settings {
    /// These settings put back to how they start, keeping only the window's
    /// size and, unless `remove_folders`, the library folders (see
    /// `nubo_frontend::settings::Settings::reset`).
    #[must_use]
    pub fn reset(&self, remove_folders: bool) -> Settings {
        Settings {
            shared: self.shared.reset(remove_folders),
            window_size: self.window_size,
            ..Settings::default()
        }
    }

    /// Reads the saved settings. Without saved settings this gives the
    /// defaults; if they can't be read it gives the defaults together with
    /// the reason, and keeps the unreadable ones aside first (see
    /// `nubo_frontend::settings::load`).
    pub fn load() -> Loaded<Settings> {
        nubo_frontend::settings::load(&*storage::platform(), Settings::from_toml)
    }

    /// The window's size saved in the settings, if they can be read, for
    /// opening the window before the app starts. Unlike [`Settings::load`]
    /// it never keeps unreadable settings aside, so the app's own load still
    /// finds them and tells the player.
    pub fn saved_window_size() -> Option<(f32, f32)> {
        let bytes = storage::platform().read(Entry::Settings).ok()??;
        Settings::from_toml(std::str::from_utf8(&bytes).ok()?)
            .ok()?
            .window_size
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
            library_sort: own.library_sort,
            library_filter: own.library_filter,
            pause_in_background: own.pause_in_background,
            window_size: own.window_size,
        })
    }

    /// Saves the settings, returning why if that failed.
    pub fn save(&self) -> Result<(), String> {
        nubo_frontend::settings::save(&*storage::platform(), self)
    }
}

#[cfg(test)]
mod tests;
