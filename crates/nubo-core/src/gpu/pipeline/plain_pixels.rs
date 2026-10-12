//! Nubo's shortcuts through mode 3, which aren't part of the SameBoy port in `pipeline.rs`: runs
//! of plain dots, in which the renderer only shifts out a pixel, draws it and takes a fetcher
//! step, are drawn without working through `Gpu::tick_drawing`'s phases, and a whole tile row
//! without objects at once. They must give exactly what `tick_drawing` gives dot by dot, which the
//! randomized tests in `gpu/tests/random_scenes.rs` check.

use super::{
    FIFO_LENGTH, FRAME_BUFFER_BOUNDS, FetcherState, OBJECT_X_OFFSET, Phase, PixelColors, WX_OFFSET,
};
use crate::gpu::Gpu;
use crate::gpu::constants::GB_SCREEN_WIDTH;
use crate::gpu::lcdc::objects_enabled;

impl Gpu {
    /// Whether the coming dot of mode 3 is a plain one that `draw_plain_pixels` can do, provided the
    /// window can't start (`window_can_start`, which the caller checks): it starts with the window
    /// check, no object starts at this position or was left behind, the background FIFO has a
    /// pixel for it, it is past the fine scroll pixels and isn't one of the last pixels of the line
    /// (which start the mode 0 interrupt countdown), and none of the glitches that the full dot
    /// handles applies. Plain pixels don't change whether the window can start.
    fn can_draw_plain_pixel(&self) -> bool {
        let pipeline = &self.pipeline;
        let position = pipeline.position_in_line;
        pipeline.phase == Phase::LineStart
            && !pipeline.object_fetch_aborted
            && pipeline.is_on_screen()
            && position < GB_SCREEN_WIDTH as u8 - 2
            && pipeline.bg_fifo.size > 0
            && !pipeline.insert_bg_pixel
            && !(pipeline.wx_triggered && self.wx == position + WX_OFFSET)
            && (pipeline.visible_object_count == 0
                || pipeline.visible_object_x[pipeline.next_object()] > position + OBJECT_X_OFFSET)
    }

    /// Runs plain dots of mode 3 (see `can_draw_plain_pixel`) as long as they come, up to `dots`,
    /// and returns how many it ran. How pixels get their colors and whether the background is
    /// drawn can't change meanwhile, so they are worked out once, and each way of picking colors
    /// gets a loop of its own. The dot of a split LCDC write (`written_bg_enable`) is left to
    /// `tick_drawing`.
    pub(in crate::gpu) fn draw_plain_pixels(&mut self, dots: u32) -> u32 {
        if self.pipeline.written_bg_enable.is_some() {
            return 0;
        }
        let cgb_rendering = self.uses_cgb_rendering();
        match self.pixel_colors() {
            PixelColors::Blocked => {
                self.draw_plain_pixels_in(dots, PixelColors::Blocked, cgb_rendering)
            }
            PixelColors::Cgb => self.draw_plain_pixels_in(dots, PixelColors::Cgb, true),
            PixelColors::Compatibility => {
                self.draw_plain_pixels_in(dots, PixelColors::Compatibility, false)
            }
            PixelColors::Dmg => self.draw_plain_pixels_in(dots, PixelColors::Dmg, false),
        }
    }

    /// `draw_plain_pixels` with the colors coming from `colors`. Each dot does exactly what
    /// `tick_drawing` does on such a dot, without working through its phases: it shifts a pixel
    /// out of the FIFOs, draws it and takes a fetcher step. Where the fetcher starts a tile on a
    /// run of eight plain dots without object pixels, the whole tile runs straight through
    /// (`draw_background_tile`).
    /// Always inlined so that `colors` is a constant in it (see `PixelColors`).
    #[inline(always)]
    fn draw_plain_pixels_in(&mut self, dots: u32, colors: PixelColors, cgb_rendering: bool) -> u32 {
        let bg_window_enabled = self.bg_window_enabled();
        let mut ran = 0;
        while ran < dots {
            if dots - ran >= FIFO_LENGTH as u32
                && self.pipeline.oam_fifo.size == 0
                && self.can_draw_plain_tile()
            {
                self.draw_background_tile(colors, cgb_rendering || bg_window_enabled);
                ran += FIFO_LENGTH as u32;
            } else if self.can_draw_plain_pixel() {
                self.draw_plain_pixel(colors, cgb_rendering, bg_window_enabled);
                self.advance_fetcher();
                ran += 1;
            } else if self.goes_straight_to_render() {
                self.line_dot += 1;
                self.pipeline.during_object_fetch = false;
                self.render_pixel_if_possible();
                self.advance_fetcher();
                ran += 1;
            } else {
                break;
            }
        }
        ran
    }

    /// Whether the coming dot of mode 3 goes from the window check straight on to drawing,
    /// provided the window can't start (`window_can_start`, which the caller checks): no object
    /// starts at this position or was left behind, the window check doesn't insert a pixel, and it
    /// isn't one of the last pixels of the line. `tick_drawing` then only draws a pixel if it can
    /// and takes a fetcher step. Unlike `can_draw_plain_pixel` this covers the fine scroll pixels
    /// at the start of the line and dots without a pixel to draw.
    fn goes_straight_to_render(&self) -> bool {
        let pipeline = &self.pipeline;
        pipeline.phase == Phase::LineStart
            && !pipeline.object_fetch_aborted
            && pipeline.signed_position() < GB_SCREEN_WIDTH as i16 - 2
            && !(pipeline.wx_triggered && self.wx == pipeline.wx_here())
            && (pipeline.visible_object_count == 0
                || pipeline.visible_object_x[pipeline.next_object()]
                    > pipeline.x_for_object_match())
    }

    /// Whether the coming eight dots are all plain (see `can_draw_plain_pixel`) and the fetcher
    /// starts a tile with them: it has just pushed a row, so it goes through its steps in order
    /// while the eight pixels of that row are drawn, and pushes the next row with the last one.
    fn can_draw_plain_tile(&self) -> bool {
        let pipeline = &self.pipeline;
        let position = pipeline.position_in_line;
        if !pipeline.is_on_screen()
            || position >= GB_SCREEN_WIDTH as u8 - 2 - (FIFO_LENGTH as u8 - 1)
        {
            return false;
        }
        let last_position = position + FIFO_LENGTH as u8 - 1;
        pipeline.fetcher_state == FetcherState::GetTileT1
            && pipeline.bg_fifo.size == FIFO_LENGTH
            && pipeline.phase == Phase::LineStart
            && !pipeline.object_fetch_aborted
            && !pipeline.insert_bg_pixel
            && !(pipeline.wx_triggered
                && (position + WX_OFFSET..=last_position + WX_OFFSET).contains(&self.wx))
            && (pipeline.visible_object_count == 0
                || pipeline.visible_object_x[pipeline.next_object()]
                    > last_position + OBJECT_X_OFFSET)
    }

    /// A plain tile (see `can_draw_plain_tile`) without object pixels: the eight pixels of the
    /// background row, with the colors from `colors` (only color 0 unless `bg_enabled`), and the
    /// fetcher's steps, each seeing the position and background FIFO it would see on its own dot.
    /// The same as eight dots of `draw_plain_pixel` and a fetcher step, without working out each
    /// pixel's color on its own.
    /// Always inlined so that `colors` is a constant in it (see `PixelColors`).
    #[inline(always)]
    fn draw_background_tile(&mut self, colors: PixelColors, bg_enabled: bool) {
        let row = self.pipeline.bg_fifo;
        let row_color = |color_id: u8| {
            self.background_color_in(colors, row.palette, if bg_enabled { color_id } else { 0 })
        };
        let row_colors = [row_color(0), row_color(1), row_color(2), row_color(3)];
        let first_pixel =
            usize::from(self.ly) * GB_SCREEN_WIDTH as usize + usize::from(self.pipeline.lcd_x);
        let color_id = |pixel: usize| {
            let bit = 7 - pixel;
            usize::from(((row.lower >> bit) & 1) | (((row.upper >> bit) & 1) << 1))
        };
        let targets = self
            .frame_buffer
            .get_mut(first_pixel..first_pixel + FIFO_LENGTH);
        debug_assert!(targets.is_some(), "{FRAME_BUFFER_BOUNDS}");
        if let Some(targets) = targets {
            for (pixel, target) in targets.iter_mut().enumerate() {
                *target = row_colors[color_id(pixel)];
            }
        }

        self.line_dot += FIFO_LENGTH as u16;
        let pipeline = &mut self.pipeline;
        pipeline.lcd_x += FIFO_LENGTH as u8;
        pipeline.during_object_fetch = false;
        pipeline.window_is_being_fetched = false;
        pipeline.window_start_backup = None;
        let position = pipeline.position_in_line;
        self.take_tile_steps(|gpu, pixels| gpu.move_on_in_tile(position, pixels));
    }

    /// The fetcher's steps over the eight dots in which a tile row's eight pixels are drawn, when
    /// it starts them in `FetcherState::GetTileT1` with a full background FIFO: one step in each
    /// state, in order, then a second push in the last dot, which finds the FIFO empty and pushes
    /// the next row. `before_step` runs before each step, with the number of the dot (1 to 8), to
    /// set up what the step sees. `draw_background_tile` takes the steps this way instead of
    /// dispatching on the state every dot like `advance_fetcher`, which debug builds check this
    /// agrees with.
    #[inline(always)]
    fn take_tile_steps(&mut self, mut before_step: impl FnMut(&mut Gpu, u8)) {
        self.take_tile_step(
            1,
            FetcherState::GetTileT1,
            &mut before_step,
            Gpu::find_tile_number,
        );
        self.take_tile_step(
            2,
            FetcherState::GetTileT2,
            &mut before_step,
            Gpu::read_tile_number,
        );
        self.take_tile_step(
            3,
            FetcherState::GetTileDataLowT1,
            &mut before_step,
            Gpu::find_tile_data_low,
        );
        self.take_tile_step(
            4,
            FetcherState::GetTileDataLowT2,
            &mut before_step,
            Gpu::read_tile_data_low,
        );
        self.take_tile_step(
            5,
            FetcherState::GetTileDataHighT1,
            &mut before_step,
            Gpu::find_tile_data_high,
        );
        self.take_tile_step(
            6,
            FetcherState::GetTileDataHighT2,
            &mut before_step,
            Gpu::read_tile_data_high,
        );
        self.take_tile_step(7, FetcherState::Push, &mut before_step, Gpu::fetcher_push);
        self.take_tile_step(8, FetcherState::Push, &mut before_step, Gpu::fetcher_push);
        // The steps end where `advance_fetcher` would: with the next row pushed, or with the
        // single blank pixel the DMG pushes instead when the window was just switched off where it
        // would start.
        debug_assert!(matches!(
            (self.pipeline.fetcher_state, self.pipeline.bg_fifo.size),
            (FetcherState::GetTileT1, FIFO_LENGTH) | (FetcherState::Push, 1)
        ));
    }

    /// One dot of `take_tile_steps`: runs `before_step`, then `step`, the step `advance_fetcher`
    /// takes in `state`.
    #[inline(always)]
    fn take_tile_step(
        &mut self,
        dot: u8,
        state: FetcherState,
        before_step: &mut impl FnMut(&mut Gpu, u8),
        step: impl FnOnce(&mut Gpu),
    ) {
        before_step(self, dot);
        debug_assert_eq!(self.pipeline.fetcher_state, state);
        step(self);
    }

    /// Sets the position and the background FIFO to where they are after `pixels` pixels of a tile
    /// drawn by `draw_background_tile`, which started at `start_position`.
    #[inline(always)]
    fn move_on_in_tile(&mut self, start_position: u8, pixels: u8) {
        let pipeline = &mut self.pipeline;
        pipeline.position_in_line = start_position + pixels;
        pipeline.bg_fifo.size = FIFO_LENGTH - usize::from(pixels);
        pipeline.bg_fifo.lower <<= 1;
        pipeline.bg_fifo.upper <<= 1;
    }

    /// The pixel of a plain dot (see `can_draw_plain_pixel`): shifts a pixel out of the FIFOs and
    /// draws it, with the colors from `colors`.
    /// Always inlined so that `colors` is a constant in it (see `PixelColors`).
    #[inline(always)]
    fn draw_plain_pixel(
        &mut self,
        colors: PixelColors,
        cgb_rendering: bool,
        bg_window_enabled: bool,
    ) {
        self.line_dot += 1;
        let lcdc = self.lcdc;
        let pipeline = &mut self.pipeline;
        pipeline.during_object_fetch = false;
        let bg_item = pipeline.bg_fifo.pop();
        let mut bg_priority = bg_item.bg_priority;
        let mut object_item = None;
        if pipeline.oam_fifo.size > 0 {
            let item = pipeline.oam_fifo.pop();
            if item.color_id != 0 && objects_enabled(lcdc) {
                bg_priority |= item.bg_priority;
                object_item = Some(item);
            }
        }
        pipeline.window_is_being_fetched = false;
        pipeline.window_start_backup = None;
        let color = self.mixed_pixel_color(
            colors,
            cgb_rendering,
            bg_window_enabled,
            bg_item,
            object_item,
            bg_priority,
        );
        self.put_pixel(color);
    }
}
