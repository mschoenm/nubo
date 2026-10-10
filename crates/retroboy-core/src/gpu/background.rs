use crate::gpu::palettes::calculate_color_id;
use crate::gpu::line_addressing::{calculate_bg_tile_map_index, calculate_tile_data_index, get_cgb_tile_attributes};
use crate::gpu::prioritization::BackgroundPixel;
use crate::gpu::utils::get_tile_line_bytes;
use crate::gpu::Gpu;

impl Gpu {
    pub(super) fn read_bg_color(&self, viewport_x: u8) -> BackgroundPixel {
        let scx = self.scx;
        let scy = self.scy;
        let ly = self.ly;
        let lcdc = self.lcdc;

        let x = scx.wrapping_add(viewport_x);
        let y = scy.wrapping_add(ly);

        let column_tile_offset = y / 8;
        let row_tile_offset = x / 8;

        let tile_map_index = calculate_bg_tile_map_index(lcdc, column_tile_offset, row_tile_offset);
        let tile_index = self.video_ram[tile_map_index as usize];
        let tile_data_index = calculate_tile_data_index(lcdc, tile_index);

        let row_offset = y % 8;
        let bit_index = x % 8;

        if self.cgb_mode {
            let attributes = get_cgb_tile_attributes(&self.video_ram, tile_map_index);
            let (lsb_byte, msb_byte) = get_tile_line_bytes(&self.video_ram, tile_data_index, row_offset, attributes.y_flip, attributes.from_bank_one);

            let dmg_compatible = self.has_dmg_compatability();
            let palette_number = if dmg_compatible { 0 } else { attributes.palette_number };
            let color_id = calculate_color_id(bit_index, msb_byte, lsb_byte, attributes.x_flip);
            let color = self.palettes.as_cgb_bg_color_rgb(palette_number, color_id, dmg_compatible);

            BackgroundPixel { color, color_id, prioritize_bg: attributes.priority }
        }
        else {
            let (lsb_byte, msb_byte) = get_tile_line_bytes(&self.video_ram, tile_data_index, row_offset, false, false);

            let color_id = calculate_color_id(bit_index, msb_byte, lsb_byte, false);
            let color = self.palettes.as_dmg_bg_color_rgb(color_id);
            
            BackgroundPixel { color, color_id, prioritize_bg: false }
        }
    }
}
