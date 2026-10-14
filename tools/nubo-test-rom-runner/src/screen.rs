//! Compares the emulator's screen with the reference screenshots of the
//! test suites.

use crate::suites::Model;
use image::RgbImage;
use std::path::Path;

pub(crate) const WIDTH: u32 = 160;
pub(crate) const HEIGHT: u32 = 144;
/// How far a color channel may be from the reference and still match.
const CHANNEL_TOLERANCE: u8 = 4;
/// How far a pixel's brightness may be from a GB Emulator Shootout
/// screenshot and still match, as in the shootout's own comparison.
const GRAYSCALE_TOLERANCE: u8 = 50;

/// Loads a reference screenshot and checks that it is the size of the
/// Game Boy screen.
pub(crate) fn load_reference(path: &Path) -> Result<RgbImage, String> {
    let image = image::open(path)
        .map_err(|error| format!("failed to load {}: {}", path.display(), error))?
        .to_rgb8();

    if image.dimensions() != (WIDTH, HEIGHT) {
        return Err(format!("{} is not {}x{}", path.display(), WIDTH, HEIGHT));
    }
    Ok(image)
}

/// Converts an RGBA frame from the emulator into the colors the reference
/// screenshots use for `model`. The original Game Boy's grays
/// (`DMG_SHADES`) are already the screenshots' own; Game Boy Color colors
/// are rounded the way the screenshots store them.
pub(crate) fn normalize(frame: &[u8], model: Model) -> RgbImage {
    if !model.is_cgb() {
        return as_shown(frame);
    }

    let pixels = frame
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]].map(cgb_channel))
        .collect();
    RgbImage::from_raw(WIDTH, HEIGHT, pixels).expect("the frame buffer is 160x144 RGBA")
}

/// An RGBA frame from the emulator in the colors the app shows it in, as
/// the GB Emulator Shootout's harness sees it: the original Game Boy's
/// grays (`DMG_SHADES`, which the app's default palette keeps) and the Game
/// Boy Color's colors as the core draws them.
pub(crate) fn as_shown(frame: &[u8]) -> RgbImage {
    let pixels = frame
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
        .collect();
    RgbImage::from_raw(WIDTH, HEIGHT, pixels).expect("the frame buffer is 160x144 RGBA")
}

/// A reference screenshot's brightness per pixel, for checking many frames
/// against it quickly in grayscale, as the GB Emulator Shootout compares.
pub(crate) struct GrayscaleReference {
    brightness: Vec<u8>,
}

impl GrayscaleReference {
    /// The brightness of every pixel of `reference`.
    pub(crate) fn new(reference: &RgbImage) -> Self {
        GrayscaleReference {
            brightness: reference.pixels().map(|pixel| brightness(*pixel)).collect(),
        }
    }

    /// Whether every pixel of the emulator's RGBA `frame`, in the colors
    /// the app shows it in (unlike [`normalize`], which rounds Game Boy
    /// Color colors as the screenshots store them), is within `GRAYSCALE_TOLERANCE`
    /// of the reference's brightness, as the Shootout's harness compares a
    /// screenshot of the app. Stops at the first pixel that isn't.
    pub(crate) fn matches_as_shown(&self, frame: &[u8]) -> bool {
        frame
            .as_chunks::<4>()
            .0
            .iter()
            .zip(&self.brightness)
            .all(|(pixel, expected)| {
                let actual = brightness(image::Rgb([pixel[0], pixel[1], pixel[2]]));
                actual.abs_diff(*expected) <= GRAYSCALE_TOLERANCE
            })
    }
}

/// Counts the pixels that don't match.
pub(crate) fn differing_pixels(actual: &RgbImage, expected: &RgbImage) -> usize {
    count_differences(actual, expected, pixels_match)
}

/// Counts the pixels whose brightness doesn't match.
pub(crate) fn differing_pixels_in_grayscale(actual: &RgbImage, expected: &RgbImage) -> usize {
    count_differences(actual, expected, grays_match)
}

/// A copy of `expected` with every pixel that differs from `actual` painted
/// red.
pub(crate) fn highlight_differences(actual: &RgbImage, expected: &RgbImage) -> RgbImage {
    paint_differences(actual, expected, pixels_match)
}

/// Like [`highlight_differences`], comparing brightness only.
pub(crate) fn highlight_differences_in_grayscale(
    actual: &RgbImage,
    expected: &RgbImage,
) -> RgbImage {
    paint_differences(actual, expected, grays_match)
}

/// Counts the pixels for which `matches` is false.
fn count_differences(
    actual: &RgbImage,
    expected: &RgbImage,
    matches: fn(image::Rgb<u8>, image::Rgb<u8>) -> bool,
) -> usize {
    actual
        .pixels()
        .zip(expected.pixels())
        .filter(|&(actual, expected)| !matches(*actual, *expected))
        .count()
}

/// A copy of `expected` with every pixel for which `matches` is false painted
/// red.
fn paint_differences(
    actual: &RgbImage,
    expected: &RgbImage,
    matches: fn(image::Rgb<u8>, image::Rgb<u8>) -> bool,
) -> RgbImage {
    let mut highlighted = expected.clone();
    for (pixel, actual) in highlighted.pixels_mut().zip(actual.pixels()) {
        if !matches(*actual, *pixel) {
            *pixel = image::Rgb([0xFF, 0x00, 0x00]);
        }
    }
    highlighted
}

/// Whether every channel is within `CHANNEL_TOLERANCE` of the reference.
fn pixels_match(actual: image::Rgb<u8>, expected: image::Rgb<u8>) -> bool {
    actual
        .0
        .iter()
        .zip(expected.0.iter())
        .all(|(actual, expected)| actual.abs_diff(*expected) <= CHANNEL_TOLERANCE)
}

/// Whether the brightness of the two pixels is within `GRAYSCALE_TOLERANCE`.
fn grays_match(actual: image::Rgb<u8>, expected: image::Rgb<u8>) -> bool {
    brightness(actual).abs_diff(brightness(expected)) <= GRAYSCALE_TOLERANCE
}

/// A pixel's brightness, weighted like the ITU-R 601 luma that the Pillow
/// library (which the shootout uses) converts to grayscale with.
fn brightness(pixel: image::Rgb<u8>) -> u8 {
    let [red, green, blue] = pixel.0.map(u32::from);
    ((red * 19_595 + green * 38_470 + blue * 7_471 + 0x8000) >> 16) as u8
}

/// Game Boy Color (CGB) colors have 5 bits per channel. This rounds an 8-bit
/// channel to 5 bits and expands it back the way the reference screenshots
/// do, by repeating the top bits.
fn cgb_channel(value: u8) -> u8 {
    let five_bit = (u32::from(value) * 31 + 127) / 255;
    ((five_bit << 3) | (five_bit >> 2)) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use nubo_core::DMG_SHADES;

    /// A white RGBA frame.
    fn white_frame() -> Vec<u8> {
        [0xFF; 4].repeat((WIDTH * HEIGHT) as usize)
    }

    /// A reference screenshot of one gray.
    fn gray_reference(gray: u8) -> GrayscaleReference {
        GrayscaleReference::new(&RgbImage::from_pixel(WIDTH, HEIGHT, image::Rgb([gray; 3])))
    }

    #[test]
    fn grays_match_within_the_shootouts_tolerance() {
        let frame = white_frame();
        assert!(gray_reference(0xFF - 50).matches_as_shown(&frame));
        assert!(!gray_reference(0xFF - 51).matches_as_shown(&frame));
    }

    #[test]
    fn one_differing_pixel_is_enough_to_differ() {
        let mut frame = white_frame();
        frame[4 * 1000..4 * 1000 + 3].fill(0);
        assert!(!gray_reference(0xFF).matches_as_shown(&frame));
        assert!(gray_reference(0xFF).matches_as_shown(&white_frame()));
    }

    #[test]
    fn the_original_game_boys_grays_are_the_screenshots_grays() {
        let grays = DMG_SHADES.map(|[red, green, blue, _alpha]| [red, green, blue]);
        assert_eq!(grays, [[0xFF; 3], [0xAA; 3], [0x55; 3], [0x00; 3]]);
    }
}
