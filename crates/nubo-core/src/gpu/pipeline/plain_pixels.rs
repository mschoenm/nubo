//! Nubo's shortcuts through mode 3, which aren't part of the SameBoy port in `pipeline.rs`: runs
//! of plain dots, in which the renderer only shifts out a pixel, draws it and takes a fetcher
//! step, are drawn without working through `Gpu::tick_drawing`'s phases, and a whole tile row
//! without objects at once, usually with the fetcher's eight steps for the next row taken in one
//! go (`fetch_tile_row_at_once`). They must give exactly what `tick_drawing` gives dot by dot,
//! which the randomized tests in `gpu/tests/random_scenes.rs` check.

use super::{
    FIFO_LENGTH, FRAME_BUFFER_BOUNDS, FetcherState, HIGH_TILE_MAP, LOW_TILE_MAP, OBJECT_X_OFFSET,
    PALETTE_BITS, PRIORITY_BIT, Phase, PixelColors, TILE_MAP_WIDTH, VRAM_BANK_SIZE, WX_OFFSET,
    X_FLIP_BIT,
};
use crate::gpu::constants::GB_SCREEN_WIDTH;
use crate::gpu::lcdc::{
    bg_uses_high_tile_map, objects_enabled, uses_8000_addressing, window_enabled,
    window_uses_high_tile_map,
};
use crate::gpu::{Gpu, TimingVariant};
use crate::utils::is_bit_set;

/// Each byte with its bits spread out to every other bit: bit `n` moves to bit `2n`. A tile row's
/// low bitplane spread out, ORed with its high bitplane spread out and shifted left by one, holds
/// all eight 2-bit color IDs at once, the leftmost pixel in the top two bits.
const SPREAD_BITS: [u16; 256] = {
    let mut table = [0; 256];
    let mut byte = 0;
    while byte < 256 {
        let mut bit = 0;
        while bit < 8 {
            table[byte] |= ((byte as u16 >> bit) & 1) << (2 * bit);
            bit += 1;
        }
        byte += 1;
    }
    table
};

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
        let color_ids =
            SPREAD_BITS[usize::from(row.lower)] | (SPREAD_BITS[usize::from(row.upper)] << 1);
        let targets = self
            .frame_buffer
            .get_mut(first_pixel..first_pixel + FIFO_LENGTH);
        debug_assert!(targets.is_some(), "{FRAME_BUFFER_BOUNDS}");
        if let Some(targets) = targets {
            for (pixel, target) in targets.iter_mut().enumerate() {
                *target = row_colors[usize::from((color_ids >> (14 - 2 * pixel)) & 3)];
            }
        }

        self.line_dot += FIFO_LENGTH as u16;
        let pipeline = &mut self.pipeline;
        pipeline.lcd_x += FIFO_LENGTH as u8;
        pipeline.during_object_fetch = false;
        pipeline.window_is_being_fetched = false;
        pipeline.window_start_backup = None;
        let position = pipeline.position_in_line;
        if self.can_fetch_tile_row_at_once(position) {
            self.fetch_tile_row_at_once(position);
        } else {
            self.take_tile_steps(|gpu, pixels| gpu.move_on_in_tile(position, pixels));
        }
    }

    /// Whether `fetch_tile_row_at_once` can stand in for the fetcher's steps over a plain tile
    /// that starts at `start_position` (see `take_tile_steps`): none of the glitches the steps
    /// handle can come up in them. Those are the LCDC tile data area write glitches
    /// (`tile_sel_glitch`, and the CGB's read of the previous unsigned fetch right after a write
    /// selects the $8000 method), STOP cutting the PPU off from VRAM, the CGB window starting
    /// during the fetch (`update_wx_glitch`, on the dots of steps 1, 3 and 5), and the single blank
    /// pixel the DMG pushes when the window was switched off where it would start
    /// (`fetcher_push`).
    #[inline(always)]
    fn can_fetch_tile_row_at_once(&self, start_position: u8) -> bool {
        let lcdc = self.lcdc;
        let pipeline = &self.pipeline;
        if pipeline.tile_sel_glitch || self.stop_blocks.vram {
            return false;
        }
        if self.timing == TimingVariant::Cgb
            && !uses_8000_addressing(self.previous_lcdc)
            && uses_8000_addressing(lcdc)
        {
            return false;
        }
        if self.is_cgb() {
            // With the window active, `update_wx_glitch` sets the glitch where WX matches the
            // position (or, for WX 0, at positions before the screen, which a plain tile never
            // has).
            let window_active = window_enabled(lcdc) && self.window_y_active();
            window_active
                .then_some(self.wx)
                .filter(|&wx| wx != 0)
                .is_none_or(|wx| {
                    [1, 3, 5]
                        .iter()
                        .all(|&dot| start_position.wrapping_add(dot).wrapping_add(WX_OFFSET) != wx)
                })
        } else {
            !(self.window_y_triggered
                && !window_enabled(lcdc)
                && !pipeline.disable_window_pixel_insertion_glitch)
        }
    }

    /// The fetcher's eight steps over a plain tile that starts at `start_position`, taken in one
    /// go when none of their glitches can come up (`can_fetch_tile_row_at_once`): it reads the
    /// next tile number (and on CGB its attributes) and that tile's row, then pushes the row into
    /// the background FIFO as the tile's last pixel leaves it. Leaves everything as
    /// `take_tile_steps` with `move_on_in_tile` would, which the randomized tests check against
    /// running dot by dot.
    #[inline(always)]
    fn fetch_tile_row_at_once(&mut self, start_position: u8) {
        let lcdc = self.lcdc;
        let is_cgb = self.is_cgb();
        if is_cgb {
            self.pipeline.cgb_wx_glitch = false;
        }
        if !window_enabled(lcdc) {
            self.pipeline.wx_triggered = false;
        }
        let y = self.current_fetcher_y();
        let scx = self.scx;
        let caches_fetcher_y = self.caches_fetcher_y();
        let pipeline = &mut self.pipeline;

        // Step 1, a dot into the tile: the tile map address.
        let wx_triggered = pipeline.wx_triggered;
        let high_map = if wx_triggered {
            window_uses_high_tile_map(lcdc)
        } else {
            bg_uses_high_tile_map(lcdc)
        };
        let map = if high_map {
            HIGH_TILE_MAP
        } else {
            LOW_TILE_MAP
        };
        let x = if wx_triggered {
            pipeline.window_tile_x
        } else {
            let position = u32::from(start_position) + 1;
            (((u32::from(scx) + position + 8 - u32::from(is_cgb)) / 8) & 0x1F) as u8
        };
        if caches_fetcher_y {
            pipeline.cached_fetcher_y = y;
        }
        let tile_index_address = map + u16::from(x) + u16::from(y / 8) * 32;
        pipeline.last_tile_index_address = tile_index_address;

        // Step 2: the tile number and attributes.
        let tile_index_address = usize::from(tile_index_address);
        pipeline.current_tile = self.video_ram[tile_index_address];
        if is_cgb {
            pipeline.current_tile_attributes =
                self.video_ram[tile_index_address + usize::from(VRAM_BANK_SIZE)];
        }

        // Steps 3 to 6: the row's two bytes.
        let low_address = self.tile_data_address(0);
        let high_address = self.tile_data_address(1);
        let pipeline = &mut self.pipeline;
        pipeline.last_tile_data_address = high_address;
        let low = self.video_ram[usize::from(low_address)];
        let high = self.video_ram[usize::from(high_address)];
        pipeline.current_tile_data = [low, high];
        pipeline.data_for_sel_glitch = high;
        if pipeline.last_tileset {
            pipeline.last_unsigned_fetch_data = high;
        }
        if wx_triggered {
            pipeline.window_tile_x = (pipeline.window_tile_x + 1) % TILE_MAP_WIDTH;
        }

        // Steps 7 and 8: the push, once the tile's last pixel has left the FIFO.
        let attributes = pipeline.current_tile_attributes;
        pipeline.position_in_line = start_position + FIFO_LENGTH as u8;
        pipeline.bg_fifo.push_row(
            low,
            high,
            attributes & PALETTE_BITS,
            is_bit_set(attributes, PRIORITY_BIT),
            is_bit_set(attributes, X_FLIP_BIT),
        );
        pipeline.fetcher_state = FetcherState::GetTileT1;
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
