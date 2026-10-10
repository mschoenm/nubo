use crate::gpu::Gpu;
use crate::gpu::palettes::{Color, calculate_color_id};
use crate::gpu::prioritization::SpritePixel;
use crate::gpu::utils::{get_obj_enabled_mode, get_obj_size_mode, get_tile_line_bytes};
use crate::utils::{get_bit, is_bit_set};

const SPRITE_LIMIT_PER_SCANLINE: usize = 10;
const TOTAL_SPRITES: u16 = 40;

const TILE_DATA_BYTE_SIZE: u16 = 16;
const SPRITE_BYTE_SIZE: u16 = 4;

const SPRITE_WIDTH: i16 = 8;

const CGB_OPRI_PRIORITY_BIT: u8 = 0;

#[derive(Debug)]
pub(super) struct Sprite {
    pub y_pos: i16,
    pub x_pos: i16,
    pub tile_index: u8,
    pub priority: bool,
    pub y_flip: bool,
    pub x_flip: bool,
    pub dmg_palette: u8,
    pub oam_index: u16,
    pub cgb_from_bank_one: bool,
    pub cgb_palette: u8
}

impl Sprite {
    pub(super) fn has_higher_priority_than(&self, compared_sprite: &Sprite, oam_location_prioritization: bool) -> bool {
        let has_lower_x = self.x_pos < compared_sprite.x_pos;
        let has_same_x = self.x_pos == compared_sprite.x_pos;
        let located_earlier_in_oam = self.oam_index < compared_sprite.oam_index;
        (oam_location_prioritization && located_earlier_in_oam) ||
            (!oam_location_prioritization && (has_lower_x || (has_same_x && located_earlier_in_oam)))
    }
}

fn within_scanline(sprite_y_pos: i16, y_int: i16, eight_by_sixteen_mode: bool) -> bool {
    let sprite_height = if eight_by_sixteen_mode { 16 } else { 8 };
    let last_row = sprite_y_pos + sprite_height;
    y_int >= sprite_y_pos && y_int < last_row && last_row >= 0
}

fn sprite_overlaps_coordinates(sprite_x_pos: i16, sprite_y_pos: i16, x_int: i16, y_int: i16, eight_by_sixteen_mode: bool) -> bool {
    within_scanline(sprite_y_pos, y_int, eight_by_sixteen_mode)
        && x_int >= sprite_x_pos && x_int < sprite_x_pos + SPRITE_WIDTH
}

fn calculate_oam_index(sprite_number: u16) -> u16 {
    sprite_number * SPRITE_BYTE_SIZE
}

fn calculate_tile_data_index(tile_index: u16) -> u16 {
    tile_index * TILE_DATA_BYTE_SIZE
}

fn pull_sprite(object_attribute_memory: &[u8], sprite_number: u16) -> Sprite {
    let oam_index = calculate_oam_index(sprite_number);

    let y_pos = object_attribute_memory[oam_index as usize];
    let x_pos = object_attribute_memory[(oam_index + 1) as usize];
    let tile_index = object_attribute_memory[(oam_index + 2) as usize];
    let attributes = object_attribute_memory[(oam_index + 3) as usize];
    
    Sprite {
        y_pos: (y_pos as i16 - 16),
        x_pos: (x_pos as i16 - 8),
        tile_index,
        priority: is_bit_set(attributes, 7),
        y_flip: is_bit_set(attributes, 6),
        x_flip: is_bit_set(attributes, 5),
        dmg_palette: get_bit(attributes, 4),
        oam_index,
        cgb_from_bank_one: is_bit_set(attributes, 3),
        cgb_palette: attributes & 0b111
    }
}

pub(super) fn collect_scanline_sprites(object_attribute_memory: &[u8], ly: u8, lcdc: u8) -> Vec<Sprite> {
    let mut sprites = Vec::new();

    let eight_by_sixteen_mode = get_obj_size_mode(lcdc);

    for sprite_number in 0..TOTAL_SPRITES {
        let sprite = pull_sprite(object_attribute_memory, sprite_number);

        let y_int = ly as i16;

        if within_scanline(sprite.y_pos, y_int, eight_by_sixteen_mode) {
            sprites.push(sprite);

            if sprites.len() == SPRITE_LIMIT_PER_SCANLINE {
                break;
            }
        }
    }

    sprites
}

fn lookup_possible_sprites(sprite_buffer: &Vec<Sprite>, x: u8, y: u8, eight_by_sixteen_mode: bool) -> Vec<&Sprite> {
    let mut found_sprites = Vec::new();

    for sprite in sprite_buffer {
        let x_int  = x as i16;
        let y_int = y as i16;

        if sprite_overlaps_coordinates(sprite.x_pos, sprite.y_pos, x_int, y_int, eight_by_sixteen_mode) {
            found_sprites.push(sprite);
        }
    }

    found_sprites
}

fn calculate_tile_index(sprite: &Sprite, y_int: i16, eight_by_sixteen_mode: bool) -> u8 {
    if eight_by_sixteen_mode && (y_int - sprite.y_pos) >= 8 {
        if sprite.y_flip { sprite.tile_index & 0xFE } else { sprite.tile_index | 0x01 }
    }
    else if eight_by_sixteen_mode {
        if sprite.y_flip { sprite.tile_index | 0x01 } else { sprite.tile_index & 0xFE }
    }
    else {
        sprite.tile_index
    }
}

impl Gpu {
    fn calculate_sprite_pixel_color(&self, sprite: &Sprite, x: u8, y: u8) -> Option<Color> {
        let y_int = y as i16;
        let x_int  = x as i16;

        let lcdc = self.lcdc;
        let eight_by_sixteen_mode = get_obj_size_mode(lcdc);

        let calculated_index = calculate_tile_index(&sprite, y_int, eight_by_sixteen_mode);
        let tile_data_index = calculate_tile_data_index(calculated_index as u16);
        let row_offset = ((y_int - sprite.y_pos) % 8) as u8;
        let column_offset = x_int - sprite.x_pos;

        if column_offset >= 0 {
            let from_bank_one = if self.cgb_mode { sprite.cgb_from_bank_one } else { false };
            let (lsb_byte, msb_byte) = get_tile_line_bytes(&self.video_ram, tile_data_index, row_offset, sprite.y_flip, from_bank_one);

            if self.cgb_mode {            
                let dmg_compatible = self.has_dmg_compatability();
                let palette_number = if dmg_compatible { sprite.dmg_palette } else { sprite.cgb_palette };
                let color_id = calculate_color_id(column_offset as u8, msb_byte, lsb_byte, sprite.x_flip);
                
                self.palettes.as_cgb_obj_color_rgb(palette_number, color_id, dmg_compatible)
            }
            else {            
                let color_id = calculate_color_id(column_offset as u8, msb_byte, lsb_byte, sprite.x_flip);
                
                self.palettes.as_dmg_obj_color_rgb(sprite.dmg_palette, color_id) 
            }
        }
        else {
            None
        } 
    }

    fn resolve_highest_priority_sprite<'a>(&self, sprites: Vec<&'a Sprite>, x: u8, y: u8) -> Option<(&'a Sprite, Option<Color>)> {
        let mut maybe_highest_priority: Option<(&'a Sprite, Option<Color>)> = None;
        let cgb_mode = self.cgb_mode;
        let oam_location_prioritization = cgb_mode && !is_bit_set(self.cgb_opri, CGB_OPRI_PRIORITY_BIT);

        for sprite in sprites {
            match maybe_highest_priority {
                Some(highest_priority) => {
                    let current_highest_priority_sprite = highest_priority.0;
                    let maybe_current_highest_priority_color = highest_priority.1;

                    let maybe_color = self.calculate_sprite_pixel_color(sprite, x, y);
 
                    match (maybe_color, maybe_current_highest_priority_color) {
                        (Some(color), Some(_)) if sprite.has_higher_priority_than(current_highest_priority_sprite, oam_location_prioritization) => {
                            maybe_highest_priority = Some((sprite, Some(color)));
                        }
                        (Some(color), None) => {
                            maybe_highest_priority = Some((sprite, Some(color)));
                        }
                        _ => {}
                    }
                }
                None => {
                    let maybe_color = self.calculate_sprite_pixel_color(sprite, x, y);
                    maybe_highest_priority = Some((sprite, maybe_color));
                }
            }
        }

        maybe_highest_priority
    }

    pub(super) fn read_sprite_pixel_color(&self, sprite_buffer: &Vec<Sprite>, viewport_x: u8) -> Option<SpritePixel> {
        let lcdc = self.lcdc;
        let ly = self.ly;

        let eight_by_sixteen_mode = get_obj_size_mode(lcdc);
        let sprites_enabled = get_obj_enabled_mode(lcdc);

        let possible_sprites = lookup_possible_sprites(sprite_buffer, viewport_x, ly, eight_by_sixteen_mode);
        
        if sprites_enabled {
            match self.resolve_highest_priority_sprite(possible_sprites, viewport_x, ly) {
                Some((highest_priority_sprite, maybe_color)) => {
                    let prioritize_bg = highest_priority_sprite.priority;
                    maybe_color.map(|color| SpritePixel { color, prioritize_bg })
                },
                _ => None
            }
        }
        else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_sprite(object_attribute_memory: &mut [u8], sprit_number: u8, y_pos: u8, x_pos: u8, attributes: u8) {
        let index = (sprit_number * 4) as usize;
        object_attribute_memory[index] = y_pos;
        object_attribute_memory[index + 1] = x_pos;
        object_attribute_memory[index + 2] = 0x0;
        object_attribute_memory[index + 3] = attributes;
    }

    #[test]
    fn should_get_ten_sprites_from_oam_memory() {
        let mut gpu = Gpu::new();
        
        gpu.ly = 0;

        write_sprite(&mut gpu.object_attribute_memory, 0, 0, 0, 0);
        write_sprite(&mut gpu.object_attribute_memory, 1, 16, 0, 0);
        write_sprite(&mut gpu.object_attribute_memory, 2, 44, 0, 0);
        write_sprite(&mut gpu.object_attribute_memory, 3, 9, 0x1F, 0);
        write_sprite(&mut gpu.object_attribute_memory, 4, 14, 0x2A, 0);
        write_sprite(&mut gpu.object_attribute_memory, 5, 16, 0x60, 0);
        write_sprite(&mut gpu.object_attribute_memory, 6, 0, 0xFF, 0);
        write_sprite(&mut gpu.object_attribute_memory, 7, 10, 0x3F, 0);
        write_sprite(&mut gpu.object_attribute_memory, 8, 16, 0x4A, 0);
        write_sprite(&mut gpu.object_attribute_memory, 9, 14, 0x51, 0);
        write_sprite(&mut gpu.object_attribute_memory, 10, 8, 0x22, 0);
        write_sprite(&mut gpu.object_attribute_memory, 11, 11, 0x1B, 0);
        write_sprite(&mut gpu.object_attribute_memory, 12, 13, 0x14, 0);
        write_sprite(&mut gpu.object_attribute_memory, 13, 16, 0x55, 0);
        write_sprite(&mut gpu.object_attribute_memory, 14, 14, 0x22, 0);
        write_sprite(&mut gpu.object_attribute_memory, 15, 15, 0x23, 0);

        let sprites = collect_scanline_sprites(&gpu.object_attribute_memory, gpu.ly, gpu.lcdc);

        assert_eq!(sprites.len(), 10);
        assert_eq!(sprites[0].y_pos, 0);
        assert_eq!(sprites[1].y_pos, -7);
        assert_eq!(sprites[2].y_pos, -2);
        assert_eq!(sprites[3].y_pos, 0);
        assert_eq!(sprites[4].y_pos, -6);
        assert_eq!(sprites[5].y_pos, 0);
        assert_eq!(sprites[6].y_pos, -2);
        assert_eq!(sprites[7].y_pos, -5);
        assert_eq!(sprites[8].y_pos, -3);
        assert_eq!(sprites[9].y_pos, 0);
    }

    #[test]
    fn should_parse_sprite_attributes_correctly() {
        let mut gpu = Gpu::new();
        
        write_sprite(&mut gpu.object_attribute_memory, 0, 16, 0, 0b11000000);
        
        let sprites = collect_scanline_sprites(&gpu.object_attribute_memory, gpu.ly, gpu.lcdc);

        assert_eq!(sprites[0].priority, true);
        assert_eq!(sprites[0].y_flip, true);
        assert_eq!(sprites[0].x_flip, false);
        assert_eq!(sprites[0].dmg_palette, 0);
    }
}