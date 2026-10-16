//! Unit tests for the mode 3 renderer: each test draws one line and checks its pixels.

use super::*;
use crate::address_bus::hdma::HdmaState;
use crate::gpu::palettes::{BLACK, Color, DARK_GRAY, LIGHT_GRAY, Palettes, WHITE};
use crate::gpu::{DOTS_PER_LINE, Gpu, LineState};
use crate::utils::set_bit;

const BLACK_TILE: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];
const SAMPLE_TILE_A: [u8; 16] = [
    0x3C, 0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x5E, 0x7E, 0x0A, 0x7C, 0x56, 0x38, 0x7C,
];
const SAMPLE_TILE_B: [u8; 16] = [
    0xFF, 0x0, 0x7E, 0xFF, 0x85, 0x81, 0x89, 0x83, 0x93, 0x85, 0xA5, 0x8B, 0xC9, 0x97, 0x7E, 0xFF,
];
const WINDOW_TILE: [u8; 16] = [
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
];

const RED: Color = [0xFF, 0x0, 0x0, 0xFF];
const BLUE: Color = [0x0, 0x0, 0xFF, 0xFF];

/// An OAM entry (object) in a readable form, written to OAM by `write_sprite_to_oam`.
#[derive(Debug, Clone, Copy, Default)]
struct Sprite {
    /// Screen y of the top row (the OAM byte minus 16).
    y_pos: i16,
    /// Screen x of the leftmost column (the OAM byte minus 8).
    x_pos: i16,
    tile_index: u8,
    /// Whether background colors 1-3 are drawn over the object.
    priority: bool,
    y_flip: bool,
    x_flip: bool,
    /// DMG palette: 0 for OBP0, 1 for OBP1.
    dmg_palette: u8,
    /// Byte offset of the entry in OAM.
    oam_index: u16,
    /// Whether the CGB reads the tile from VRAM bank 1.
    cgb_from_bank_one: bool,
    cgb_palette: u8,
}

fn write_tile_to_memory(gpu: &mut Gpu, base_index: u16, index: u16, tile_bytes: [u8; 16]) {
    let offset = index * 16;
    for (tile_byte_index, tile_byte) in tile_bytes.iter().enumerate() {
        gpu.video_ram[(base_index + offset + tile_byte_index as u16) as usize] = *tile_byte;
    }
}

fn write_tile_to_bg_memory(gpu: &mut Gpu, index: u16, tile_bytes: [u8; 16]) {
    write_tile_to_memory(gpu, 0x1000, index, tile_bytes);
}

fn write_tile_to_bg_memory_in_bank_one(gpu: &mut Gpu, index: u16, tile_bytes: [u8; 16]) {
    write_tile_to_memory(gpu, 0x3000, index, tile_bytes);
}

fn write_tile_to_obj_memory(gpu: &mut Gpu, index: u16, tile_bytes: [u8; 16]) {
    write_tile_to_memory(gpu, 0x0000, index, tile_bytes);
}

fn write_tile_attributes(gpu: &mut Gpu, index: u16, attributes: u8) {
    gpu.video_ram[(0x3800 + index) as usize] = attributes;
}

fn write_sprite_to_oam(gpu: &mut Gpu, sprite: Sprite) {
    let oam_index = sprite.oam_index;
    gpu.object_attribute_memory[oam_index as usize] = (sprite.y_pos + 16) as u8;
    gpu.object_attribute_memory[(oam_index + 1) as usize] = (sprite.x_pos + 8) as u8;
    gpu.object_attribute_memory[(oam_index + 2) as usize] = sprite.tile_index;

    let mut attributes = 0;
    if sprite.priority {
        attributes = set_bit(attributes, 7);
    }
    if sprite.y_flip {
        attributes = set_bit(attributes, 6);
    }
    if sprite.x_flip {
        attributes = set_bit(attributes, 5);
    }
    if sprite.dmg_palette == 1 {
        attributes = set_bit(attributes, 4);
    }
    if sprite.cgb_from_bank_one {
        attributes = set_bit(attributes, 3);
    }
    attributes |= sprite.cgb_palette;
    gpu.object_attribute_memory[(oam_index + 3) as usize] = attributes;
}

fn write_window_tile_index_to_memory(gpu: &mut Gpu, position_index: u16, tile_index: u8) {
    gpu.video_ram[(0x1C00 + position_index) as usize] = tile_index;
}

fn initialize_monochrome_palettes(palettes: &mut Palettes) {
    // DMG Palette:
    // Black: color id 0
    // Dark Gray: color id 1
    // Light Gray: color id 2
    // White: color id 3
    palettes.set_bgp(0b0001_1011);
    palettes.set_obp0(0b0001_1011);
}

fn initialize_color_palettes(palettes: &mut Palettes) {
    // Red
    palettes.set_bg_palette_ram_byte(8, 0b0001_1111);
    palettes.set_bg_palette_ram_byte(9, 0);

    // Green
    palettes.set_bg_palette_ram_byte(10, 0b1110_0000);
    palettes.set_bg_palette_ram_byte(11, 0b0000_0011);

    // Blue
    palettes.set_bg_palette_ram_byte(12, 0);
    palettes.set_bg_palette_ram_byte(13, 0b0111_1100);
}

struct FrameBufferAssertion<'a> {
    frame_buffer: &'a [u8],
    coordinates: (u32, u32),
}

impl<'a> FrameBufferAssertion<'a> {
    pub fn new(frame_buffer: &[u8]) -> FrameBufferAssertion<'_> {
        FrameBufferAssertion {
            frame_buffer,
            coordinates: (0, 0),
        }
    }

    pub fn at_starting_coordinates(&mut self, coordinates: (u32, u32)) -> &mut Self {
        self.coordinates = coordinates;
        self
    }

    pub fn has_pixels(&self, color_pixels: &[Color]) {
        let (x, y) = self.coordinates;
        let first_pixel_index = (y * GB_SCREEN_WIDTH + x) as usize;

        for (index, color) in color_pixels.iter().enumerate() {
            let byte_index = (first_pixel_index + index) * 4;
            assert_eq!(&self.frame_buffer[byte_index..byte_index + 4], color);
        }
    }
}

fn assert_that(frame_buffer: &[u8]) -> FrameBufferAssertion<'_> {
    FrameBufferAssertion::new(frame_buffer)
}

#[test]
fn should_render_tile_line() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);

    gpu.ly = 0;
    gpu.lcdc = 0b1000_0011;

    gpu.write_scanline();

    let frame_buffer = gpu.frame_buffer.as_flattened();

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[
            BLACK, LIGHT_GRAY, WHITE, WHITE, WHITE, WHITE, LIGHT_GRAY, BLACK,
        ]);
}

#[test]
fn should_render_multiple_tile_lines() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);

    gpu.lcdc = 0b1000_0011;

    for _ in 0..3 {
        gpu.write_scanline();
        gpu.ly += 1;
    }

    let frame_buffer = gpu.frame_buffer.as_flattened();

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[
            BLACK, LIGHT_GRAY, WHITE, WHITE, WHITE, WHITE, LIGHT_GRAY, BLACK,
        ]);

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 1))
        .has_pixels(&[BLACK, WHITE, BLACK, BLACK, BLACK, BLACK, WHITE, BLACK]);

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 2))
        .has_pixels(&[BLACK, WHITE, BLACK, BLACK, BLACK, BLACK, WHITE, BLACK]);
}

#[test]
fn should_render_multiple_tile_lines_in_color_mode() {
    let mut gpu = Gpu::new();

    gpu.set_model(Model::CgbE);

    initialize_color_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);

    write_tile_attributes(&mut gpu, 0, 0b0000_0001);

    gpu.lcdc = 0b1000_0011;

    for _ in 0..8 {
        gpu.write_scanline();
        gpu.ly += 1;
    }

    let frame_buffer = gpu.frame_buffer.as_flattened();

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[RED, BLUE, BLACK, BLACK, BLACK, BLACK, BLUE, RED]);

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 1))
        .has_pixels(&[RED, BLACK, RED, RED, RED, RED, BLACK, RED]);

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 2))
        .has_pixels(&[RED, BLACK, RED, RED, RED, RED, BLACK, RED]);

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 7))
        .has_pixels(&[RED, BLUE, BLACK, BLACK, BLACK, BLUE, RED, RED]);
}

#[test]
fn should_render_multiple_tile_lines_in_color_mode_from_bank_one() {
    let mut gpu = Gpu::new();

    gpu.set_model(Model::CgbE);

    initialize_color_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory_in_bank_one(&mut gpu, 0, SAMPLE_TILE_A);

    write_tile_attributes(&mut gpu, 0, 0b0000_1001);

    gpu.lcdc = 0b1000_0011;

    for _ in 0..8 {
        gpu.write_scanline();
        gpu.ly += 1;
    }

    let frame_buffer = gpu.frame_buffer.as_flattened();

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[RED, BLUE, BLACK, BLACK, BLACK, BLACK, BLUE, RED]);

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 1))
        .has_pixels(&[RED, BLACK, RED, RED, RED, RED, BLACK, RED]);

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 2))
        .has_pixels(&[RED, BLACK, RED, RED, RED, RED, BLACK, RED]);

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 7))
        .has_pixels(&[RED, BLUE, BLACK, BLACK, BLACK, BLUE, RED, RED]);
}

#[test]
fn should_flip_background_tile_on_y_axis() {
    let mut gpu = Gpu::new();

    gpu.set_model(Model::CgbE);

    initialize_color_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);

    write_tile_attributes(&mut gpu, 0, 0b0100_0001);

    gpu.lcdc = 0b1000_0011;

    for _ in 0..8 {
        gpu.write_scanline();
        gpu.ly += 1;
    }

    let frame_buffer = gpu.frame_buffer.as_flattened();

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[RED, BLUE, BLACK, BLACK, BLACK, BLUE, RED, RED]);

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 5))
        .has_pixels(&[RED, BLACK, RED, RED, RED, RED, BLACK, RED]);

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 6))
        .has_pixels(&[RED, BLACK, RED, RED, RED, RED, BLACK, RED]);

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 7))
        .has_pixels(&[RED, BLUE, BLACK, BLACK, BLACK, BLACK, BLUE, RED]);
}

#[test]
fn should_flip_background_tile_on_x_axis() {
    let mut gpu = Gpu::new();

    gpu.set_model(Model::CgbE);

    initialize_color_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);

    write_tile_attributes(&mut gpu, 0, 0b0010_0001);

    gpu.lcdc = 0b1000_0011;

    for _ in 0..8 {
        gpu.write_scanline();
        gpu.ly += 1;
    }

    let frame_buffer = gpu.frame_buffer.as_flattened();

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 7))
        .has_pixels(&[RED, RED, BLUE, BLACK, BLACK, BLACK, BLUE, RED]);
}

#[test]
fn should_overlay_window_over_multiple_tile_lines() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);
    write_tile_to_bg_memory(&mut gpu, 1, WINDOW_TILE);
    write_window_tile_index_to_memory(&mut gpu, 0, 1);

    gpu.wy = 1;
    gpu.wx = 8;
    gpu.lcdc = 0b1110_0011;

    for _ in 0..3 {
        gpu.write_scanline();
        gpu.ly += 1;
    }

    let frame_buffer = gpu.frame_buffer.as_flattened();

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[
            BLACK, LIGHT_GRAY, WHITE, WHITE, WHITE, WHITE, LIGHT_GRAY, BLACK,
        ]);

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 1))
        .has_pixels(&[BLACK, WHITE, WHITE, WHITE, WHITE, WHITE, WHITE, WHITE]);

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 2))
        .has_pixels(&[BLACK, WHITE, WHITE, WHITE, WHITE, WHITE, WHITE, WHITE]);
}

#[test]
fn should_render_tile_line_in_middle_of_frame() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 1, SAMPLE_TILE_A);

    gpu.video_ram[0x1A10] = 0x1;
    gpu.ly = 3;
    gpu.scy = 0x80;
    gpu.scx = 0x80;
    gpu.lcdc = 0b1000_0011;

    gpu.write_scanline();

    let frame_buffer = gpu.frame_buffer.as_flattened();

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 3))
        .has_pixels(&[BLACK, WHITE, BLACK, BLACK, BLACK, BLACK, WHITE, BLACK]);
}

#[test]
fn should_render_tile_line_properly_with_greater_scroll_x_value() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 1, SAMPLE_TILE_A);

    gpu.video_ram[0x1A10] = 0x1;
    gpu.ly = 3;
    gpu.scy = 0x80;
    gpu.scx = 0x82;
    gpu.lcdc = 0b1000_0011;

    gpu.write_scanline();

    let frame_buffer = gpu.frame_buffer.as_flattened();

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 3))
        .has_pixels(&[BLACK, BLACK, BLACK, BLACK, WHITE, BLACK, BLACK, BLACK]);
}

#[test]
fn should_wrap_around_when_rendering_past_max_tile_map_x_value() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 1, SAMPLE_TILE_A);

    gpu.video_ram[0x1800] = 0x1;
    gpu.ly = 0;
    gpu.scx = 0xFE;
    gpu.lcdc = 0b1000_0011;

    gpu.write_scanline();

    let frame_buffer = gpu.frame_buffer.as_flattened();

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[BLACK, BLACK, BLACK, LIGHT_GRAY, WHITE, WHITE, WHITE, WHITE]);
}

#[test]
fn should_wrap_around_when_rendering_past_max_tile_map_y_value() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 1, SAMPLE_TILE_A);

    gpu.video_ram[0x1800] = 0x1;
    gpu.ly = 2;
    gpu.scy = 0xFE;
    gpu.lcdc = 0b1000_0011;

    gpu.write_scanline();

    let frame_buffer = gpu.frame_buffer.as_flattened();

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 2))
        .has_pixels(&[
            BLACK, LIGHT_GRAY, WHITE, WHITE, WHITE, WHITE, LIGHT_GRAY, BLACK,
        ]);
}

#[test]
fn should_render_tile_line_with_sprite() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);
    write_tile_to_obj_memory(&mut gpu, 1, SAMPLE_TILE_B);

    write_sprite_to_oam(
        &mut gpu,
        Sprite {
            y_pos: 0,
            x_pos: 2,
            tile_index: 1,
            priority: false,
            y_flip: false,
            x_flip: false,
            dmg_palette: 0,
            oam_index: 0,
            cgb_from_bank_one: false,
            cgb_palette: 0,
        },
    );

    gpu.ly = 0;
    gpu.lcdc = 0b1000_0011;

    gpu.write_scanline();

    let frame_buffer = gpu.frame_buffer.as_flattened();

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[
            BLACK, LIGHT_GRAY, DARK_GRAY, DARK_GRAY, DARK_GRAY, DARK_GRAY, DARK_GRAY, DARK_GRAY,
        ]);
}

#[test]
fn should_render_sprite_over_background_color_0_if_background_and_window_enabled_is_cleared() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);
    write_tile_to_obj_memory(&mut gpu, 1, SAMPLE_TILE_B);

    write_sprite_to_oam(
        &mut gpu,
        Sprite {
            y_pos: 0,
            x_pos: 2,
            tile_index: 1,
            priority: false,
            y_flip: false,
            x_flip: false,
            dmg_palette: 0,
            oam_index: 0,
            cgb_from_bank_one: false,
            cgb_palette: 0,
        },
    );

    gpu.ly = 0;
    gpu.lcdc = 0b1000_0010;

    gpu.write_scanline();

    let frame_buffer = gpu.frame_buffer.as_flattened();

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[
            BLACK, BLACK, DARK_GRAY, DARK_GRAY, DARK_GRAY, DARK_GRAY, DARK_GRAY, DARK_GRAY,
        ]);
}

#[test]
fn should_render_tile_line_with_sprite_having_negative_y_pos() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);
    write_tile_to_obj_memory(&mut gpu, 1, SAMPLE_TILE_B);

    write_sprite_to_oam(
        &mut gpu,
        Sprite {
            y_pos: -2,
            x_pos: 2,
            tile_index: 1,
            priority: false,
            y_flip: false,
            x_flip: false,
            dmg_palette: 0,
            oam_index: 0,
            cgb_from_bank_one: false,
            cgb_palette: 0,
        },
    );

    gpu.ly = 0;
    gpu.lcdc = 0b1000_0011;

    gpu.write_scanline();

    let frame_buffer = gpu.frame_buffer.as_flattened();

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[
            BLACK, LIGHT_GRAY, WHITE, WHITE, WHITE, WHITE, LIGHT_GRAY, DARK_GRAY,
        ]);
}

#[test]
fn should_flip_sprite_on_x_axis() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);
    write_tile_to_obj_memory(&mut gpu, 1, SAMPLE_TILE_B);

    write_sprite_to_oam(
        &mut gpu,
        Sprite {
            y_pos: -2,
            x_pos: 2,
            tile_index: 1,
            priority: false,
            y_flip: false,
            x_flip: true,
            dmg_palette: 0,
            oam_index: 0,
            cgb_from_bank_one: false,
            cgb_palette: 0,
        },
    );

    gpu.ly = 0;
    gpu.lcdc = 0b1000_0011;

    gpu.write_scanline();

    let frame_buffer = gpu.frame_buffer.as_flattened();

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[
            BLACK, LIGHT_GRAY, WHITE, WHITE, DARK_GRAY, WHITE, LIGHT_GRAY, BLACK,
        ]);
}

#[test]
fn should_flip_sprite_on_y_axis() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);
    write_tile_to_obj_memory(&mut gpu, 1, SAMPLE_TILE_B);

    write_sprite_to_oam(
        &mut gpu,
        Sprite {
            y_pos: -2,
            x_pos: 2,
            tile_index: 1,
            priority: false,
            y_flip: true,
            x_flip: false,
            dmg_palette: 0,
            oam_index: 0,
            cgb_from_bank_one: false,
            cgb_palette: 0,
        },
    );

    gpu.ly = 0;
    gpu.lcdc = 0b1000_0011;

    gpu.write_scanline();

    let frame_buffer = gpu.frame_buffer.as_flattened();

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[
            BLACK, LIGHT_GRAY, WHITE, WHITE, DARK_GRAY, WHITE, LIGHT_GRAY, DARK_GRAY,
        ]);
}

#[test]
fn should_render_eight_by_sixteen_sprite() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, BLACK_TILE);
    write_tile_to_obj_memory(&mut gpu, 2, SAMPLE_TILE_A);
    write_tile_to_obj_memory(&mut gpu, 3, SAMPLE_TILE_B);

    write_sprite_to_oam(
        &mut gpu,
        Sprite {
            y_pos: 0,
            x_pos: 2,
            tile_index: 3,
            priority: false,
            y_flip: false,
            x_flip: false,
            dmg_palette: 0,
            oam_index: 0,
            cgb_from_bank_one: false,
            cgb_palette: 0,
        },
    );

    gpu.ly = 0;
    gpu.lcdc = 0b1000_0111;

    for _ in 0..9 {
        gpu.write_scanline();
        gpu.ly += 1;
    }

    let frame_buffer = gpu.frame_buffer.as_flattened();

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[BLACK, BLACK, BLACK, LIGHT_GRAY, WHITE, WHITE, WHITE, WHITE]);

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 8))
        .has_pixels(&[
            BLACK, BLACK, DARK_GRAY, DARK_GRAY, DARK_GRAY, DARK_GRAY, DARK_GRAY, DARK_GRAY,
        ]);
}

#[test]
fn should_prioritize_non_color_id_zero_background_colors_when_sprite_priority_flag_set_to_true() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);
    write_tile_to_obj_memory(&mut gpu, 1, SAMPLE_TILE_B);

    write_sprite_to_oam(
        &mut gpu,
        Sprite {
            y_pos: 0,
            x_pos: 2,
            tile_index: 1,
            priority: true,
            y_flip: false,
            x_flip: false,
            dmg_palette: 0,
            oam_index: 0,
            cgb_from_bank_one: false,
            cgb_palette: 0,
        },
    );

    gpu.ly = 0;
    gpu.lcdc = 0b1000_0011;

    gpu.write_scanline();

    let frame_buffer = gpu.frame_buffer.as_flattened();

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[
            BLACK, LIGHT_GRAY, WHITE, WHITE, WHITE, WHITE, LIGHT_GRAY, DARK_GRAY,
        ]);
}

#[test]
fn should_prioritize_background_colors_when_lcdc_bit_1_is_off() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);
    write_tile_to_obj_memory(&mut gpu, 1, SAMPLE_TILE_B);

    write_sprite_to_oam(
        &mut gpu,
        Sprite {
            y_pos: 0,
            x_pos: 2,
            tile_index: 1,
            priority: false,
            y_flip: false,
            x_flip: false,
            dmg_palette: 0,
            oam_index: 0,
            cgb_from_bank_one: false,
            cgb_palette: 0,
        },
    );

    gpu.ly = 0;
    gpu.lcdc = 0b1000_0001;

    gpu.write_scanline();

    let frame_buffer = gpu.frame_buffer.as_flattened();

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[
            BLACK, LIGHT_GRAY, WHITE, WHITE, WHITE, WHITE, LIGHT_GRAY, BLACK,
        ]);
}

#[test]
fn should_find_the_first_ten_objects_on_the_line_in_oam_order() {
    let mut gpu = Gpu::new();
    let oam_y_values = [0, 16, 44, 9, 14, 16, 0, 10, 16, 14, 8, 11, 13, 16, 14, 15];
    for (index, oam_y) in oam_y_values.into_iter().enumerate() {
        gpu.object_attribute_memory[index * 4] = oam_y;
        gpu.object_attribute_memory[index * 4 + 1] = index as u8;
    }
    gpu.ly = 0;

    gpu.clear_visible_objects();
    gpu.scan_oam_entries(0..OAM_ENTRY_COUNT);

    let pipeline = &gpu.pipeline;
    let count = usize::from(pipeline.visible_object_count);
    let mut found = pipeline.visible_object_indices[..count].to_vec();
    found.sort_unstable();
    assert_eq!(found, [1, 3, 4, 5, 7, 8, 9, 11, 12, 13]);
    assert!(
        pipeline.visible_object_x[..count].is_sorted_by(|a, b| a >= b),
        "the objects are sorted by descending x"
    );
}

/// A PPU drawing line 1, at position 40 of the line, where a save state can catch it.
fn gpu_drawing() -> Gpu {
    let mut gpu = Gpu::new();
    gpu.set_lcdc(0b1000_0000);
    gpu.set_frame_position(1, 0);
    let mut hdma = HdmaState::new();
    for _ in 0..DOTS_PER_LINE {
        if gpu.line_state == LineState::Drawing
            && gpu.fetcher_start_delay == 0
            && gpu.pipeline.position_in_line == 40
        {
            return gpu;
        }
        gpu.tick_dot(&mut hdma);
    }
    panic!("the PPU never drew position 40");
}

/// Whether a save state of `gpu` loads.
fn state_loads(gpu: &Gpu) -> bool {
    let mut state = Vec::new();
    gpu.serialize(&mut state).unwrap();
    Gpu::new().deserialize(&mut state.as_slice()).is_ok()
}

#[test]
fn a_save_state_fetching_an_object_while_none_is_left_does_not_load() {
    let mut gpu = gpu_drawing();
    gpu.pipeline.phase = Phase::ReadObjectAttributes;
    gpu.pipeline.visible_object_count = 1;
    assert!(state_loads(&gpu));

    gpu.pipeline.visible_object_count = 0;
    assert!(!state_loads(&gpu));
}

#[test]
fn a_save_state_still_drawing_a_finished_line_does_not_load() {
    let mut gpu = gpu_drawing();
    gpu.pipeline.phase = Phase::Render;
    gpu.pipeline.position_in_line = GB_SCREEN_WIDTH as u8 - 1;
    gpu.pipeline.lcd_x = GB_SCREEN_WIDTH as u8 - 1;
    assert!(state_loads(&gpu));

    gpu.pipeline.position_in_line = GB_SCREEN_WIDTH as u8;
    gpu.pipeline.lcd_x = GB_SCREEN_WIDTH as u8;
    assert!(!state_loads(&gpu));

    let mut gpu = gpu_drawing();
    gpu.pipeline.phase = Phase::Done;
    assert!(!state_loads(&gpu));
}
