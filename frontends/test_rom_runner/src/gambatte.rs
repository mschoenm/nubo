//! Checks the results of the Gambatte test ROMs the way Gambatte's own test
//! runner does (test/testrunner.cpp in
//! https://github.com/pokemon-speedrunning/gambatte-core): after a fixed
//! number of frames, a test either shows a hexadecimal number in the top-left
//! corner, plays sound or stays silent, or matches a reference screenshot.

use crate::screen;
use crate::suites::Model;
use image::RgbImage;

/// Gambatte checks each test this many frames after the boot ROM exits.
pub(crate) const FRAMES_AFTER_BOOT: u32 = 15;

/// The hexadecimal digits the Gambatte tests draw, one 8x8 tile each. Each
/// byte is a row, with the leftmost pixel in the highest bit; a set bit is
/// black and a clear bit is white.
const DIGIT_TILES: [[u8; 8]; 16] = [
    [
        0b0000_0000,
        0b0111_1111,
        0b0100_0001,
        0b0100_0001,
        0b0100_0001,
        0b0100_0001,
        0b0100_0001,
        0b0111_1111,
    ], // 0
    [
        0b0000_0000,
        0b0000_1000,
        0b0000_1000,
        0b0000_1000,
        0b0000_1000,
        0b0000_1000,
        0b0000_1000,
        0b0000_1000,
    ], // 1
    [
        0b0000_0000,
        0b0111_1111,
        0b0000_0001,
        0b0000_0001,
        0b0111_1111,
        0b0100_0000,
        0b0100_0000,
        0b0111_1111,
    ], // 2
    [
        0b0000_0000,
        0b0111_1111,
        0b0000_0001,
        0b0000_0001,
        0b0011_1111,
        0b0000_0001,
        0b0000_0001,
        0b0111_1111,
    ], // 3
    [
        0b0000_0000,
        0b0100_0001,
        0b0100_0001,
        0b0100_0001,
        0b0111_1111,
        0b0000_0001,
        0b0000_0001,
        0b0000_0001,
    ], // 4
    [
        0b0000_0000,
        0b0111_1111,
        0b0100_0000,
        0b0100_0000,
        0b0111_1110,
        0b0000_0001,
        0b0000_0001,
        0b0111_1110,
    ], // 5
    [
        0b0000_0000,
        0b0111_1111,
        0b0100_0000,
        0b0100_0000,
        0b0111_1111,
        0b0100_0001,
        0b0100_0001,
        0b0111_1111,
    ], // 6
    [
        0b0000_0000,
        0b0111_1111,
        0b0000_0001,
        0b0000_0010,
        0b0000_0100,
        0b0000_1000,
        0b0001_0000,
        0b0001_0000,
    ], // 7
    [
        0b0000_0000,
        0b0011_1110,
        0b0100_0001,
        0b0100_0001,
        0b0011_1110,
        0b0100_0001,
        0b0100_0001,
        0b0011_1110,
    ], // 8
    [
        0b0000_0000,
        0b0111_1111,
        0b0100_0001,
        0b0100_0001,
        0b0111_1111,
        0b0000_0001,
        0b0000_0001,
        0b0111_1111,
    ], // 9
    [
        0b0000_0000,
        0b0000_1000,
        0b0010_0010,
        0b0100_0001,
        0b0111_1111,
        0b0100_0001,
        0b0100_0001,
        0b0100_0001,
    ], // A
    [
        0b0000_0000,
        0b0111_1110,
        0b0100_0001,
        0b0100_0001,
        0b0111_1110,
        0b0100_0001,
        0b0100_0001,
        0b0111_1110,
    ], // B
    [
        0b0000_0000,
        0b0011_1110,
        0b0100_0001,
        0b0100_0000,
        0b0100_0000,
        0b0100_0000,
        0b0100_0001,
        0b0011_1110,
    ], // C
    [
        0b0000_0000,
        0b0111_1110,
        0b0100_0001,
        0b0100_0001,
        0b0100_0001,
        0b0100_0001,
        0b0100_0001,
        0b0111_1110,
    ], // D
    [
        0b0000_0000,
        0b0111_1111,
        0b0100_0000,
        0b0100_0000,
        0b0111_1111,
        0b0100_0000,
        0b0100_0000,
        0b0111_1111,
    ], // E
    [
        0b0000_0000,
        0b0111_1111,
        0b0100_0000,
        0b0100_0000,
        0b0111_1111,
        0b0100_0000,
        0b0100_0000,
        0b0100_0000,
    ], // F
];

/// Gambatte only compares the top five bits of each color channel.
const CHANNEL_MASK: u8 = 0xF8;

/// The colors other than grays that Gambatte's CGB reference screenshots
/// show, each as the CGB color Nubo draws (red, green and blue, five bits
/// each) and the color of the same pixels in the screenshots.
///
/// Gambatte mixes the channels of CGB colors to look like a real CGB screen,
/// with a formula of its own. Gambatte is GPL-2.0-only, so Nubo doesn't copy
/// that formula; these pairs were learned instead by comparing Nubo's screen
/// with the reference screenshots of the tests that pass. They are the only
/// colors other than grays that any of Gambatte's CGB tests draws in Nubo.
/// The screenshots hold one more, (33, 146, 108), in the three
/// `scx_during_m3_spx2` tests that fail because Nubo draws black there; it
/// isn't in any palette the test writes, so its CGB color can't be learned
/// until Nubo draws it.
const LEARNED_COLORS: [([u8; 3], [u8; 3]); 2] = [
    ([21, 10, 21], [157, 102, 157]),
    ([10, 21, 10], [91, 146, 91]),
];

/// What a CGB color the screenshots haven't taught (see `LEARNED_COLORS`)
/// turns into: a magenta no reference screenshot uses, so it never matches.
/// A test that draws one fails, and its saved screen shows where.
const UNKNOWN_COLOR: [u8; 3] = [0xFF, 0x00, 0xFF];

/// The color Gambatte's reference screenshots show for a CGB color with
/// these 5-bit channels. A gray keeps its level, spread over eight bits like
/// every gray in the screenshots.
fn reference_color(color: [u8; 3]) -> [u8; 3] {
    let [red, green, blue] = color;
    if red == green && green == blue {
        return [red << 3; 3];
    }
    LEARNED_COLORS
        .iter()
        .find(|(cgb_color, _)| *cgb_color == color)
        .map_or(UNKNOWN_COLOR, |(_, reference)| *reference)
}

/// Converts an RGBA frame from the emulator into the colors of Gambatte's
/// reference screenshots. DMG shades are evenly spaced grays; CGB colors are
/// looked up with `reference_color`.
pub(crate) fn normalize(frame: &[u8], model: Model) -> RgbImage {
    let mut image = screen::normalize(frame, model);
    if model.is_cgb() {
        for pixel in image.pixels_mut() {
            pixel.0 = reference_color(pixel.0.map(|channel| channel >> 3));
        }
    }
    image
}

/// Whether the top-left corner of the screen shows `number`, one digit per
/// tile, in black on white.
pub(crate) fn shows_number(screen: &RgbImage, number: &str) -> bool {
    number.chars().enumerate().all(|(position, digit)| {
        let Some(tile) = digit
            .to_digit(16)
            .and_then(|value| DIGIT_TILES.get(value as usize))
        else {
            return false;
        };
        (0..8u32).all(|y| {
            (0..8u32).all(|x| {
                let is_black = tile[y as usize] & (0x80 >> x) != 0;
                let expected = if is_black { 0 } else { CHANNEL_MASK };
                screen
                    .get_pixel(position as u32 * 8 + x, y)
                    .0
                    .iter()
                    .all(|channel| channel & CHANNEL_MASK == expected)
            })
        })
    })
}

/// Reads up to `max_digits` digits of the number shown in the top-left
/// corner, for failure messages. Stops at the first tile that isn't a digit.
pub(crate) fn shown_number(screen: &RgbImage, max_digits: usize) -> String {
    let mut number = String::new();
    while number.len() < max_digits {
        let Some(digit) = "0123456789ABCDEF".chars().find(|digit| {
            let mut candidate = number.clone();
            candidate.push(*digit);
            shows_number(screen, &candidate)
        }) else {
            break;
        };
        number.push(digit);
    }
    number
}

/// Whether the audio output is silent. Gambatte checks that its raw samples
/// never change. Nubo's output passes through a high-pass filter, so a
/// constant level slowly drifts toward zero instead; that drift only ever
/// moves one way, while any sound makes the output go back and forth.
pub(crate) fn is_silent(samples: &[f32]) -> bool {
    const TOLERANCE: f32 = 1e-6;
    let never_rises = samples
        .windows(2)
        .all(|pair| pair[1] <= pair[0] + TOLERANCE);
    let never_falls = samples
        .windows(2)
        .all(|pair| pair[1] >= pair[0] - TOLERANCE);
    never_rises || never_falls
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A white screen with `number` drawn in the top-left corner.
    fn screen_showing(number: &str) -> RgbImage {
        let mut screen = RgbImage::from_pixel(160, 144, image::Rgb([0xFF; 3]));
        for (position, digit) in number.chars().enumerate() {
            let tile = DIGIT_TILES[digit.to_digit(16).unwrap() as usize];
            for y in 0..8u32 {
                for x in 0..8u32 {
                    if tile[y as usize] & (0x80 >> x) != 0 {
                        screen.put_pixel(position as u32 * 8 + x, y, image::Rgb([0; 3]));
                    }
                }
            }
        }
        screen
    }

    #[test]
    fn maps_grays_and_learned_colors_and_marks_the_rest() {
        assert_eq!(reference_color([0, 0, 0]), [0, 0, 0]);
        assert_eq!(reference_color([20, 20, 20]), [160, 160, 160]);
        assert_eq!(reference_color([31, 31, 31]), [248, 248, 248]);
        assert_eq!(reference_color([21, 10, 21]), [157, 102, 157]);
        assert_eq!(reference_color([11, 18, 11]), UNKNOWN_COLOR);
    }

    #[test]
    fn reads_the_number_on_screen() {
        let screen = screen_showing("3F");
        assert!(shows_number(&screen, "3F"));
        assert!(!shows_number(&screen, "3E"));
        assert_eq!(shown_number(&screen, 4), "3F");
        assert_eq!(shown_number(&screen, 1), "3");
    }

    #[test]
    fn treats_a_drifting_level_as_silence() {
        let drifting: Vec<f32> = (0..100).map(|i| 0.25 * 0.99_f32.powi(i)).collect();
        assert!(is_silent(&drifting));
        let square: Vec<f32> = (0..100)
            .map(|i| if i % 10 < 5 { 0.1 } else { -0.1 })
            .collect();
        assert!(!is_silent(&square));
    }
}
