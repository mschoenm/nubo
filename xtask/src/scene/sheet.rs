//! The contact sheet of `cargo xtask scene explore`: every candidate's
//! picture in a grid, twice its size, each under its number, so a person or
//! a session can see at a glance which are worth keeping.

use image::{Rgba, RgbaImage};

/// How many pictures each row holds.
const COLUMNS: u32 = 4;
/// How many times their size the pictures are shown.
const SCALE: u32 = 2;
/// The space around and between the pictures, in pixels.
const GAP: u32 = 12;
/// How many pixels each dot of a number's digits takes.
const DIGIT_SCALE: u32 = 3;
/// How high the strip above each picture is, where its number goes.
const LABEL_HEIGHT: u32 = 5 * DIGIT_SCALE + 8;
const BACKGROUND: Rgba<u8> = Rgba([13, 15, 20, 255]);
const INK: Rgba<u8> = Rgba([235, 235, 240, 255]);

/// The digits 0 to 9, three dots wide and five high, each row's dots as the
/// three lowest bits, the leftmost highest.
const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

/// The contact sheet of `pictures`, each with the number it comes with.
pub fn draw(pictures: &[(u32, RgbaImage)]) -> RgbaImage {
    let cell_width = pictures
        .iter()
        .map(|(_, picture)| picture.width())
        .max()
        .unwrap_or(0)
        * SCALE;
    let cell_height = pictures
        .iter()
        .map(|(_, picture)| picture.height())
        .max()
        .unwrap_or(0)
        * SCALE
        + LABEL_HEIGHT;
    let count = u32::try_from(pictures.len()).unwrap_or(u32::MAX);
    let columns = count.clamp(1, COLUMNS);
    let rows = count.div_ceil(columns).max(1);
    let width = GAP + columns * (cell_width + GAP);
    let height = GAP + rows * (cell_height + GAP);

    let mut sheet = RgbaImage::from_pixel(width, height, BACKGROUND);
    for (index, (number, picture)) in (0..).zip(pictures) {
        let x = GAP + (index % columns) * (cell_width + GAP);
        let y = GAP + (index / columns) * (cell_height + GAP);
        draw_number(&mut sheet, *number, x, y);
        let scaled = image::imageops::resize(
            picture,
            picture.width() * SCALE,
            picture.height() * SCALE,
            image::imageops::FilterType::Nearest,
        );
        image::imageops::overlay(
            &mut sheet,
            &scaled,
            i64::from(x),
            i64::from(y + LABEL_HEIGHT),
        );
    }
    sheet
}

/// Writes `number` with its top-left corner at `x`, `y`.
fn draw_number(sheet: &mut RgbaImage, number: u32, x: u32, y: u32) {
    for (place, digit) in (0..).zip(number.to_string().bytes()) {
        let rows = DIGITS[usize::from(digit - b'0')];
        let left = x + place * 4 * DIGIT_SCALE;
        for (row, bits) in (0..).zip(rows) {
            for column in 0..3 {
                if bits & (0b100 >> column) != 0 {
                    for dy in 0..DIGIT_SCALE {
                        for dx in 0..DIGIT_SCALE {
                            let (px, py) =
                                (left + column * DIGIT_SCALE + dx, y + row * DIGIT_SCALE + dy);
                            if px < sheet.width() && py < sheet.height() {
                                sheet.put_pixel(px, py, INK);
                            }
                        }
                    }
                }
            }
        }
    }
}
