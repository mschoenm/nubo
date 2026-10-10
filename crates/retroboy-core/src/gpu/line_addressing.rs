use crate::gpu::utils::*;
use crate::utils::is_bit_set;
 
const TILES_PER_ROW: u8 = 32;
const TILE_DATA_LENGTH: u8 = 16;

pub(super) struct TileAttributes {
    pub priority: bool,
    pub y_flip: bool,
    pub x_flip: bool,
    pub from_bank_one: bool,
    pub palette_number: u8,
}

fn calculate_tile_map_index(tile_map_mode: bool, tile_map_offset: u16) -> u16 {
    if tile_map_mode {
        0x1C00 + tile_map_offset
    }
    else {
        0x1800 + tile_map_offset
    }
}

fn calculate_tile_offset(column_tile_offset: u8, row_tile_offset: u8) -> u16 {
    column_tile_offset as u16 * TILES_PER_ROW as u16 + row_tile_offset as u16
}

pub(super) fn get_cgb_tile_attributes(video_ram: &[u8], tile_map_index: u16) -> TileAttributes {
    let tile_attributes_index = 0x2000 + tile_map_index;
    let attributes_byte = video_ram[tile_attributes_index as usize];
    TileAttributes {
        priority: is_bit_set(attributes_byte, 7),
        y_flip: is_bit_set(attributes_byte, 6),
        x_flip: is_bit_set(attributes_byte, 5),
        from_bank_one: is_bit_set(attributes_byte, 3),
        palette_number: attributes_byte & 0b00000111,
    }
}

pub(super) fn calculate_bg_tile_map_index(lcdc: u8, column_tile_offset: u8, row_tile_offset: u8) -> u16 {
    let tile_map_offset = calculate_tile_offset(column_tile_offset, row_tile_offset);
    let tile_map_mode = get_bg_tile_map_mode(lcdc);
    calculate_tile_map_index(tile_map_mode, tile_map_offset)
}

pub(super) fn calculate_window_tile_map_index(lcdc: u8, column_tile_offset: u8, row_tile_offset: u8) -> u16 {
    let tile_map_offset = calculate_tile_offset(column_tile_offset, row_tile_offset);
    let tile_map_mode = get_window_tile_map_mode(lcdc);
    calculate_tile_map_index(tile_map_mode, tile_map_offset)
}

pub(super) fn calculate_tile_data_index(lcdc: u8, index: u8) -> u16 {
    let unsigned_addressing = get_tile_data_addressing_mode(lcdc);
    if unsigned_addressing {
        index as u16 * TILE_DATA_LENGTH as u16
    }
    else if index >= 128 {
        0x800 + ((index - 128) as u16 * TILE_DATA_LENGTH as u16)
    }
    else {
        0x1000 + (index as u16 * TILE_DATA_LENGTH as u16)
    }
}
