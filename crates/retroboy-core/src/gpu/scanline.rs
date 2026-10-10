use crate::gpu::constants::{GB_SCREEN_WIDTH, BYTES_PER_COLOR};
use crate::gpu::Gpu;
use crate::gpu::prioritization::resolve_highest_priority_pixel;
use crate::gpu::sprites::collect_scanline_sprites;
use crate::gpu::utils::get_bg_and_window_enabled_mode;

impl Gpu {
    pub(super) fn write_scanline(&mut self) {
        let ly = self.ly;
        let lcdc = self.lcdc;

        let sprite_buffer = collect_scanline_sprites(&self.object_attribute_memory, ly, lcdc);

        for viewport_x in 0..GB_SCREEN_WIDTH as u8 {
            let bg_pixel = self.read_window_color(viewport_x)
                .unwrap_or(self.read_bg_color(viewport_x));

            let maybe_sprite_pixel = self.read_sprite_pixel_color(&sprite_buffer, viewport_x);

            let lcdc_bg_and_window_priority = get_bg_and_window_enabled_mode(lcdc);
            let color = resolve_highest_priority_pixel(self.cgb_mode, lcdc_bg_and_window_priority, bg_pixel, maybe_sprite_pixel);

            let pixel_position = ly as u32 * GB_SCREEN_WIDTH + viewport_x as u32;
            let pixel_index = (pixel_position * BYTES_PER_COLOR) as usize;

            self.frame_buffer[pixel_index] = color[0];
            self.frame_buffer[pixel_index + 1] = color[1];
            self.frame_buffer[pixel_index + 2] = color[2];
            self.frame_buffer[pixel_index + 3] = color[3];
        }
    }
}

#[cfg(test)]
mod tests;