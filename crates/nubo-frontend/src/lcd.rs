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
//!   a little light from each subpixel spilling into its neighbours, and a
//!   slightly lighter black and darker white.
//! - **Frame blending.** The Game Boy's screen was slow to change, so a
//!   sprite a game shows only every other frame looked see-through instead
//!   of flickering. [`blend`] mixes each frame with the one before it.
//!
//! Both work on the picture after the emulator made it, so the emulation
//! itself never changes.

use std::sync::OnceLock;

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
