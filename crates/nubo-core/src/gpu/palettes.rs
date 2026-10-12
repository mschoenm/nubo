//! Palette registers and the conversion from 2-bit color IDs to RGBA colors.
//!
//! Tiles store a 2-bit color ID (0 to 3) per pixel. On DMG the palette registers BGP (0xFF47),
//! OBP0 (0xFF48) and OBP1 (0xFF49) map each color ID to one of four gray shades. On CGB, palette
//! RAM holds eight background and eight object palettes of four RGB555 colors each (five bits
//! per channel). The CPU reaches palette RAM through an index register (BCPS/OCPS) and a data
//! register (BCPD/OCPD).

use crate::serializable::Serializable;
use crate::utils::is_bit_set;
use std::io::{Read, Write};

/// An RGBA color as stored in the frame buffer.
pub(super) type Color = [u8; 4];

pub(super) const BLACK: Color = [0x0, 0x0, 0x0, 0xFF];
pub(super) const DARK_GRAY: Color = [0xA9, 0xA9, 0xA9, 0xFF];
pub(super) const LIGHT_GRAY: Color = [0xD3, 0xD3, 0xD3, 0xFF];
pub(super) const WHITE: Color = [0xFF, 0xFF, 0xFF, 0xFF];

const COLORS_PER_PALETTE: usize = 4;
const CGB_PALETTE_COUNT: usize = 8;
const BYTES_PER_RGB555_COLOR: usize = 2;
const CGB_COLOR_COUNT: usize = COLORS_PER_PALETTE * CGB_PALETTE_COUNT;
const CGB_PALETTE_RAM_SIZE: usize = CGB_COLOR_COUNT * BYTES_PER_RGB555_COLOR;
const PALETTE_RAM_ADDRESS_MASK: u8 = 0b0011_1111;
const AUTO_INCREMENT_BIT: u8 = 7;

/// The four RGBA colors the emulator draws DMG frames in, lightest first: the shades 0 (white) to
/// 3 (black) that the palette registers BGP, OBP0 and OBP1 pick for each color ID. Frontends that
/// show DMG games in other colors, or compare frames with screenshots, look for these exact values.
pub const DMG_SHADES: [Color; 4] = [WHITE, LIGHT_GRAY, DARK_GRAY, BLACK];

/// The DMG palette registers and CGB palette RAM.
#[derive(Clone, Debug)]
pub(crate) struct Palettes {
    /// BGP (0xFF47): the DMG background and window palette, two bits per color ID.
    bgp: u8,
    /// OBP0 (0xFF48): DMG object palette 0. Color ID 0 is transparent, so bits 0-1 aren't used.
    obp0: u8,
    /// OBP1 (0xFF49): DMG object palette 1.
    obp1: u8,
    bg_palette_ram: [u8; CGB_PALETTE_RAM_SIZE],
    object_palette_ram: [u8; CGB_PALETTE_RAM_SIZE],
    /// BCPS (0xFF68): bits 0-5 select a byte of background palette RAM, bit 7 enables
    /// auto-increment after each BCPD write.
    cgb_bcps: u8,
    /// OCPS (0xFF6A): like BCPS, for object palette RAM.
    cgb_ocps: u8,
    /// Every color of background palette RAM (four per palette) as RGBA, worked out again whenever
    /// palette RAM changes so the PPU doesn't have to convert each pixel. Not part of save states.
    bg_colors: [Color; CGB_COLOR_COUNT],
    /// Like `bg_colors`, for object palette RAM.
    object_colors: [Color; CGB_COLOR_COUNT],
}

/// Maps a color ID through a DMG palette register (BGP) to a shade from 0 (white) to 3 (black).
fn bg_shade(palette: u8, color_id: u8) -> u8 {
    (palette >> ((color_id & 0b11) * 2)) & 0b11
}

/// Maps a color ID through a DMG object palette register (OBP0/OBP1) to a shade. Color ID 0 is
/// transparent for objects, so it has no shade.
fn object_shade(palette: u8, color_id: u8) -> Option<u8> {
    (color_id != 0).then(|| bg_shade(palette, color_id))
}

/// Reads the little-endian RGB555 color for a palette number and color ID from CGB palette RAM.
#[cfg(test)]
fn read_rgb555(palette_ram: &[u8; CGB_PALETTE_RAM_SIZE], palette_number: u8, color_id: u8) -> u16 {
    color_rgb555(
        palette_ram,
        usize::from(palette_number) * COLORS_PER_PALETTE + usize::from(color_id),
    )
}

/// Reads color number `color` (palette number * 4 + color ID) from CGB palette RAM.
fn color_rgb555(palette_ram: &[u8; CGB_PALETTE_RAM_SIZE], color: usize) -> u16 {
    let index = color * BYTES_PER_RGB555_COLOR;
    u16::from(palette_ram[index]) | (u16::from(palette_ram[index + 1]) << 8)
}

/// Every color of CGB palette RAM as RGBA.
fn rgba_colors(palette_ram: &[u8; CGB_PALETTE_RAM_SIZE]) -> [Color; CGB_COLOR_COUNT] {
    std::array::from_fn(|color| rgb555_to_rgba(color_rgb555(palette_ram, color)))
}

/// Writes a byte of CGB palette RAM through a BCPD/OCPD data register, at the index selected by
/// the matching BCPS/OCPS register, updates the RGBA color it belongs to, and advances the index
/// when auto-increment is enabled.
fn write_palette_ram(
    palette_ram: &mut [u8; CGB_PALETTE_RAM_SIZE],
    colors: &mut [Color; CGB_COLOR_COUNT],
    index_register: &mut u8,
    value: u8,
) {
    let index = usize::from(*index_register & PALETTE_RAM_ADDRESS_MASK);
    palette_ram[index] = value;
    let color = index / BYTES_PER_RGB555_COLOR;
    colors[color] = rgb555_to_rgba(color_rgb555(palette_ram, color));
    if is_bit_set(*index_register, AUTO_INCREMENT_BIT) {
        *index_register = index_register.wrapping_add(1) | 0x80;
    }
}

/// Scales a CGB RGB555 color (five bits per channel) to an RGBA color with eight bits per channel.
fn rgb555_to_rgba(rgb555: u16) -> Color {
    let channel = |shift: u16| CHANNEL_LEVELS[usize::from((rgb555 >> shift) & 0b11111)];
    [channel(0), channel(5), channel(10), 0xFF]
}

/// Each 5-bit color channel value scaled to 8 bits.
const CHANNEL_LEVELS: [u8; 32] = {
    let mut levels = [0; 32];
    let mut value = 0;
    while value < 32 {
        levels[value] = (value * 0xFF / 31) as u8;
        value += 1;
    }
    levels
};

impl Default for Palettes {
    fn default() -> Self {
        Palettes {
            bgp: 0,
            obp0: 0,
            obp1: 0,
            bg_palette_ram: [0; CGB_PALETTE_RAM_SIZE],
            object_palette_ram: [0; CGB_PALETTE_RAM_SIZE],
            cgb_bcps: 0,
            cgb_ocps: 0,
            bg_colors: [BLACK; CGB_COLOR_COUNT],
            object_colors: [BLACK; CGB_COLOR_COUNT],
        }
    }
}

impl Serializable for Palettes {
    fn serialize(&self, writer: &mut dyn Write) -> std::io::Result<()> {
        self.bgp.serialize(writer)?;
        self.obp0.serialize(writer)?;
        self.obp1.serialize(writer)?;
        self.bg_palette_ram.serialize(writer)?;
        self.object_palette_ram.serialize(writer)?;
        self.cgb_bcps.serialize(writer)?;
        self.cgb_ocps.serialize(writer)
    }

    fn deserialize(&mut self, reader: &mut dyn Read) -> std::io::Result<()> {
        self.bgp.deserialize(reader)?;
        self.obp0.deserialize(reader)?;
        self.obp1.deserialize(reader)?;
        self.bg_palette_ram.deserialize(reader)?;
        self.object_palette_ram.deserialize(reader)?;
        self.cgb_bcps.deserialize(reader)?;
        self.cgb_ocps.deserialize(reader)?;
        self.bg_colors = rgba_colors(&self.bg_palette_ram);
        self.object_colors = rgba_colors(&self.object_palette_ram);
        Ok(())
    }
}

impl Palettes {
    pub(crate) fn bgp(&self) -> u8 {
        self.bgp
    }

    pub(crate) fn set_bgp(&mut self, value: u8) {
        self.bgp = value;
    }

    pub(crate) fn obp0(&self) -> u8 {
        self.obp0
    }

    pub(crate) fn set_obp0(&mut self, value: u8) {
        self.obp0 = value;
    }

    pub(crate) fn obp1(&self) -> u8 {
        self.obp1
    }

    pub(crate) fn set_obp1(&mut self, value: u8) {
        self.obp1 = value;
    }

    pub(crate) fn cgb_bcps(&self) -> u8 {
        self.cgb_bcps
    }

    pub(crate) fn set_cgb_bcps(&mut self, value: u8) {
        self.cgb_bcps = value;
    }

    pub(crate) fn cgb_ocps(&self) -> u8 {
        self.cgb_ocps
    }

    pub(crate) fn set_cgb_ocps(&mut self, value: u8) {
        self.cgb_ocps = value;
    }

    /// Returns OBP0 for DMG object palette 0 and OBP1 otherwise.
    fn dmg_object_palette(&self, palette_number: u8) -> u8 {
        if palette_number == 0 {
            self.obp0
        } else {
            self.obp1
        }
    }

    fn dmg_bg_shade(&self, color_id: u8) -> u8 {
        bg_shade(self.bgp, color_id)
    }

    fn dmg_object_shade(&self, palette_number: u8, color_id: u8) -> Option<u8> {
        object_shade(self.dmg_object_palette(palette_number), color_id)
    }

    /// The gray shade of a background or window pixel on DMG.
    pub(super) fn dmg_bg_color(&self, color_id: u8) -> Color {
        DMG_SHADES[self.dmg_bg_shade(color_id) as usize]
    }

    /// The gray shade of an object pixel on DMG, or `None` when the pixel is transparent.
    pub(super) fn dmg_object_color(&self, palette_number: u8, color_id: u8) -> Option<Color> {
        self.dmg_object_shade(palette_number, color_id)
            .map(|shade| DMG_SHADES[shade as usize])
    }

    #[cfg(test)]
    fn cgb_bg_rgb555(&self, palette_number: u8, color_id: u8) -> u16 {
        read_rgb555(&self.bg_palette_ram, palette_number, color_id)
    }

    #[cfg(test)]
    fn cgb_object_rgb555(&self, palette_number: u8, color_id: u8) -> u16 {
        read_rgb555(&self.object_palette_ram, palette_number, color_id)
    }

    /// The color of a background or window pixel on CGB hardware. In DMG compatibility mode (a
    /// DMG game running on CGB) the color ID first goes through BGP.
    pub(super) fn cgb_bg_color(
        &self,
        palette_number: u8,
        color_id: u8,
        dmg_compatible: bool,
    ) -> Color {
        let color_index = if dmg_compatible {
            self.dmg_bg_shade(color_id)
        } else {
            color_id
        };
        self.bg_colors[usize::from(palette_number) * COLORS_PER_PALETTE + usize::from(color_index)]
    }

    /// The color of an object pixel on CGB hardware, or `None` when the pixel is transparent. In
    /// DMG compatibility mode the color ID first goes through OBP0 or OBP1.
    pub(super) fn cgb_object_color(
        &self,
        palette_number: u8,
        color_id: u8,
        dmg_compatible: bool,
    ) -> Option<Color> {
        if color_id == 0 {
            return None;
        }
        let color_index = if dmg_compatible {
            self.dmg_object_shade(palette_number, color_id)?
        } else {
            color_id
        };
        Some(
            self.object_colors
                [usize::from(palette_number) * COLORS_PER_PALETTE + usize::from(color_index)],
        )
    }

    /// Reads BCPD (0xFF69): the background palette RAM byte selected by BCPS.
    pub(crate) fn cgb_bcpd(&self) -> u8 {
        self.bg_palette_ram[usize::from(self.cgb_bcps & PALETTE_RAM_ADDRESS_MASK)]
    }

    /// Writes BCPD (0xFF69).
    pub(crate) fn set_cgb_bcpd(&mut self, value: u8) {
        write_palette_ram(
            &mut self.bg_palette_ram,
            &mut self.bg_colors,
            &mut self.cgb_bcps,
            value,
        );
    }

    #[cfg(test)]
    pub(super) fn set_bg_palette_ram_byte(&mut self, index: usize, value: u8) {
        self.bg_palette_ram[index] = value;
        self.bg_colors = rgba_colors(&self.bg_palette_ram);
    }

    /// A BCPD write while the PPU blocks palette RAM: the byte is lost but the index still
    /// increments.
    pub(crate) fn skip_cgb_bcpd_write(&mut self) {
        if is_bit_set(self.cgb_bcps, AUTO_INCREMENT_BIT) {
            self.cgb_bcps = self.cgb_bcps.wrapping_add(1) | 0x80;
        }
    }

    /// An OCPD write while the PPU blocks palette RAM.
    pub(crate) fn skip_cgb_ocpd_write(&mut self) {
        if is_bit_set(self.cgb_ocps, AUTO_INCREMENT_BIT) {
            self.cgb_ocps = self.cgb_ocps.wrapping_add(1) | 0x80;
        }
    }

    /// Reads OCPD (0xFF6B): the object palette RAM byte selected by OCPS.
    pub(crate) fn cgb_ocpd(&self) -> u8 {
        self.object_palette_ram[usize::from(self.cgb_ocps & PALETTE_RAM_ADDRESS_MASK)]
    }

    /// Writes OCPD (0xFF6B).
    pub(crate) fn set_cgb_ocpd(&mut self, value: u8) {
        write_palette_ram(
            &mut self.object_palette_ram,
            &mut self.object_colors,
            &mut self.cgb_ocps,
            value,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color_id(bit_index: u8, high_byte: u8, low_byte: u8) -> u8 {
        let bit = 7 - bit_index;
        (((high_byte >> bit) & 1) << 1) | ((low_byte >> bit) & 1)
    }

    fn fill_test_palette_ram(palette_ram: &mut [u8; CGB_PALETTE_RAM_SIZE]) {
        let values = (0x11..=0xFF).step_by(0x11);
        for (color, value) in palette_ram.as_chunks_mut::<2>().0.iter_mut().zip(values) {
            color.fill(value);
        }
    }

    fn palettes_with_test_bg_palettes() -> Palettes {
        let mut palettes = Palettes::default();
        fill_test_palette_ram(&mut palettes.bg_palette_ram);
        palettes.bg_colors = rgba_colors(&palettes.bg_palette_ram);
        palettes
    }

    fn palettes_with_test_object_palettes() -> Palettes {
        let mut palettes = Palettes::default();
        fill_test_palette_ram(&mut palettes.object_palette_ram);
        palettes.object_colors = rgba_colors(&palettes.object_palette_ram);
        palettes
    }

    #[test]
    fn should_lookup_background_palette() {
        let palettes = palettes_with_test_bg_palettes();
        assert_eq!(palettes.cgb_bg_rgb555(3, 1), 0xEEEE);
    }

    #[test]
    fn should_lookup_object_palette() {
        let palettes = palettes_with_test_object_palettes();
        assert_eq!(palettes.cgb_object_rgb555(2, 2), 0xBBBB);
    }

    #[test]
    fn should_lookup_background_palette_byte_via_bcpd() {
        let mut palettes = palettes_with_test_bg_palettes();
        palettes.bg_palette_ram[2] = 0x22;
        palettes.bg_palette_ram[3] = 0xBB;

        palettes.set_cgb_bcps(0b0000_0011);

        assert_eq!(palettes.cgb_bcpd(), 0xBB);
    }

    #[test]
    fn should_lookup_object_palette_byte_via_ocpd() {
        let mut palettes = palettes_with_test_object_palettes();
        palettes.object_palette_ram[2] = 0x22;
        palettes.object_palette_ram[3] = 0xBB;

        palettes.set_cgb_ocps(0b0000_0011);

        assert_eq!(palettes.cgb_ocpd(), 0xBB);
    }

    #[test]
    fn should_write_palette_byte_via_bcpd() {
        let mut palettes = palettes_with_test_bg_palettes();

        palettes.set_cgb_bcps(0b0000_0011);
        palettes.set_cgb_bcpd(0xBB);

        assert_eq!(palettes.bg_palette_ram[3], 0xBB);
    }

    #[test]
    fn should_auto_increment_bcps_address_when_writing_palette_byte() {
        let mut palettes = palettes_with_test_bg_palettes();

        palettes.set_cgb_bcps(0b1000_0011);
        palettes.set_cgb_bcpd(0xBB);

        assert_eq!(palettes.cgb_bcps, 0b1000_0100);
    }

    #[test]
    fn should_auto_increment_ocps_address_when_writing_palette_byte() {
        let mut palettes = palettes_with_test_object_palettes();

        palettes.set_cgb_ocps(0b1000_0011);
        palettes.set_cgb_ocpd(0xBB);

        assert_eq!(palettes.cgb_ocps, 0b1000_0100);
    }

    #[test]
    fn should_wrap_palette_address_when_auto_incrementing_past_the_last_byte() {
        let mut palettes = Palettes::default();

        palettes.set_cgb_bcps(0b1011_1111);
        palettes.set_cgb_bcpd(0xBB);

        assert_eq!(palettes.bg_palette_ram[0x3F], 0xBB);
        assert_eq!(palettes.cgb_bcps, 0b1100_0000);
    }

    #[test]
    fn should_not_auto_increment_bcps_address_when_writing_palette_byte_if_auto_incrementing_is_disabled()
     {
        let mut palettes = palettes_with_test_bg_palettes();

        palettes.set_cgb_bcps(0b0000_0011);
        palettes.set_cgb_bcpd(0xBB);

        assert_eq!(palettes.cgb_bcps, 0b0000_0011);
    }

    #[test]
    fn should_not_auto_increment_ocps_address_when_writing_palette_byte_if_auto_incrementing_is_disabled()
     {
        let mut palettes = palettes_with_test_object_palettes();

        palettes.set_cgb_ocps(0b0000_0011);
        palettes.set_cgb_ocpd(0xBB);

        assert_eq!(palettes.cgb_ocps, 0b0000_0011);
    }

    #[test]
    fn should_calculate_background_color() {
        let palettes = palettes_with_test_bg_palettes();

        let color_id = color_id(0, 0b0010_1010, 0b1101_0101);
        let color = palettes.cgb_bg_color(3, color_id, false);

        assert_eq!(color, [0x73, 0xBD, 0xDE, 0xFF]);
    }

    #[test]
    fn should_calculate_object_color() {
        let palettes = palettes_with_test_object_palettes();

        let color_id = color_id(0, 0b0010_1010, 0b1101_0101);
        let color = palettes.cgb_object_color(3, color_id, false);

        assert_eq!(color, Some([0x73, 0xBD, 0xDE, 0xFF]));
    }

    #[test]
    fn should_return_none_when_calculating_object_color_with_color_id_0() {
        let palettes = palettes_with_test_object_palettes();

        let color_id = color_id(0, 0b0010_1010, 0b0101_0101);
        let color = palettes.cgb_object_color(3, color_id, false);

        assert_eq!(color, None);
    }

    #[test]
    fn should_map_dmg_color_ids_through_the_palette_registers() {
        let mut palettes = Palettes::default();
        palettes.set_bgp(0b1110_0100);
        palettes.set_obp1(0b0001_1011);

        assert_eq!(palettes.dmg_bg_color(0), WHITE);
        assert_eq!(palettes.dmg_bg_color(3), BLACK);
        assert_eq!(palettes.dmg_object_color(1, 0), None);
        assert_eq!(palettes.dmg_object_color(1, 1), Some(DARK_GRAY));
        assert_eq!(palettes.dmg_object_color(1, 3), Some(WHITE));
    }
}
