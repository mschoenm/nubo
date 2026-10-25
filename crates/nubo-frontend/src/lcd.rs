//! What the Game Boy's own screen did to the picture that a modern screen
//! doesn't, for an app to put back if the player wants:
//!
//! - **Color correction.** A game sets each Game Boy Color color as five
//!   bits of red, green and blue, which the emulator scales evenly to
//!   eight. The Game Boy Color's screen showed them darker in the middle
//!   tones, less saturated and with less contrast, and games were drawn for
//!   that, so on a modern screen they look too bright and garish.
//!   [`correct_colors`] makes them look closer to the real screen with a
//!   simple model of its own: the screen's response to the color's level,
//!   a little light from each subpixel spilling into its neighbors, and a
//!   slightly lighter black and darker white.
//! - **Frame blending.** The Game Boy's screen was slow to change, so a
//!   sprite a game shows only every other frame looked see-through instead
//!   of flickering. [`blend`] mixes each frame with the one before it.
//!
//! Both work on the picture after the emulator made it, so the emulation
//! itself never changes. The settings' one switch for both, the original
//! screen look, turns on only the one each model's screen had
//! ([`ScreenEffects`], see `GameSession::set_screen_look`).

use crate::palettes::DmgPalette;
use crate::picture::{Frame, Picture};
use nubo_core::Model;
use std::sync::OnceLock;

/// What the picture goes through to look like the model's own screen, when
/// the player wants the original screen look.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScreenEffects {
    /// Whether the colors are shown as on a Game Boy Color's screen (see
    /// [`correct_colors`]).
    pub correct_colors: bool,
    /// Whether each frame is mixed with the one before, as a slow screen
    /// showed it (see [`blend`]).
    pub blend_frames: bool,
}

impl ScreenEffects {
    /// The effects of `model`'s own screen, or none without the
    /// `original_screen` look. A Game Boy Color's colors are corrected, and
    /// an original Game Boy's or Pocket's slow screen blends frames. The Game
    /// Boy Color's screen was fast enough not to, the Game Boy Advance's
    /// showed colors differently, and a Super Game Boy's TV did neither.
    pub fn of(model: Model, original_screen: bool) -> Self {
        ScreenEffects {
            correct_colors: original_screen && model.is_cgb() && model != Model::Agb,
            blend_frames: original_screen && matches!(model, Model::Dmg0 | Model::Dmg | Model::Mgb),
        }
    }

    /// `frame`, as the emulator drew it, as the player sees it through the
    /// effects: in `palette` and with or without a Super Game Boy's border
    /// (see `Frame::shown`), mixed with `previous`, the frame before as the
    /// emulator drew it, when frames blend and there is one, and with its
    /// colors corrected. Both frames are repainted in the palette before
    /// they are mixed: the palette only knows the emulator's four grays, so
    /// it can't repaint the shades in between that mixing makes. Frames in
    /// the Game Boy's grays and Super Game Boy frames, which a TV showed,
    /// keep their colors.
    pub fn show(
        self,
        frame: &Frame,
        previous: Option<&Frame>,
        palette: DmgPalette,
        with_border: bool,
    ) -> Picture {
        let picture = if self.correct_colors && !frame.in_dmg_shades && frame.sgb_menu.is_none() {
            let mut corrected = frame.clone();
            correct_colors(&mut corrected.pixels);
            corrected.shown(palette, with_border)
        } else {
            frame.shown(palette, with_border)
        };
        match previous {
            Some(previous) if self.blend_frames => {
                let mut pixels = picture.pixels().to_vec();
                blend(&mut pixels, previous.shown(palette, with_border).pixels());
                Picture::new(picture.width(), picture.height(), pixels)
            }
            _ => picture,
        }
    }
}

/// How the screen's levels respond to a color's: darker in the middle.
const SCREEN_GAMMA: f64 = 2.2;
/// How much of each channel's light ends up in each channel, rows red,
/// green and blue. Each row adds up to 1, so white stays white.
const BLEED: [[f64; 3]; 3] = [[0.90, 0.07, 0.03], [0.04, 0.86, 0.10], [0.04, 0.10, 0.86]];
/// The darkest and lightest levels the screen shows, out of 1.
const BLACK: f64 = 0.04;
const WHITE: f64 = 0.96;

/// Shows the RGBA `pixels` of a Game Boy Color picture as its screen would
/// have, in place. The pixels must be the emulator's own, five-bit levels
/// scaled to eight bits; others are corrected as their nearest such color.
pub fn correct_colors(pixels: &mut [u8]) {
    let table = table();
    let (pixels, _) = pixels.as_chunks_mut::<4>();
    for pixel in pixels {
        let [red, green, blue, _] = pixel.map(five_bits);
        let index = usize::from(red) | usize::from(green) << 5 | usize::from(blue) << 10;
        pixel[..3].copy_from_slice(&table[index]);
    }
}

/// Mixes `previous`, the frame before, into the RGBA `pixels` half and half,
/// as the Game Boy's slow screen did. Pictures of different sizes are left
/// alone.
pub fn blend(pixels: &mut [u8], previous: &[u8]) {
    if pixels.len() != previous.len() {
        return;
    }
    for (pixel, previous) in pixels.iter_mut().zip(previous) {
        *pixel = (u16::from(*pixel) + u16::from(*previous)).div_ceil(2) as u8;
    }
}

/// An eight-bit level as the five-bit level it was scaled from.
fn five_bits(level: u8) -> u8 {
    ((u16::from(level) * 31 + 127) / 255) as u8
}

/// The corrected red, green and blue of every five-bit color, indexed by
/// the color as RGB555 (red in the low bits), made once.
fn table() -> &'static [[u8; 3]] {
    static TABLE: OnceLock<Vec<[u8; 3]>> = OnceLock::new();
    TABLE.get_or_init(|| (0..1 << 15).map(corrected).collect())
}

/// The corrected red, green and blue of the RGB555 color `color`.
fn corrected(color: u16) -> [u8; 3] {
    let level = |shift: u16| (f64::from((color >> shift) & 0x1F) / 31.0).powf(SCREEN_GAMMA);
    let light = [level(0), level(5), level(10)];
    BLEED.map(|row| {
        let mixed: f64 = row
            .iter()
            .zip(light)
            .map(|(share, level)| share * level)
            .sum();
        let shown = BLACK + (WHITE - BLACK) * mixed.powf(1.0 / SCREEN_GAMMA);
        (shown * 255.0).round() as u8
    })
}

#[cfg(test)]
mod tests;
