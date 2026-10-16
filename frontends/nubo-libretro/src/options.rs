//! The core options RetroArch shows in its Quick Menu: which hardware a game
//! starts on, the colors of original Game Boy games, the colors and border of
//! Super Game Boy games (the choices a real Super Game Boy's menu offers; see
//! `nubo_frontend::sgb_menu`), and which clock an MBC3 cartridge's real-time
//! clock follows.
//!
//! The definitions here are plain data; `ffi.rs` hands them to RetroArch in
//! whichever form it understands and reads the values back as text, which
//! [`Options::set`] turns into settings.

use nubo_core::{Model, RtcClock};
use nubo_frontend::model::ModelPreference;
use nubo_frontend::palettes::DmgPalette;
use nubo_frontend::sgb_menu::SgbColors;

/// The option that picks the hardware a game starts on.
pub const MODEL: &str = "nubo_model";
/// The option that picks the colors of original Game Boy games.
pub const PALETTE: &str = "nubo_palette";
/// The option that picks which clock the cartridge's real-time clock follows.
pub const RTC: &str = "nubo_rtc";
/// The option that picks a Super Game Boy's border, or hides it.
pub const SGB_BORDER: &str = "nubo_sgb_border";
/// The option that picks the colors of Super Game Boy games.
pub const SGB_COLORS: &str = "nubo_sgb_colors";

/// The value of [`MODEL`] that picks the hardware from the game.
const AUTOMATIC_MODEL: &str = "auto";
/// The value of [`RTC`] for the wall clock.
const REAL_TIME: &str = "real";
/// The value of [`RTC`] for the time the emulator has run.
const EMULATED_TIME: &str = "emulated";
/// The value of [`SGB_BORDER`] that shows the game's border (Nubo's until
/// the game sends one).
const SHOWN: &str = "enabled";
/// The value of [`SGB_BORDER`] that shows Nubo's border, even over a game's.
const NUBO_BORDER: &str = "nubo";
/// The value of [`SGB_BORDER`] that hides the border.
const HIDDEN: &str = "disabled";
/// The value of [`SGB_COLORS`] that keeps the game's own colors.
const GAME_COLORS: &str = "game";

/// One core option as RetroArch shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionDefinition {
    /// The name RetroArch stores the option under, such as `nubo_model`.
    pub key: &'static str,
    /// The option's name in the menu.
    pub description: &'static str,
    /// The longer explanation the menu shows for it.
    pub info: &'static str,
    /// Each value as stored, with the label the menu shows for it. The first
    /// is the default.
    pub values: Vec<(String, String)>,
}

impl OptionDefinition {
    /// The value the option has until the player picks another.
    pub fn default_value(&self) -> &str {
        &self.values[0].0
    }
}

/// Every option the core offers, in the order the menu shows them.
pub fn definitions() -> Vec<OptionDefinition> {
    let models = std::iter::once((String::from(AUTOMATIC_MODEL), String::from("Automatic"))).chain(
        Model::ALL.iter().map(|model| {
            (
                String::from(model.name()),
                String::from(model.description()),
            )
        }),
    );
    vec![
        OptionDefinition {
            key: MODEL,
            description: "Model",
            info: "The hardware to emulate. Automatic runs Game Boy Color games on a Game Boy \
                   Color (CGB-E), Super Game Boy games on a Super Game Boy (SGB) with their \
                   colors and borders, and other games on the original Game Boy (DMG). Takes \
                   effect when the game is restarted or loaded again.",
            values: models.collect(),
        },
        OptionDefinition {
            key: PALETTE,
            description: "Game Boy palette",
            info: "The colors original Game Boy games are shown in when they run on a Game Boy \
                   (not a Game Boy Color, which colors them itself).",
            values: DmgPalette::ALL
                .iter()
                .map(|palette| (palette.to_string().to_lowercase(), palette.to_string()))
                .collect(),
        },
        OptionDefinition {
            key: SGB_COLORS,
            description: "Super Game Boy colors",
            info: "The colors of games on a Super Game Boy, as its menu picks them: the game's \
                   own, or one palette for the whole screen. A game can keep its own colors \
                   anyway, or take them back when it sets new ones. Loading a state keeps the \
                   colors and border picked here, and each netplay player sees their own.",
            values: std::iter::once((String::from(GAME_COLORS), String::from("The game's")))
                .chain(
                    DmgPalette::ALL
                        .iter()
                        .map(|palette| (palette.to_string().to_lowercase(), palette.to_string())),
                )
                .collect(),
        },
        OptionDefinition {
            key: SGB_BORDER,
            description: "Super Game Boy border",
            info: "The border around the screen of games on a Super Game Boy: the game's own \
                   (Nubo's for games that bring none), always Nubo's, or none, which shows \
                   only the Game Boy screen.",
            values: vec![
                (String::from(SHOWN), String::from("The game's")),
                (String::from(NUBO_BORDER), String::from("Nubo's")),
                (String::from(HIDDEN), String::from("Hidden")),
            ],
        },
        OptionDefinition {
            key: RTC,
            description: "Cartridge clock",
            info: "Which time the clock of games such as Pokémon Gold follows. Real time keeps \
                   up with the real world, also while the game is closed, so a save state can \
                   go on differently when it is loaded later. Emulated time only counts the \
                   time the game ran, so every run is the same: choose it for netplay with \
                   games that have a clock. Takes effect when the game is restarted or loaded \
                   again; the clock keeps the time it shows.",
            values: vec![
                (String::from(REAL_TIME), String::from("Real time")),
                (String::from(EMULATED_TIME), String::from("Emulated time")),
            ],
        },
    ]
}

/// The settings the options stand for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// The hardware a game starts on.
    pub model: ModelPreference,
    /// The colors of original Game Boy games on a Game Boy.
    pub palette: DmgPalette,
    /// The colors of Super Game Boy games.
    pub sgb_colors: SgbColors,
    /// Whether a Super Game Boy's border shows around the screen.
    pub sgb_border: bool,
    /// Whether that border is Nubo's, even when the game sent one.
    pub nubo_border: bool,
    /// The clock an MBC3 cartridge's real-time clock follows.
    pub rtc_clock: RtcClock,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            model: ModelPreference::default(),
            palette: DmgPalette::default(),
            sgb_colors: SgbColors::Game,
            sgb_border: true,
            nubo_border: false,
            rtc_clock: RtcClock::default(),
        }
    }
}

impl Options {
    /// Sets the option `key` to `value`, as RetroArch stores it. Returns
    /// false, changing nothing, for a key or value the core doesn't know.
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        match key {
            MODEL => match ModelPreference::try_from(String::from(value)) {
                Ok(model) => self.model = model,
                Err(_) => return false,
            },
            PALETTE => {
                match DmgPalette::ALL
                    .into_iter()
                    .find(|palette| palette.to_string().eq_ignore_ascii_case(value))
                {
                    Some(palette) => self.palette = palette,
                    None => return false,
                }
            }
            SGB_COLORS => {
                if value == GAME_COLORS {
                    self.sgb_colors = SgbColors::Game;
                } else {
                    match DmgPalette::ALL
                        .into_iter()
                        .find(|palette| palette.to_string().eq_ignore_ascii_case(value))
                    {
                        Some(palette) => self.sgb_colors = SgbColors::Preset(palette),
                        None => return false,
                    }
                }
            }
            SGB_BORDER => {
                (self.sgb_border, self.nubo_border) = match value {
                    SHOWN => (true, false),
                    NUBO_BORDER => (true, true),
                    HIDDEN => (false, false),
                    _ => return false,
                };
            }
            RTC => match value {
                REAL_TIME => self.rtc_clock = RtcClock::WallClock,
                EMULATED_TIME => self.rtc_clock = RtcClock::EmulatedTime,
                _ => return false,
            },
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests;
