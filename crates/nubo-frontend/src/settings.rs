//! The player's preferences that every app shares. They are kept between
//! runs as TOML in `settings.toml` (see [`Entry::Settings`]), and saved as
//! soon as they change.
//!
//! An app with settings of its own keeps them in the same file: its own
//! settings type takes [`Settings`] in with `#[serde(flatten)]` to write
//! them, and reads its own settings and [`Settings`] from the same text, each
//! skipping the other's keys (serde's flatten would lose where in the file a
//! mistake is). It reads and writes that type with [`load`] and [`save`]. The
//! Iced app does this for its appearance and integer scaling
//! (`frontends/nubo/src/settings.rs`).

use crate::controls::Controls;
use crate::model::ModelPreference;
use crate::palettes::DmgPalette;
use crate::sgb_menu::{DEFAULT_OWN_PALETTE, SgbColors};
use crate::storage::{Entry, Storage};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// A folder the library looks for ROMs in.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LibraryFolder {
    /// Where the folder is.
    pub path: PathBuf,
    /// Whether to look in the folders inside it too, all the way down.
    pub include_subfolders: bool,
}

/// Everything the player can change in the settings that isn't about one
/// app's looks. Fields missing from the saved file get their default, so
/// older files keep working.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The hardware that games start on.
    pub model: ModelPreference,
    /// The folders the library shows the games of.
    pub library_folders: Vec<LibraryFolder>,
    /// Whether to download box art for the library and show it on the
    /// cards. On until the player turns it off; the Settings page says
    /// that it sends the games' names to the libretro servers. It is on the
    /// Library tab of the settings.
    pub download_box_art: bool,
    /// Whether leaving a game saves its state, so it continues there the
    /// next time it starts. On until the player turns it off, in the
    /// Playing section of the settings' General tab.
    pub resume_games: bool,
    /// How fast fast forward runs.
    pub fast_forward_speed: FastForwardSpeed,
    /// How loud to play, from 0 (silent) to 1 (as loud as the game).
    pub volume: f32,
    /// Whether the sound is off, whatever the volume.
    pub muted: bool,
    /// Which keys and controller buttons do what.
    pub controls: Controls,
    /// The colors original Game Boy games are shown in.
    pub dmg_palette: DmgPalette,
    /// Whether a Super Game Boy's border shows around the screen. On until
    /// the player turns it off, and only saved then, so files from versions
    /// without it write back unchanged.
    #[serde(skip_serializing_if = "is_true")]
    pub sgb_border: bool,
    /// The player's own palette for Super Game Boy games, four RGB555
    /// colors from the lightest shade to the darkest, which they edit on the
    /// pause menu's Super Game Boy page. Only saved once it differs from
    /// [`DEFAULT_OWN_PALETTE`].
    #[serde(skip_serializing_if = "is_default_own_palette")]
    pub sgb_own_palette: [u16; 4],
    /// The colors Super Game Boy games show, as the player picks them on the
    /// pause menu's Super Game Boy page: the game's until they pick others,
    /// and only saved then. Save states don't keep them, so they also
    /// apply to a game continued where the player left off.
    #[serde(skip_serializing_if = "is_game_colors")]
    pub sgb_colors: SgbColors,
    /// Whether Super Game Boy games show Nubo's border even when they bring
    /// their own, as the player picks it on the same page. Only saved when
    /// it is on.
    #[serde(skip_serializing_if = "is_false")]
    pub sgb_nubo_border: bool,
}

/// Whether `colors` are the game's own, which settings leave out.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
fn is_game_colors(colors: &SgbColors) -> bool {
    *colors == SgbColors::Game
}

/// Whether `value` is false, for settings saved only when they are on.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
fn is_false(value: &bool) -> bool {
    !*value
}

/// Whether `palette` is the player's own palette as it starts. serde's
/// skip_serializing_if passes a reference to the array, which this takes as
/// a slice: a reference to an array of this size trips a Clippy lint in some
/// builds and not others, so no single `expect` fits.
fn is_default_own_palette(palette: &[u16]) -> bool {
    palette == DEFAULT_OWN_PALETTE
}

/// Whether `value` is true, for settings saved only when they are off.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
fn is_true(value: &bool) -> bool {
    *value
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            model: ModelPreference::default(),
            library_folders: Vec::new(),
            download_box_art: true,
            resume_games: true,
            fast_forward_speed: FastForwardSpeed::default(),
            volume: 1.0,
            muted: false,
            controls: Controls::default(),
            dmg_palette: DmgPalette::default(),
            sgb_border: true,
            sgb_own_palette: DEFAULT_OWN_PALETTE,
            sgb_colors: SgbColors::Game,
            sgb_nubo_border: false,
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

    /// How many times faster than real time this speed is.
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

/// Reads the settings saved in `storage` with `parse`, which makes a `T` of
/// the TOML text: [`Settings`] (with `toml::from_str`) or an app's own type
/// that holds them. Without saved settings this returns the defaults; if
/// they can't be read it returns the defaults together with the reason.
pub fn load<T: Default>(
    storage: &dyn Storage,
    parse: impl FnOnce(&str) -> Result<T, toml::de::Error>,
) -> (T, Option<String>) {
    match read_text(storage) {
        Ok(None) => (T::default(), None),
        Ok(Some(contents)) => match parse(&contents) {
            Ok(settings) => (settings, None),
            Err(error) => (
                T::default(),
                Some(format!(
                    "Your settings couldn't be read and were reset: {error}"
                )),
            ),
        },
        Err(error) => (
            T::default(),
            Some(format!("Your settings couldn't be read: {error}")),
        ),
    }
}

/// Saves `settings` (see [`load`]) in `storage`, returning why if that
/// failed.
pub fn save<T: Serialize>(storage: &dyn Storage, settings: &T) -> Result<(), String> {
    let contents = toml::to_string(settings).map_err(|error| error.to_string())?;
    storage
        .write(Entry::Settings, contents.as_bytes())
        .map_err(|error| format!("Your settings couldn't be saved: {error}"))
}

/// The saved settings file as text, or `None` if there isn't one yet.
fn read_text(storage: &dyn Storage) -> Result<Option<String>, String> {
    match storage.read(Entry::Settings)? {
        None => Ok(None),
        // The same words as reading a file that isn't text says.
        Some(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| String::from("stream did not contain valid UTF-8")),
    }
}

#[cfg(test)]
mod tests;
