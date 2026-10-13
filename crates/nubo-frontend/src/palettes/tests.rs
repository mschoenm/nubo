//! Tests for repainting DMG frames.

use super::*;

#[test]
fn grays_become_the_palette_colors() {
    let mut pixels = vec![0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0xFF];

    DmgPalette::Green.recolor(&mut pixels);

    assert_eq!(pixels, [0x9B, 0xBC, 0x0F, 0xFF, 0x0F, 0x38, 0x0F, 0xFF]);
}

#[test]
fn other_colors_are_left_alone() {
    let mut pixels = vec![0x12, 0x34, 0x56, 0xFF];

    DmgPalette::Pocket.recolor(&mut pixels);

    assert_eq!(pixels, [0x12, 0x34, 0x56, 0xFF]);
}

#[test]
fn gray_changes_nothing() {
    let mut pixels = vec![0xD3, 0xD3, 0xD3, 0xFF];

    DmgPalette::Gray.recolor(&mut pixels);

    assert_eq!(pixels, [0xD3, 0xD3, 0xD3, 0xFF]);
}
