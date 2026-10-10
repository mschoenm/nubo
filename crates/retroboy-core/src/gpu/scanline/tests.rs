use crate::gpu::Gpu;
use crate::gpu::palettes::{Color, Palettes, BLACK, DARK_GRAY, LIGHT_GRAY, WHITE};
use crate::gpu::sprites::Sprite;
use crate::utils::set_bit;
use super::*;

const BLACK_TILE: [u8; 16] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
const SAMPLE_TILE_A: [u8; 16] = [0x3C, 0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x5E, 0x7E, 0x0A, 0x7C, 0x56, 0x38, 0x7C];
const SAMPLE_TILE_B: [u8; 16] = [0xFF, 0x0, 0x7E, 0xFF, 0x85, 0x81, 0x89, 0x83, 0x93, 0x85, 0xA5, 0x8B, 0xC9, 0x97, 0x7E, 0xFF];
const WINDOW_TILE: [u8; 16] = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];

const RED: Color = [0xFF, 0x0, 0x0, 0xFF];
const BLUE: Color = [0x0, 0x0, 0xFF, 0xFF];

fn write_tile_to_memory(gpu: &mut Gpu, base_index: u16, index: u16, tile_bytes: [u8; 16]) {
    let offset = index * 16;
    for (tile_byte_index, tile_byte) in tile_bytes.iter().enumerate() {
        gpu.video_ram[(base_index + offset + tile_byte_index as u16) as usize] = *tile_byte;
    }
}

fn write_tile_to_bg_memory(gpu: &mut Gpu, index: u16, tile_bytes: [u8; 16]) {
    write_tile_to_memory(gpu, 0x1000, index, tile_bytes)
}

fn write_tile_to_bg_memory_in_bank_one(gpu: &mut Gpu, index: u16, tile_bytes: [u8; 16]) {
    write_tile_to_memory(gpu, 0x3000, index, tile_bytes)
}

fn write_tile_to_obj_memory(gpu: &mut Gpu, index: u16, tile_bytes: [u8; 16]) {
    write_tile_to_memory(gpu, 0x0000, index, tile_bytes)
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
    if sprite.priority { attributes = set_bit(attributes, 7); }
    if sprite.y_flip { attributes = set_bit(attributes, 6); }
    if sprite.x_flip { attributes = set_bit(attributes, 5); }
    if sprite.dmg_palette == 1 { attributes = set_bit(attributes, 4); }
    if sprite.cgb_from_bank_one { attributes = set_bit(attributes, 3); }
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
    palettes.set_bgp(0b00011011);
    palettes.set_obp0(0b00011011);
}

fn initialize_color_palettes(palettes: &mut Palettes) {
    // Red
    palettes.set_cgb_bcpd_by_index(8, 0b00011111);
    palettes.set_cgb_bcpd_by_index(9, 0);

    // Green
    palettes.set_cgb_bcpd_by_index(10, 0b11100000);
    palettes.set_cgb_bcpd_by_index(11, 0b00000011);

    // Blue
    palettes.set_cgb_bcpd_by_index(12, 0);
    palettes.set_cgb_bcpd_by_index(13, 0b01111100);
}

struct FrameBufferAssertion<'a> {
    frame_buffer: &'a Vec<u8>,
    coordinates: (u32, u32)
}

impl <'a> FrameBufferAssertion<'a> {
    pub fn new(frame_buffer: &Vec<u8>) -> FrameBufferAssertion {
        FrameBufferAssertion {
            frame_buffer,
            coordinates: (0, 0),
        }
    }

    pub fn at_starting_coordinates(&mut self, coordinates: (u32, u32)) -> &mut Self {
        self.coordinates = coordinates;
        self
    }

    fn assert_pixel_color(&self, frame_buffer: &Vec<u8>, pixel_position: u32, color: Color) {
        let pixel_index = (pixel_position * 4) as usize;
        assert_eq!(frame_buffer[pixel_index], color[0]);
        assert_eq!(frame_buffer[pixel_index + 1], color[1]);
        assert_eq!(frame_buffer[pixel_index + 2], color[2]);
        assert_eq!(frame_buffer[pixel_index + 3], color[3]);
    }

    pub fn has_pixels(&self, color_pixels: &[Color]) {
        let (x, y) = self.coordinates;
        let pixel_position = y * GB_SCREEN_WIDTH + x;

        for (index, color) in color_pixels.iter().enumerate() {
            self.assert_pixel_color(self.frame_buffer, pixel_position + index as u32, *color);
        }
    }
}

fn assert_that(frame_buffer: &Vec<u8>) -> FrameBufferAssertion {
    FrameBufferAssertion::new(frame_buffer)
}

#[test]
fn should_render_tile_line() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);

    gpu.ly = 0;
    gpu.lcdc = 0b10000011;

    gpu.write_scanline();

    let frame_buffer = &gpu.frame_buffer;

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[BLACK, LIGHT_GRAY, WHITE, WHITE, WHITE, WHITE, LIGHT_GRAY, BLACK]);
}

#[test]
fn should_render_multiple_tile_lines() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);

    gpu.lcdc = 0b10000011;

    for _ in 0..3 {
        gpu.write_scanline();
        gpu.ly += 1;
    }

    let frame_buffer = &gpu.frame_buffer;

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[BLACK, LIGHT_GRAY, WHITE, WHITE, WHITE, WHITE, LIGHT_GRAY, BLACK]);

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
    
    gpu.cgb_mode = true;

    initialize_color_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);

    write_tile_attributes(&mut gpu, 0, 0b00000001);

    gpu.lcdc = 0b10000011;

    for _ in 0..8 {
        gpu.write_scanline();
        gpu.ly += 1;
    }

    let frame_buffer = &gpu.frame_buffer;

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

    gpu.cgb_mode = true;

    initialize_color_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory_in_bank_one(&mut gpu, 0, SAMPLE_TILE_A);

    write_tile_attributes(&mut gpu, 0, 0b00001001);

    gpu.lcdc = 0b10000011;

    for _ in 0..8 {
        gpu.write_scanline();
        gpu.ly += 1;
    }

    let frame_buffer = &gpu.frame_buffer;

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

    gpu.cgb_mode = true;

    initialize_color_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);

    write_tile_attributes(&mut gpu, 0, 0b01000001);

    gpu.lcdc = 0b10000011;

    for _ in 0..8 {
        gpu.write_scanline();
        gpu.ly += 1;
    }

    let frame_buffer = &gpu.frame_buffer;

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

    gpu.cgb_mode = true;

    initialize_color_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);

    write_tile_attributes(&mut gpu, 0, 0b00100001);

    gpu.lcdc = 0b10000011;

    for _ in 0..8 {
        gpu.write_scanline();
        gpu.ly += 1;
    }

    let frame_buffer = &gpu.frame_buffer;

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
    gpu.lcdc = 0b11100011;

    for _ in 0..3 {
        gpu.write_scanline();
        gpu.ly += 1;
    }

    let frame_buffer = &gpu.frame_buffer;

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[BLACK, LIGHT_GRAY, WHITE, WHITE, WHITE, WHITE, LIGHT_GRAY, BLACK]);

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
    gpu.lcdc = 0b10000011;
    
    gpu.write_scanline();

    let frame_buffer = &gpu.frame_buffer;

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
    gpu.lcdc = 0b10000011;

    gpu.write_scanline();

    let frame_buffer = &gpu.frame_buffer;

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
    gpu.lcdc = 0b10000011;

    gpu.write_scanline();

    let frame_buffer = &gpu.frame_buffer;

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
    gpu.lcdc = 0b10000011;

    gpu.write_scanline();

    let frame_buffer = &gpu.frame_buffer;

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 2))
        .has_pixels(&[BLACK, LIGHT_GRAY, WHITE, WHITE, WHITE, WHITE, LIGHT_GRAY, BLACK]);
}

#[test]
fn should_render_tile_line_with_sprite() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);
    write_tile_to_obj_memory(&mut gpu, 1, SAMPLE_TILE_B);

    write_sprite_to_oam(&mut gpu, Sprite {
        y_pos: 0,
        x_pos: 2,
        tile_index: 1,
        priority: false,
        y_flip: false,
        x_flip: false,
        dmg_palette: 0,
        oam_index: 0,
        cgb_from_bank_one: false,
        cgb_palette: 0
    });

    gpu.ly = 0;
    gpu.lcdc = 0b10000011;

    gpu.write_scanline();

    let frame_buffer: &Vec<u8> = &gpu.frame_buffer;

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[BLACK, LIGHT_GRAY, DARK_GRAY, DARK_GRAY, DARK_GRAY, DARK_GRAY, DARK_GRAY, DARK_GRAY]);
}

#[test]
fn should_render_sprite_with_white_background_if_background_and_window_enabled_is_cleared() {
     let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);
    write_tile_to_obj_memory(&mut gpu, 1, SAMPLE_TILE_B);

    write_sprite_to_oam(&mut gpu, Sprite {
        y_pos: 0,
        x_pos: 2,
        tile_index: 1,
        priority: false,
        y_flip: false,
        x_flip: false,
        dmg_palette: 0,
        oam_index: 0,
        cgb_from_bank_one: false,
        cgb_palette: 0
    });

    gpu.ly = 0;
    gpu.lcdc = 0b10000010;

    gpu.write_scanline();

    let frame_buffer = &gpu.frame_buffer;

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[WHITE, WHITE, DARK_GRAY, DARK_GRAY, DARK_GRAY, DARK_GRAY, DARK_GRAY, DARK_GRAY]);
}

#[test]
fn should_render_tile_line_with_sprite_having_negative_y_pos() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);
    write_tile_to_obj_memory(&mut gpu, 1, SAMPLE_TILE_B);

    write_sprite_to_oam(&mut gpu, Sprite {
        y_pos: -2,
        x_pos: 2,
        tile_index: 1,
        priority: false,
        y_flip: false,
        x_flip: false,
        dmg_palette: 0,
        oam_index: 0,
        cgb_from_bank_one: false,
        cgb_palette: 0
    });

    gpu.ly = 0;
    gpu.lcdc = 0b10000011;

    gpu.write_scanline();

    let frame_buffer = &gpu.frame_buffer;

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[BLACK, LIGHT_GRAY, WHITE, WHITE, WHITE, WHITE, LIGHT_GRAY, DARK_GRAY]);
}

#[test]
fn should_flip_sprite_on_x_axis() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);
    write_tile_to_obj_memory(&mut gpu, 1, SAMPLE_TILE_B);

    write_sprite_to_oam(&mut gpu, Sprite {
        y_pos: -2,
        x_pos: 2,
        tile_index: 1,
        priority: false,
        y_flip: false,
        x_flip: true,
        dmg_palette: 0,
        oam_index: 0,
        cgb_from_bank_one: false,
        cgb_palette: 0
    });

    gpu.ly = 0;
    gpu.lcdc = 0b10000011;

    gpu.write_scanline();

    let frame_buffer = &gpu.frame_buffer;

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[BLACK, LIGHT_GRAY, WHITE, WHITE, DARK_GRAY, WHITE, LIGHT_GRAY, BLACK]);
}

#[test]
fn should_flip_sprite_on_y_axis() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);
    write_tile_to_obj_memory(&mut gpu, 1, SAMPLE_TILE_B);

    write_sprite_to_oam(&mut gpu, Sprite {
        y_pos: -2,
        x_pos: 2,
        tile_index: 1,
        priority: false,
        y_flip: true,
        x_flip: false,
        dmg_palette: 0,
        oam_index: 0,
        cgb_from_bank_one: false,
        cgb_palette: 0
    });
    
    gpu.ly = 0;
    gpu.lcdc = 0b10000011;

    gpu.write_scanline();

    let frame_buffer = &gpu.frame_buffer;

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[BLACK, LIGHT_GRAY, WHITE, WHITE, DARK_GRAY, WHITE, LIGHT_GRAY, DARK_GRAY]);
}

#[test]
fn should_render_eight_by_sixteen_sprite() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, BLACK_TILE);
    write_tile_to_obj_memory(&mut gpu, 2, SAMPLE_TILE_A);
    write_tile_to_obj_memory(&mut gpu, 3, SAMPLE_TILE_B);

    write_sprite_to_oam(&mut gpu, Sprite {
        y_pos: 0,
        x_pos: 2,
        tile_index: 3,
        priority: false,
        y_flip: false,
        x_flip: false,
        dmg_palette: 0,
        oam_index: 0,
        cgb_from_bank_one: false,
        cgb_palette: 0
    });

    gpu.ly = 0;
    gpu.lcdc = 0b10000111;

    for _ in 0..9 {
        gpu.write_scanline();
        gpu.ly += 1;
    }

    let frame_buffer = &gpu.frame_buffer;

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[BLACK, BLACK, BLACK, LIGHT_GRAY, WHITE, WHITE, WHITE, WHITE]);

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 8))
        .has_pixels(&[BLACK, BLACK, DARK_GRAY, DARK_GRAY, DARK_GRAY, DARK_GRAY, DARK_GRAY, DARK_GRAY]);
}

#[test]
fn should_prioritize_non_color_id_zero_background_colors_when_sprite_priority_flag_set_to_true() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);
    write_tile_to_obj_memory(&mut gpu, 1, SAMPLE_TILE_B);

    write_sprite_to_oam(&mut gpu, Sprite {
        y_pos: 0,
        x_pos: 2,
        tile_index: 1,
        priority: true,
        y_flip: false,
        x_flip: false,
        dmg_palette: 0,
        oam_index: 0,
        cgb_from_bank_one: false,
        cgb_palette: 0
    });

    gpu.ly = 0;
    gpu.lcdc = 0b10000011;

    gpu.write_scanline();

    let frame_buffer = &gpu.frame_buffer;

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[BLACK, LIGHT_GRAY, WHITE, WHITE, WHITE, WHITE, LIGHT_GRAY, DARK_GRAY]);
}

#[test]
fn should_prioritize_background_colors_when_lcdc_bit_1_is_off() {
    let mut gpu = Gpu::new();

    initialize_monochrome_palettes(&mut gpu.palettes);

    write_tile_to_bg_memory(&mut gpu, 0, SAMPLE_TILE_A);
    write_tile_to_obj_memory(&mut gpu, 1, SAMPLE_TILE_B);

    write_sprite_to_oam(&mut gpu, Sprite {
        y_pos: 0,
        x_pos: 2,
        tile_index: 1,
        priority: false,
        y_flip: false,
        x_flip: false,
        dmg_palette: 0,
        oam_index: 0,
        cgb_from_bank_one: false,
        cgb_palette: 0
    });
    
    gpu.ly = 0;
    gpu.lcdc = 0b10000001;

    gpu.write_scanline();

    let frame_buffer = &gpu.frame_buffer;

    assert_that(frame_buffer)
        .at_starting_coordinates((0, 0))
        .has_pixels(&[BLACK, LIGHT_GRAY, WHITE, WHITE, WHITE, WHITE, LIGHT_GRAY, BLACK]);
}