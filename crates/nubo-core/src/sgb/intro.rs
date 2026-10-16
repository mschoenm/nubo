//! Nubo's intro on a Super Game Boy: what the Game Boy screen shows while the
//! Super Nintendo starts and holds the Game Boy in reset. The real Super Game
//! Boy plays an animation and a jingle of Nintendo's; Nubo shows its own
//! logo instead, silently, for as long (`INTRO_FRAMES` in `sgb.rs`).
//!
//! The logo fades in on a dark purple, stays, and fades out again before the
//! game's picture takes over.

use crate::sgb::INTRO_FRAMES;
use crate::sgb::nubo_border::nubo_colors;

/// The screen's width in pixels.
const SCREEN_WIDTH: usize = 160;
/// The screen's height in pixels.
const SCREEN_HEIGHT: usize = 144;
/// The logo's size in pixels.
const LOGO_SIZE: usize = 64;
/// The color behind the logo: the dark rim around the screen in Nubo's
/// border.
const BACKGROUND: [u8; 3] = nubo_colors::RIM;
/// When the logo starts fading in, in frames.
const FADE_IN_START: u16 = 20;
/// How long the logo takes to fade in, in frames.
const FADE_IN_FRAMES: u16 = 50;
/// How long the logo takes to fade out at the end, in frames.
const FADE_OUT_FRAMES: u16 = 40;

/// The intro's logo, drawn once.
#[derive(Clone, Debug)]
pub(super) struct Intro {
    /// The logo as RGBA pixels, `LOGO_SIZE` on each side.
    logo: Vec<u8>,
}

impl Intro {
    pub(super) fn new() -> Self {
        Intro {
            logo: nubo_logo::render(LOGO_SIZE as u32, 0.0),
        }
    }

    /// Draws frame `frame` (1 to `INTRO_FRAMES`) into the 160×144 RGBA
    /// `game_screen`.
    pub(super) fn draw(&self, frame: u16, game_screen: &mut [u8]) {
        let pixels = game_screen.as_chunks_mut::<4>().0;
        let [red, green, blue] = BACKGROUND;
        pixels.fill([red, green, blue, 0xFF]);

        let visibility = logo_visibility(frame);
        if visibility == 0.0 {
            return;
        }
        let (left, top) = (
            (SCREEN_WIDTH - LOGO_SIZE) / 2,
            (SCREEN_HEIGHT - LOGO_SIZE) / 2,
        );
        for (index, logo_pixel) in self.logo.as_chunks::<4>().0.iter().enumerate() {
            let (x, y) = (left + index % LOGO_SIZE, top + index / LOGO_SIZE);
            let amount = f32::from(logo_pixel[3]) / 255.0 * visibility;
            let pixel = &mut pixels[x + y * SCREEN_WIDTH];
            for channel in 0..3 {
                let mixed = f32::from(pixel[channel]) * (1.0 - amount)
                    + f32::from(logo_pixel[channel]) * amount;
                pixel[channel] = mixed.round() as u8;
            }
        }
    }
}

/// How visible the logo is in `frame`, from 0 to 1.
fn logo_visibility(frame: u16) -> f32 {
    let fade_out_start = INTRO_FRAMES - FADE_OUT_FRAMES;
    if frame < FADE_IN_START {
        0.0
    } else if frame < FADE_IN_START + FADE_IN_FRAMES {
        f32::from(frame - FADE_IN_START) / f32::from(FADE_IN_FRAMES)
    } else if frame < fade_out_start {
        1.0
    } else {
        f32::from(INTRO_FRAMES.saturating_sub(frame)) / f32::from(FADE_OUT_FRAMES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_logo_fades_in_and_out() {
        assert_eq!(logo_visibility(1), 0.0);
        assert!(logo_visibility(FADE_IN_START + FADE_IN_FRAMES / 2) > 0.4);
        assert_eq!(logo_visibility(100), 1.0);
        assert_eq!(logo_visibility(INTRO_FRAMES), 0.0);
    }
}
