use crate::gpu::palettes::{Color, WHITE};

pub(super) struct BackgroundPixel {
    pub color: Color,
    pub color_id: u8,
    pub prioritize_bg: bool
}

pub(super) struct SpritePixel {
    pub color: Color,
    pub prioritize_bg: bool
}

pub(super) fn resolve_highest_priority_pixel(cgb_mode: bool, lcdc_bg_and_window_priority: bool, bg_pixel: BackgroundPixel, maybe_sprite_pixel: Option<SpritePixel>) -> Color {
    match maybe_sprite_pixel {
        Some(sprite_pixel) if !cgb_mode => {
            if (bg_pixel.color_id == 0 && sprite_pixel.prioritize_bg) || !sprite_pixel.prioritize_bg {
                sprite_pixel.color
            }
            else {
                bg_pixel.color
            }
        },
        Some(sprite_pixel) => {
            if bg_pixel.color_id == 0 || !lcdc_bg_and_window_priority || (!bg_pixel.prioritize_bg && !sprite_pixel.prioritize_bg) {
                sprite_pixel.color
            }
            else {
                bg_pixel.color
            }
        },
        _ if !cgb_mode && !lcdc_bg_and_window_priority => WHITE,
        _ => bg_pixel.color
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::palettes::{WHITE, DARK_GRAY, LIGHT_GRAY};

    #[test]
    fn should_return_background_pixel_if_no_sprite() {
        let cgb_mode = false;
        let lcdc_bg_and_window_priority = true;
        let bg_pixel = BackgroundPixel { color: LIGHT_GRAY, color_id: 1, prioritize_bg: false };
        let pixel = resolve_highest_priority_pixel(cgb_mode, lcdc_bg_and_window_priority, bg_pixel, None);
        assert_eq!(pixel, LIGHT_GRAY);
    }

    #[test]
    fn should_return_white_if_lcdc_bg_window_priority_is_false_in_dmg_mode() {
        let cgb_mode = false;
        let lcdc_bg_and_window_priority = false;
        let bg_pixel = BackgroundPixel { color: LIGHT_GRAY, color_id: 1, prioritize_bg: false };
        let pixel = resolve_highest_priority_pixel(cgb_mode, lcdc_bg_and_window_priority, bg_pixel, None);
        assert_eq!(pixel, WHITE); 
    }

    #[test]
    fn should_prioritize_sprite_when_background_uses_color_id_zero_in_dmg_mode() {
        let cgb_mode = false;
        let lcdc_bg_and_window_priority = true;
        let bg_pixel = BackgroundPixel { color: WHITE, color_id: 0, prioritize_bg: false };
        let sprite_pixel = SpritePixel { color: LIGHT_GRAY, prioritize_bg: true };
        let pixel = resolve_highest_priority_pixel(cgb_mode, lcdc_bg_and_window_priority, bg_pixel, Some(sprite_pixel));
        assert_eq!(pixel, LIGHT_GRAY);
    }

    #[test]
    fn should_prioritize_sprite_when_prioritize_bg_is_false_in_dmg_mode() {
        let cgb_mode = false;
        let lcdc_bg_and_window_priority = true;
        let bg_pixel = BackgroundPixel { color: DARK_GRAY, color_id: 2, prioritize_bg: false };
        let sprite_pixel = SpritePixel { color: LIGHT_GRAY, prioritize_bg: false };
        let pixel = resolve_highest_priority_pixel(cgb_mode, lcdc_bg_and_window_priority, bg_pixel, Some(sprite_pixel));
        assert_eq!(pixel, LIGHT_GRAY);
    }

    #[test]
    fn should_prioritize_background_when_prioritize_bg_is_true_in_dmg_mode() {
        let cgb_mode = false;
        let lcdc_bg_and_window_priority = true;
        let bg_pixel = BackgroundPixel { color: DARK_GRAY, color_id: 2, prioritize_bg: false };
        let sprite_pixel = SpritePixel { color: LIGHT_GRAY, prioritize_bg: true };
        let pixel = resolve_highest_priority_pixel(cgb_mode, lcdc_bg_and_window_priority, bg_pixel, Some(sprite_pixel));
        assert_eq!(pixel, DARK_GRAY);
    }

    #[test]
    fn should_prioritize_sprite_when_background_uses_color_id_zero_in_cgb_mode() {
        let cgb_mode = true;
        let lcdc_bg_and_window_priority = true;
        let bg_pixel = BackgroundPixel { color: WHITE, color_id: 0, prioritize_bg: true };
        let sprite_pixel = SpritePixel { color: LIGHT_GRAY, prioritize_bg: true };
        let pixel = resolve_highest_priority_pixel(cgb_mode, lcdc_bg_and_window_priority, bg_pixel, Some(sprite_pixel));
        assert_eq!(pixel, LIGHT_GRAY);
    }

    #[test]
    fn should_prioritize_sprite_when_lcdc_bg_window_priority_is_false_in_cgb_mode() {
        let cgb_mode = true;
        let lcdc_bg_and_window_priority = false;
        let bg_pixel = BackgroundPixel { color: DARK_GRAY, color_id: 2, prioritize_bg: true };
        let sprite_pixel = SpritePixel { color: LIGHT_GRAY, prioritize_bg: true };
        let pixel = resolve_highest_priority_pixel(cgb_mode, lcdc_bg_and_window_priority, bg_pixel, Some(sprite_pixel));
        assert_eq!(pixel, LIGHT_GRAY);
    }

    #[test]
    fn should_prioritize_sprite_if_neither_prioritize_bg_flags_are_true() {
        let cgb_mode = true;
        let lcdc_bg_and_window_priority = true;
        let bg_pixel = BackgroundPixel { color: DARK_GRAY, color_id: 2, prioritize_bg: false };
        let sprite_pixel = SpritePixel { color: LIGHT_GRAY, prioritize_bg: false };
        let pixel = resolve_highest_priority_pixel(cgb_mode, lcdc_bg_and_window_priority, bg_pixel, Some(sprite_pixel));
        assert_eq!(pixel, LIGHT_GRAY); 
    }

    #[test]
    fn should_prioritize_background_if_at_least_one_prioritize_bg_flag_is_true() {
        let cgb_mode = true;
        let lcdc_bg_and_window_priority = true;
        let bg_pixel = BackgroundPixel { color: DARK_GRAY, color_id: 2, prioritize_bg: true };
        let sprite_pixel = SpritePixel { color: LIGHT_GRAY, prioritize_bg: false };
        let pixel = resolve_highest_priority_pixel(cgb_mode, lcdc_bg_and_window_priority, bg_pixel, Some(sprite_pixel));
        assert_eq!(pixel, DARK_GRAY);  
    }
}