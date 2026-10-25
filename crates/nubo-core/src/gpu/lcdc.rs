//! Helpers that read the individual bits of LCDC, the LCD control register at 0xFF40.
//!
//! Each function takes the raw register value and answers one question about it, so callers can
//! pass either the current value or an older one (the renderer sometimes needs the previous dot's
//! LCDC to reproduce hardware timing).

use crate::utils::is_bit_set;

const BG_AND_WINDOW_ENABLED_BIT: u8 = 0;
const OBJECTS_ENABLED_BIT: u8 = 1;
const TALL_OBJECTS_BIT: u8 = 2;
const BG_TILE_MAP_BIT: u8 = 3;
const TILE_DATA_ADDRESSING_BIT: u8 = 4;
const WINDOW_ENABLED_BIT: u8 = 5;
const WINDOW_TILE_MAP_BIT: u8 = 6;
const LCD_ENABLED_BIT: u8 = 7;

/// Bit 0. On DMG, and on a CGB running a DMG game, clearing it blanks the background and window.
/// In CGB mode it instead removes the background's priority over objects.
pub(super) fn bg_and_window_enabled(lcdc: u8) -> bool {
    is_bit_set(lcdc, BG_AND_WINDOW_ENABLED_BIT)
}

/// Bit 1: whether objects (sprites) are drawn.
pub(super) fn objects_enabled(lcdc: u8) -> bool {
    is_bit_set(lcdc, OBJECTS_ENABLED_BIT)
}

/// Bit 2: whether objects are 8x16 pixels instead of 8x8.
pub(super) fn uses_tall_objects(lcdc: u8) -> bool {
    is_bit_set(lcdc, TALL_OBJECTS_BIT)
}

/// Bit 3: whether the background uses the tile map at 0x9C00 instead of 0x9800.
pub(super) fn bg_uses_high_tile_map(lcdc: u8) -> bool {
    is_bit_set(lcdc, BG_TILE_MAP_BIT)
}

/// Bit 4: whether background and window tiles use Pan Docs' "$8000 method" (unsigned tile
/// numbers from 0x8000) instead of the "$8800 method" (signed tile numbers around 0x9000).
pub(super) fn uses_8000_addressing(lcdc: u8) -> bool {
    is_bit_set(lcdc, TILE_DATA_ADDRESSING_BIT)
}

/// Bit 5: whether the window layer is drawn.
pub(super) fn window_enabled(lcdc: u8) -> bool {
    is_bit_set(lcdc, WINDOW_ENABLED_BIT)
}

/// Bit 6: whether the window uses the tile map at 0x9C00 instead of 0x9800.
pub(super) fn window_uses_high_tile_map(lcdc: u8) -> bool {
    is_bit_set(lcdc, WINDOW_TILE_MAP_BIT)
}

/// Bit 7: whether the LCD and the PPU are switched on.
pub(super) fn lcd_enabled(lcdc: u8) -> bool {
    is_bit_set(lcdc, LCD_ENABLED_BIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_read_each_lcdc_bit() {
        assert!(bg_and_window_enabled(0x01));
        assert!(objects_enabled(0x02));
        assert!(uses_tall_objects(0x04));
        assert!(bg_uses_high_tile_map(0x08));
        assert!(uses_8000_addressing(0x10));
        assert!(window_enabled(0x20));
        assert!(window_uses_high_tile_map(0x40));
        assert!(lcd_enabled(0x80));
    }

    #[test]
    fn should_ignore_other_lcdc_bits() {
        assert!(!bg_and_window_enabled(0xFE));
        assert!(!objects_enabled(0xFD));
        assert!(!uses_tall_objects(0xFB));
        assert!(!bg_uses_high_tile_map(0xF7));
        assert!(!uses_8000_addressing(0xEF));
        assert!(!window_enabled(0xDF));
        assert!(!window_uses_high_tile_map(0xBF));
        assert!(!lcd_enabled(0x7F));
    }
}
