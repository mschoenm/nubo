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
//! its own instead, from its logo (`nubo_logo`), with a screensaver of its
//! own too (`attraction.rs`).

use crate::sgb::attraction::{self, Cloud};
use crate::sgb::{BORDERED_HEIGHT, BORDERED_WIDTH, SCREEN_OFFSET};
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
    /// The RGB555 color of the border at `x`, `y` in the 256×224 picture, or
    /// `None` where it is transparent.
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
        Some(u16::from_le_bytes([self.map[index], self.map[index + 1]]))
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
const TRANSPARENT: u16 = 0x8000;

/// The colors of Nubo's border, as RGB 8-bit channels.
pub(super) mod nubo_colors {
    /// The frame around everything: a deep purple.
    pub(in crate::sgb) const BACKGROUND: [u8; 3] = [0x24, 0x1C, 0x5A];
    /// The raised panel the screen sits in.
    pub(super) const PANEL: [u8; 3] = [0x34, 0x29, 0x8A];
    /// The thin dark rim right around the screen.
    pub(super) const RIM: [u8; 3] = [0x14, 0x0E, 0x38];
}

/// The panel the screen sits in: its left, top, right and bottom edges and
/// the radius of its corners, in pixels.
const PANEL: (f32, f32, f32, f32, f32) = (36.0, 28.0, 220.0, 196.0, 10.0);
/// The rim around the screen.
const RIM: (f32, f32, f32, f32, f32) = (44.0, 36.0, 212.0, 188.0, 4.0);
/// The size of the logo below the panel, and where its top-left corner is.
const LOGO_SIZE: usize = 20;
const LOGO_POSITION: (usize, usize) = (118, 200);
/// How many samples per pixel edge the shapes are drawn with, to smooth their
/// edges.
const SAMPLES: usize = 4;

/// Nubo's own border: the screen sits in a rounded purple panel on a deep
/// purple frame, with the Nubo logo below it. Besides the picture, it keeps
/// what the attraction mode (`attraction.rs`) needs to let clouds drift over
/// the frame, behind the panel and the logo.
#[derive(Clone, Debug, Default)]
pub(super) struct NuboBorder {
    /// The border as it normally shows.
    pub(super) picture: BorderPicture,
    /// The same picture as RGB 8-bit colors, row by row.
    pub(super) colors: Vec<[u8; 3]>,
    /// How much of each pixel is the open frame (`nubo_colors::BACKGROUND`)
    /// rather than the panel or the logo, from 0 to 1.
    pub(super) open_frame: Vec<f32>,
    /// The clouds of the attraction mode.
    pub(super) clouds: Vec<Cloud>,
}

impl NuboBorder {
    pub(super) fn new() -> Self {
        let mut colors = vec![nubo_colors::BACKGROUND; BORDERED_WIDTH * BORDERED_HEIGHT];
        let mut open_frame = vec![1.0; BORDERED_WIDTH * BORDERED_HEIGHT];
        for (shape, color) in [(PANEL, nubo_colors::PANEL), (RIM, nubo_colors::RIM)] {
            fill_rounded_rectangle(&mut colors, &mut open_frame, shape, color);
        }

        let logo = nubo_logo::render(LOGO_SIZE as u32, 0.0);
        for (index, pixel) in logo.as_chunks::<4>().0.iter().enumerate() {
            let (x, y) = (
                LOGO_POSITION.0 + index % LOGO_SIZE,
                LOGO_POSITION.1 + index / LOGO_SIZE,
            );
            let amount = f32::from(pixel[3]) / 255.0;
            let color = &mut colors[x + y * BORDERED_WIDTH];
            *color = blend(*color, [pixel[0], pixel[1], pixel[2]], amount);
            open_frame[x + y * BORDERED_WIDTH] *= 1.0 - amount;
        }

        let pixels = colors
            .iter()
            .enumerate()
            .map(|(index, &color)| {
                if in_game_screen(index % BORDERED_WIDTH, index / BORDERED_WIDTH) {
                    TRANSPARENT
                } else {
                    to_rgb555(color)
                }
            })
            .collect();
        NuboBorder {
            picture: BorderPicture { pixels },
            colors,
            open_frame,
            clouds: attraction::clouds(),
        }
    }
}

/// Whether `x`, `y` in the 256×224 picture is on the game screen.
fn in_game_screen(x: usize, y: usize) -> bool {
    let (screen_left, screen_top) = SCREEN_OFFSET;
    (screen_left..screen_left + 160).contains(&x) && (screen_top..screen_top + 144).contains(&y)
}

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

/// Which border to draw: a game's, or Nubo's at a frame of the attraction
/// mode (`None` when it isn't running).
#[derive(Clone, Copy, Debug)]
pub(super) enum BorderSource<'a> {
    Game(&'a Border),
    Nubo { attraction_frame: Option<u32> },
}

/// A border the cache drew, to tell whether it has to draw again.
#[derive(Clone, Debug)]
enum DrawnSource {
    Game(Box<Border>),
    Nubo { attraction_frame: Option<u32> },
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
    pub(in crate::sgb) fn update(&mut self, source: BorderSource, nubo_border: &NuboBorder) {
        let up_to_date = match (&self.source, source) {
            (Some(DrawnSource::Game(drawn)), BorderSource::Game(border)) => {
                drawn.tiles == border.tiles && drawn.map == border.map
            }
            (
                Some(DrawnSource::Nubo { attraction_frame }),
                BorderSource::Nubo {
                    attraction_frame: frame,
                },
            ) => *attraction_frame == frame,
            _ => false,
        };
        if up_to_date {
            return;
        }

        self.picture = match source {
            BorderSource::Game(border) => BorderPicture::from_border(border),
            BorderSource::Nubo {
                attraction_frame: None,
            } => nubo_border.picture.clone(),
            BorderSource::Nubo {
                attraction_frame: Some(frame),
            } => attraction::draw(nubo_border, frame),
        };
        let (screen_left, screen_top) = SCREEN_OFFSET;
        self.screen_cover = (0..160 * 144)
            .filter_map(|index| {
                let (x, y) = (screen_left + index % 160, screen_top + index / 160);
                let color = self.picture.pixels[x + y * BORDERED_WIDTH];
                (color != TRANSPARENT).then_some((index as u16, color))
            })
            .collect();
        self.source = Some(match source {
            BorderSource::Game(border) => DrawnSource::Game(Box::new(border.clone())),
            BorderSource::Nubo { attraction_frame } => DrawnSource::Nubo { attraction_frame },
        });
        self.drawn_with = None;
    }
}

/// Paints a rounded rectangle (left, top, right, bottom, corner radius) over
/// `colors`, with its edges smoothed, and takes what it covers out of
/// `open_frame`.
fn fill_rounded_rectangle(
    colors: &mut [[u8; 3]],
    open_frame: &mut [f32],
    (left, top, right, bottom, radius): (f32, f32, f32, f32, f32),
    color: [u8; 3],
) {
    let contains = |x: f32, y: f32| {
        if x < left || y < top || x >= right || y >= bottom {
            return false;
        }
        let nearest_x = x.clamp(left + radius, right - radius);
        let nearest_y = y.clamp(top + radius, bottom - radius);
        let (dx, dy) = (x - nearest_x, y - nearest_y);
        dx * dx + dy * dy <= radius * radius
    };
    for (index, (pixel, open)) in colors.iter_mut().zip(open_frame).enumerate() {
        let (x, y) = (index % BORDERED_WIDTH, index / BORDERED_WIDTH);
        let mut covered = 0;
        for sample_y in 0..SAMPLES {
            for sample_x in 0..SAMPLES {
                let offset = |sample: usize| (sample as f32 + 0.5) / SAMPLES as f32;
                if contains(x as f32 + offset(sample_x), y as f32 + offset(sample_y)) {
                    covered += 1;
                }
            }
        }
        if covered > 0 {
            let amount = covered as f32 / (SAMPLES * SAMPLES) as f32;
            *pixel = blend(*pixel, color, amount);
            *open *= 1.0 - amount;
        }
    }
}

/// Mixes `over` into `under`, `amount` (0 to 1) of the way.
fn blend(under: [u8; 3], over: [u8; 3], amount: f32) -> [u8; 3] {
    std::array::from_fn(|channel| {
        let mixed = f32::from(under[channel]) * (1.0 - amount) + f32::from(over[channel]) * amount;
        mixed.round() as u8
    })
}

/// The nearest RGB555 color to an RGB color with 8-bit channels.
pub(super) fn to_rgb555(color: [u8; 3]) -> u16 {
    let channel = |value: u8| (u16::from(value) * 31 + 127) / 255;
    channel(color[0]) | channel(color[1]) << 5 | channel(color[2]) << 10
}
