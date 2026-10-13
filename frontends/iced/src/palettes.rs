//! Colors for original Game Boy games. The emulator draws them in four
//! shades of gray (`nubo_core::DMG_SHADES`); this swaps those for the colors
//! of the player's chosen palette, such as the green of the original screen.
//! The core always draws gray, and the palette is up to the app.

use nubo_core::DMG_SHADES;
use serde::{Deserialize, Serialize};
use std::fmt;

/// The colors original Game Boy games are shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DmgPalette {
    /// Plain grays, as the emulator draws them.
    #[default]
    Gray,
    /// The yellow-green of the original Game Boy's screen.
    Green,
    /// The olive tones of the Game Boy Pocket.
    Pocket,
}

impl DmgPalette {
    pub const ALL: [DmgPalette; 3] = [DmgPalette::Gray, DmgPalette::Green, DmgPalette::Pocket];

    /// The palette's four colors, lightest first.
    pub fn colors(self) -> [[u8; 3]; 4] {
        match self {
            DmgPalette::Gray => DMG_SHADES.map(|[red, green, blue, _alpha]| [red, green, blue]),
            DmgPalette::Green => [
                [0x9B, 0xBC, 0x0F],
                [0x8B, 0xAC, 0x0F],
                [0x30, 0x62, 0x30],
                [0x0F, 0x38, 0x0F],
            ],
            DmgPalette::Pocket => [
                [0xC4, 0xCF, 0xA1],
                [0x8B, 0x95, 0x6D],
                [0x4D, 0x53, 0x3C],
                [0x1F, 0x1F, 0x1F],
            ],
        }
    }

    /// Repaints a frame of RGBA pixels drawn in the emulator's grays with
    /// this palette. Pixels of any other color are left alone.
    pub fn recolor(self, pixels: &mut [u8]) {
        if self == DmgPalette::Gray {
            return;
        }

        let colors = self.colors();
        for pixel in pixels.as_chunks_mut::<4>().0 {
            if let Some(shade) = DMG_SHADES.iter().position(|gray| pixel[..3] == gray[..3]) {
                pixel[..3].copy_from_slice(&colors[shade]);
            }
        }
    }
}

impl fmt::Display for DmgPalette {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            DmgPalette::Gray => "Gray",
            DmgPalette::Green => "Green",
            DmgPalette::Pocket => "Pocket",
        })
    }
}

#[cfg(test)]
mod tests;
