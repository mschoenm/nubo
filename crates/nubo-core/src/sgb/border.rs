/*
    How a game's border is decoded (the tiles, the map entries with their palette and flip bits)
    is ported from SameBoy's sgb.c (https://github.com/LIJI32/SameBoy, Expat License,
    Copyright (c) 2015-2026 Lior Halphon; see the notice in apu.rs).
*/

//! The border around the Game Boy screen on a Super Game Boy: a game's own,
//! sent as Super Nintendo tiles, or Nubo's.
//!
//! A game's border is a Super Nintendo background: 256 tiles of 8×8 pixels
//! with 16 colors each (four bits a pixel), a map of 32×28 tiles saying which
//! tile goes where, and four palettes of 16 RGB555 colors. Color 0 of every
//! palette is transparent, so the Game Boy screen shows through it in the
//! middle and the game's color 0 fills it elsewhere.
//!
//! The Super Game Boy's built-in borders are Nintendo's artwork, so Nubo draws
//! its own instead (`nubo_border.rs`), with a screensaver of its own too
//! (`attraction.rs`).

use crate::sgb::attraction;
use crate::sgb::nubo_border::NuboBorder;
use crate::sgb::{
    BORDERED_HEIGHT, BORDERED_WIDTH, SCREEN_OFFSET, SCREEN_PIXELS, SCREEN_WIDTH, SgbBorderTime,
};
use nubo_serializable_derive::Serializable;

/// The bytes of the border's tiles: 256 tiles of 32 bytes.
const TILES_SIZE: usize = 256 * 32;
/// The bytes `PCT_TRN` sends: the 32×32 tile map, two bytes an entry, and
/// then 64 colors of two bytes for the four palettes.
const MAP_SIZE: usize = 32 * 32 * 2 + 64 * 2;
/// Where the palettes start in `Border::map`.
const PALETTES_START: usize = 32 * 32 * 2;

/// A border a game sends.
#[derive(Clone, Debug, Serializable)]
pub(super) struct Border {
    /// The 256 tiles, in the Super Nintendo's four-bit format: for each row
    /// of a tile, its first two bit planes, and after the eight rows, the
    /// other two.
    pub(super) tiles: [u8; TILES_SIZE],
    /// The tile map (32×32 little-endian entries, of which the first 28 rows
    /// show), followed by the four palettes as little-endian RGB555.
    pub(super) map: [u8; MAP_SIZE],
    /// Whether a game sent the map. Until one does, Nubo's border shows.
    pub(super) from_game: bool,
}

impl Default for Border {
    fn default() -> Self {
        Border {
            tiles: [0; TILES_SIZE],
            map: [0; MAP_SIZE],
            from_game: false,
        }
    }
}

impl Border {
    /// The RGB555 color of the border at `x`, `y` in the 256×224 picture
    /// (with the unused top bit clear), or `None` where it is transparent.
    ///
    /// Each map entry holds the tile number in bits 0-7, the palette in bits
    /// 10-11, and flips the tile horizontally with bit 14 and vertically with
    /// bit 15. Entries with bit 8 or 9 set point past the 256 tiles and show
    /// nothing.
    pub(super) fn pixel(&self, x: usize, y: usize) -> Option<u16> {
        let entry_index = (x / 8 + y / 8 * 32) * 2;
        let entry = u16::from_le_bytes([self.map[entry_index], self.map[entry_index + 1]]);
        if entry & 0x0300 != 0 {
            return None;
        }
        let column = if entry & 0x4000 != 0 {
            x % 8
        } else {
            7 - x % 8
        };
        let row = if entry & 0x8000 != 0 {
            7 - y % 8
        } else {
            y % 8
        };
        let base = usize::from(entry & 0xFF) * 32 + row * 2;
        let bit = |offset: usize, value: u8| {
            if self.tiles[base + offset] & (1 << column) != 0 {
                value
            } else {
                0
            }
        };
        let color = bit(0, 1) | bit(1, 2) | bit(16, 4) | bit(17, 8);
        if color == 0 {
            return None;
        }
        let palette = usize::from((entry >> 10) & 0x03);
        let index = PALETTES_START + (palette * 16 + usize::from(color)) * 2;
        // The Super Nintendo ignores a color's top bit, which `TRANSPARENT` uses.
        Some(u16::from_le_bytes([self.map[index], self.map[index + 1]]) & 0x7FFF)
    }
}

/// A border drawn ahead as a whole picture of RGB555 colors.
#[derive(Clone, Debug, Default)]
pub(super) struct BorderPicture {
    /// 256×224 colors, row by row, with `TRANSPARENT` where the game screen
    /// shows.
    pub(super) pixels: Vec<u16>,
}

/// Marks a transparent pixel in a `BorderPicture`. RGB555 colors leave the
/// top bit unused.
pub(super) const TRANSPARENT: u16 = 0x8000;

impl BorderPicture {
    /// A game's border, drawn as a whole picture.
    fn from_border(border: &Border) -> Self {
        let pixels = (0..BORDERED_WIDTH * BORDERED_HEIGHT)
            .map(|index| {
                border
                    .pixel(index % BORDERED_WIDTH, index / BORDERED_WIDTH)
                    .unwrap_or(TRANSPARENT)
            })
            .collect();
        BorderPicture { pixels }
    }

    /// Every pixel's RGB555 color, row by row, or `None` where it is
    /// transparent.
    pub(super) fn pixels(&self) -> impl Iterator<Item = Option<u16>> + '_ {
        self.pixels
            .iter()
            .map(|&color| (color != TRANSPARENT).then_some(color))
    }
}

/// Which border to draw: a game's, or Nubo's in the look for `time`, at a
/// frame of the attraction mode (`None` when it isn't running).
#[derive(Clone, Copy, Debug)]
pub(super) enum BorderSource<'a> {
    Game(&'a Border),
    Nubo {
        time: SgbBorderTime,
        attraction_frame: Option<u32>,
    },
}

/// A border the cache drew, to tell whether it has to draw again.
#[derive(Clone, Debug)]
enum DrawnSource {
    Game(Box<Border>),
    Nubo {
        time: SgbBorderTime,
        attraction_frame: Option<u32>,
    },
}

/// The border as last drawn, kept so that it is only drawn again when it
/// changes. Save states leave it out: it is worked out again from the border
/// whenever that differs.
#[derive(Clone, Debug, Default)]
pub(in crate::sgb) struct BorderCache {
    /// The border `picture` was drawn from. Empty until the first frame.
    source: Option<DrawnSource>,
    /// The border as RGB555 colors.
    pub(in crate::sgb) picture: BorderPicture,
    /// The pixels of the border that cover the game screen: their index in
    /// the 160×144 screen and their RGB555 color.
    pub(in crate::sgb) screen_cover: Vec<(u16, u16)>,
    /// The fade and color 0 the picture with the border was last drawn with,
    /// or `None` when it has to be drawn again.
    pub(in crate::sgb) drawn_with: Option<(u8, [u8; 4])>,
}

impl BorderCache {
    /// Draws the border again if it isn't the one `source` names.
    pub(in crate::sgb) fn update(&mut self, source: BorderSource) {
        let up_to_date = match (&self.source, source) {
            (Some(DrawnSource::Game(drawn)), BorderSource::Game(border)) => {
                drawn.tiles == border.tiles && drawn.map == border.map
            }
            (
                Some(DrawnSource::Nubo {
                    time: drawn_time,
                    attraction_frame: drawn_frame,
                }),
                BorderSource::Nubo {
                    time,
                    attraction_frame,
                },
            ) => *drawn_time == time && *drawn_frame == attraction_frame,
            _ => false,
        };
        if up_to_date {
            return;
        }

        self.picture = match source {
            BorderSource::Game(border) => BorderPicture::from_border(border),
            BorderSource::Nubo {
                time,
                attraction_frame: None,
            } => NuboBorder::get(time).picture.clone(),
            BorderSource::Nubo {
                time,
                attraction_frame: Some(frame),
            } => attraction::draw(NuboBorder::get(time), frame),
        };
        let (screen_left, screen_top) = SCREEN_OFFSET;
        self.screen_cover = (0..SCREEN_PIXELS)
            .filter_map(|index| {
                let (x, y) = (
                    screen_left + index % SCREEN_WIDTH,
                    screen_top + index / SCREEN_WIDTH,
                );
                let color = self.picture.pixels[x + y * BORDERED_WIDTH];
                (color != TRANSPARENT).then_some((index as u16, color))
            })
            .collect();
        self.source = Some(match source {
            BorderSource::Game(border) => DrawnSource::Game(Box::new(border.clone())),
            BorderSource::Nubo {
                time,
                attraction_frame,
            } => DrawnSource::Nubo {
                time,
                attraction_frame,
            },
        });
        self.drawn_with = None;
    }
}
