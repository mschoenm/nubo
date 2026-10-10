use crate::utils::is_bit_set;

const LCDC_BG_AND_WINDOW_ENABLED_INDEX: u8 = 0;
const LCDC_OBJ_ENABLED_INDEX: u8 = 1;
const LCDC_OBJ_SIZE_INDEX: u8 = 2;
const LCDC_BG_TILE_MAP_INDEX: u8 = 3;
const LCDC_TILE_DATA_INDEX: u8 = 4;
const LCDC_WINDOW_ENABLED_INDEX: u8 = 5;
const LCDC_WINDOW_TILE_MAP_INDEX: u8 = 6;
const LCDC_ENABLED_INDEX: u8 = 7;

pub(super) fn get_bg_and_window_enabled_mode(lcdc: u8) -> bool {
    is_bit_set(lcdc, LCDC_BG_AND_WINDOW_ENABLED_INDEX)
}

pub(super) fn get_obj_enabled_mode(lcdc: u8) -> bool {
    is_bit_set(lcdc, LCDC_OBJ_ENABLED_INDEX) 
}

pub(super) fn get_obj_size_mode(lcdc: u8) -> bool {
    is_bit_set(lcdc, LCDC_OBJ_SIZE_INDEX)
}

pub(super) fn get_bg_tile_map_mode(lcdc: u8) -> bool {
    is_bit_set(lcdc, LCDC_BG_TILE_MAP_INDEX)
}

pub(super) fn get_tile_data_addressing_mode(lcdc: u8) -> bool {
    is_bit_set(lcdc, LCDC_TILE_DATA_INDEX)
}

pub(super) fn get_window_enabled_mode(lcdc: u8) -> bool {
    is_bit_set(lcdc, LCDC_WINDOW_ENABLED_INDEX)
}

pub(super) fn get_window_tile_map_mode(lcdc: u8) -> bool {
    is_bit_set(lcdc, LCDC_WINDOW_TILE_MAP_INDEX)
}

pub(super) fn get_lcd_enabled_mode(lcdc: u8) -> bool {
    is_bit_set(lcdc, LCDC_ENABLED_INDEX)
}

fn calculate_line_index(tile_data_index: u16, row_offset: u8, y_flip: bool, from_bank_one: bool) -> u16 {
    let byte_offset = if y_flip {
        0xF - ((row_offset * 2) + 1)
    }
    else {
        row_offset * 2
    } as u16;

    let index = tile_data_index + byte_offset;
    if from_bank_one { index + 0x2000 } else { index }
}

pub(super) fn get_tile_line_bytes(video_ram: &[u8], tile_data_index: u16, row_offset: u8, y_flip: bool, from_bank_one: bool) -> (u8, u8) {
    let line_index = calculate_line_index(tile_data_index, row_offset, y_flip, from_bank_one);
    let lsb_byte = video_ram[line_index as usize];
    let msb_byte = video_ram[(line_index + 1) as usize];
    (lsb_byte, msb_byte)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_return_obj_size_mode() {
        let result = get_obj_size_mode(0x04);
        assert_eq!(result, true); 
    }

    #[test]
    fn should_return_bg_tile_map_mode() {
        let result = get_bg_tile_map_mode(0x08);
        assert_eq!(result, true);
    }

    #[test]
    fn should_return_tile_data_mode() {
        let result = get_tile_data_addressing_mode(0x10);
        assert_eq!(result, true);
    }

    #[test]
    fn should_return_window_enabled_mode() {
        let result = get_window_enabled_mode(0x20);
        assert_eq!(result, true);
    }

    #[test]
    fn should_return_window_tile_map_mode() {
        let result = get_window_tile_map_mode(0x40);
        assert_eq!(result, true);
    }

    #[test]
    fn should_return_lcdc_enabled_mode() {
        let result = get_lcd_enabled_mode(0x80);
        assert_eq!(result, true);
    }
}