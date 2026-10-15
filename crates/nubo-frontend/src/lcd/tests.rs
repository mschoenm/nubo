//! Tests for making pictures look like the Game Boy's own screen.

use super::*;

/// An RGBA pixel of the five-bit levels `red`, `green` and `blue`, scaled
/// to eight bits the way the emulator does.
fn pixel(red: u8, green: u8, blue: u8) -> [u8; 4] {
    let scale = |level: u8| (u16::from(level) * 255 / 31) as u8;
    [scale(red), scale(green), scale(blue), 0xFF]
}

#[test]
fn every_eight_bit_level_finds_its_five_bit_one() {
    for level in 0..32 {
        assert_eq!(five_bits(pixel(level, 0, 0)[0]), level);
    }
}

#[test]
fn white_and_black_stay_gray_with_a_little_less_contrast() {
    let mut pixels = [pixel(31, 31, 31), pixel(0, 0, 0)].concat();
    correct_colors(&mut pixels);

    assert_eq!(&pixels[..4], &[245, 245, 245, 0xFF]);
    assert_eq!(&pixels[4..], &[10, 10, 10, 0xFF]);
}

#[test]
fn bright_colors_come_out_less_saturated() {
    let mut pixels = pixel(31, 0, 0).to_vec();
    correct_colors(&mut pixels);

    assert!(
        pixels[0] < 245 && pixels[1] > 0 && pixels[2] > 0,
        "{pixels:?}"
    );
}

#[test]
fn blending_mixes_two_frames_half_and_half() {
    let mut pixels = vec![0, 100, 255, 255];
    blend(&mut pixels, &[255, 101, 255, 255]);

    assert_eq!(pixels, [128, 101, 255, 255]);
}

#[test]
fn frames_of_different_sizes_are_not_blended() {
    let mut pixels = vec![0; 8];
    blend(&mut pixels, &[255; 4]);

    assert_eq!(pixels, [0; 8]);
}
