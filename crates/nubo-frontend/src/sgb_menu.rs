//! The Super Game Boy's menu as Nubo's apps offer it: which colors the game
//! shows (the game's own, one of Nubo's palettes, or the player's own) and
//! which border shows around the screen. A real Super Game Boy has a menu for
//! this, opened with the Super Nintendo's L and R buttons; the emulator keeps
//! the choices (`Emulator::set_sgb_player_palette`, `Emulator::set_sgb_border`)
//! until the game starts again, also when a save state loads, since they are
//! the player's rather than the game's. Each app draws a page of its own for
//! them.
//!
//! Nubo's own border has a night and a day look, which follow the player's
//! clock ([`BorderClock`]) rather than a choice in the menu.

use crate::palettes::DmgPalette;
use chrono::Timelike;
use nubo_core::{Emulator, SgbBorder, SgbBorderTime};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// What the emulator's Super Game Boy says about its menu, taken with each
/// frame so an app's menu page shows what the game shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SgbMenu {
    /// The player's palette while it colors the screen, as RGB555 colors
    /// from the lightest shade to the darkest.
    pub player_palette: Option<[u16; 4]>,
    /// Whether the game lets the player pick colors.
    pub palettes_allowed: bool,
    /// The border the player picked.
    pub border: SgbBorder,
}

impl SgbMenu {
    /// The menu of `emulator`'s Super Game Boy, or `None` on other models.
    pub fn of(emulator: &Emulator) -> Option<SgbMenu> {
        emulator.model().is_sgb().then(|| SgbMenu {
            player_palette: emulator.sgb_player_palette(),
            palettes_allowed: emulator.sgb_player_palette_allowed(),
            border: emulator.sgb_border(),
        })
    }

    /// Which of the colors the player can pick show now, given the player's
    /// own palette `own`.
    pub fn colors(&self, own: [u16; 4]) -> SgbColors {
        // A preset that is also the player's own palette counts as their
        // own, so the menu shows the colors they are editing as picked.
        match self.player_palette {
            None => SgbColors::Game,
            Some(palette) if palette == own => SgbColors::Own,
            Some(palette) => DmgPalette::ALL
                .into_iter()
                .find(|preset| preset.rgb555() == palette)
                .map_or(SgbColors::Own, SgbColors::Preset),
        }
    }
}

/// The hour of the day, on the player's clock, at which Nubo's Super Game
/// Boy border turns to its day look.
pub const DAY_STARTS_AT: u32 = 7;
/// The hour at which it turns back to its night look.
pub const NIGHT_STARTS_AT: u32 = 19;
/// How many frames go by between looks at the clock: about ten seconds of
/// play.
const FRAMES_BETWEEN_CHECKS: u32 = 600;

/// Which look Nubo's Super Game Boy border has at `hour` (0 to 23) of the
/// day.
pub fn border_time_at(hour: u32) -> SgbBorderTime {
    if (DAY_STARTS_AT..NIGHT_STARTS_AT).contains(&hour) {
        SgbBorderTime::Day
    } else {
        SgbBorderTime::Night
    }
}

/// Which look Nubo's Super Game Boy border has now, by the player's local
/// clock.
pub fn border_time_now() -> SgbBorderTime {
    border_time_at(chrono::Local::now().hour())
}

/// Keeps Nubo's Super Game Boy border in step with the player's clock: its
/// day look from [`DAY_STARTS_AT`] and its night look from
/// [`NIGHT_STARTS_AT`]. It looks at the clock when a game starts and every
/// ten seconds or so of play after that. Other models have no border and
/// are left alone.
#[derive(Debug, Default)]
pub struct BorderClock {
    /// Frames since the clock was last looked at.
    frames_since_check: u32,
}

impl BorderClock {
    /// Gives `emulator`'s border the look for the time now, for a game that
    /// just started.
    pub fn start(emulator: &mut Emulator) -> BorderClock {
        let mut clock = BorderClock::default();
        clock.check(emulator);
        clock
    }

    /// Counts a finished frame, and every 600 frames (about ten seconds)
    /// turns the border to the look the clock says.
    pub fn frame_finished(&mut self, emulator: &mut Emulator) {
        self.frames_since_check += 1;
        if self.frames_since_check >= FRAMES_BETWEEN_CHECKS {
            self.check(emulator);
        }
    }

    /// Gives the border the look for the time now, if it has another.
    fn check(&mut self, emulator: &mut Emulator) {
        self.frames_since_check = 0;
        if !emulator.model().is_sgb() {
            return;
        }
        let time = border_time_now();
        if emulator.sgb_border_time() != time {
            emulator.set_sgb_border_time(time);
        }
    }
}

/// The colors a player can pick for a Super Game Boy game. Settings store
/// them by `stored_name`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SgbColors {
    /// The game's own colors, or the Game Boy's grays until it sets some.
    #[default]
    Game,
    /// One of Nubo's palettes for the whole screen.
    Preset(DmgPalette),
    /// The player's own palette (see [`DEFAULT_OWN_PALETTE`]).
    Own,
}

impl SgbColors {
    /// Every choice, in the order a menu lists them.
    pub const ALL: [SgbColors; 5] = [
        SgbColors::Game,
        SgbColors::Preset(DmgPalette::Gray),
        SgbColors::Preset(DmgPalette::Green),
        SgbColors::Preset(DmgPalette::Pocket),
        SgbColors::Own,
    ];

    /// The palette to give the emulator for this choice (`None` for the
    /// game's colors), with the player's own palette `own`.
    pub fn palette(self, own: [u16; 4]) -> Option<[u16; 4]> {
        match self {
            SgbColors::Game => None,
            SgbColors::Preset(preset) => Some(preset.rgb555()),
            SgbColors::Own => Some(own),
        }
    }

    /// The name settings store the choice under, the same as RetroArch's
    /// option values.
    pub fn stored_name(self) -> &'static str {
        match self {
            SgbColors::Game => "game",
            SgbColors::Preset(DmgPalette::Gray) => "gray",
            SgbColors::Preset(DmgPalette::Green) => "green",
            SgbColors::Preset(DmgPalette::Pocket) => "pocket",
            SgbColors::Own => "own",
        }
    }

    /// The choice's name, for a menu.
    pub fn label(self) -> &'static str {
        match self {
            SgbColors::Game => "The game's colors",
            SgbColors::Preset(DmgPalette::Gray) => "Gray",
            SgbColors::Preset(DmgPalette::Green) => "Green",
            SgbColors::Preset(DmgPalette::Pocket) => "Pocket",
            SgbColors::Own => "Your own colors",
        }
    }
}

impl Serialize for SgbColors {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.stored_name())
    }
}

impl<'de> Deserialize<'de> for SgbColors {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        SgbColors::ALL
            .into_iter()
            .find(|colors| colors.stored_name() == name)
            .ok_or_else(|| {
                serde::de::Error::custom(format!("unknown Super Game Boy colors: {name}"))
            })
    }
}

/// The player's own palette until they change it: a cool blue, lightest
/// first, in RGB555.
pub const DEFAULT_OWN_PALETTE: [u16; 4] = [
    rgb555([0xE8, 0xF0, 0xF8]),
    rgb555([0x88, 0xA8, 0xD8]),
    rgb555([0x38, 0x50, 0x90]),
    rgb555([0x10, 0x18, 0x38]),
];

/// The red, green or blue part of an RGB555 color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorChannel {
    /// The lowest five bits.
    Red,
    /// The middle five bits.
    Green,
    /// The highest five bits.
    Blue,
}

impl ColorChannel {
    /// The three channels, in the order a color editor lists them.
    pub const ALL: [ColorChannel; 3] = [ColorChannel::Red, ColorChannel::Green, ColorChannel::Blue];

    /// The highest value a channel takes.
    pub const MAX: u8 = 31;

    /// Where the channel's five bits are in an RGB555 color.
    fn shift(self) -> u16 {
        match self {
            ColorChannel::Red => 0,
            ColorChannel::Green => 5,
            ColorChannel::Blue => 10,
        }
    }

    /// The channel's value (0 to 31) in `color`.
    pub fn of(self, color: u16) -> u8 {
        ((color >> self.shift()) & 0x1F) as u8
    }

    /// `color` with this channel set to `value` (kept within 0 to 31).
    pub fn with(self, color: u16, value: u8) -> u16 {
        let value = u16::from(value.min(Self::MAX));
        color & !(0x1F << self.shift()) | value << self.shift()
    }

    /// The channel's name, for a color editor.
    pub fn label(self) -> &'static str {
        match self {
            ColorChannel::Red => "Red",
            ColorChannel::Green => "Green",
            ColorChannel::Blue => "Blue",
        }
    }
}

/// The nearest RGB555 color to an RGB color with 8-bit channels.
pub const fn rgb555([red, green, blue]: [u8; 3]) -> u16 {
    const fn channel(value: u8) -> u16 {
        (value as u16 * 31 + 127) / 255
    }
    channel(red) | channel(green) << 5 | channel(blue) << 10
}

/// An RGB555 color as RGB with 8-bit channels, as the emulator shows it.
pub fn rgb(color: u16) -> [u8; 3] {
    ColorChannel::ALL.map(|channel| {
        let value = u16::from(channel.of(color));
        (value * 0xFF / 31) as u8
    })
}

#[cfg(test)]
mod tests;
