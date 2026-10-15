//! Nubo's shortcuts through mode 3, which aren't part of the SameBoy port in `pipeline.rs`: runs
//! of plain dots, in which the renderer only shifts out a pixel, draws it and takes a fetcher
//! step, are drawn without working through `Gpu::tick_drawing`'s phases, and a whole tile row
//! without objects at once, usually in runs of such tiles with the fetcher's eight steps for each
//! next row taken in one go (`draw_background_tiles`). They must give exactly what `tick_drawing` gives dot by dot,
//! which the randomized tests in `gpu/tests/random_scenes.rs` check.

use super::{
    BackgroundFifo, FIFO_LENGTH, FRAME_BUFFER_BOUNDS, FetcherState, HIGH_TILE_MAP, LOW_TILE_MAP,
    OBJECT_X_OFFSET, PALETTE_BITS, PRIORITY_BIT, Phase, PixelColors, SIGNED_TILE_DATA_CENTER,
    TILE_BYTES, TILE_MAP_WIDTH, VRAM_BANK_BIT, VRAM_BANK_SIZE, WX_OFFSET, X_FLIP_BIT, Y_FLIP_BIT,
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
                let max_tiles = (dots - ran) / FIFO_LENGTH as u32;
                let tiles = self.draw_background_tiles(
                    colors,
                    cgb_rendering || bg_window_enabled,
                    max_tiles,
                );
                ran += tiles * FIFO_LENGTH as u32;
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

    /// Plain tiles (see `can_draw_plain_tile`) without object pixels, one after the other, up to
    /// `max_tiles` of them, and returns how many it drew: always the first, and the next ones for
    /// as long as they are plain too. Each tile is the eight pixels of the background row, with the
    /// colors from `colors` (only color 0 unless `bg_enabled`), and the fetcher's eight steps,
    /// which read the next row and push it as the tile's last pixel leaves the FIFO.
    ///
    /// When none of the fetcher's glitches can come up (`can_fetch_tile_row_at_once`), nothing
    /// but the position changes from one tile to the next while the PPU catches up, so the run
    /// keeps what the steps would work out again for every tile (the tile map row, the tile data
    /// area, the y in the tile) at hand and leaves the fetcher's registers as its last tile's
    /// steps would. Otherwise the tile takes the steps one by one (`draw_background_tile`). Both
    /// give what `tick_drawing` gives dot by dot.
    /// Always inlined so that `colors` is a constant in it (see `PixelColors`).
    #[inline(always)]
    fn draw_background_tiles(
        &mut self,
        colors: PixelColors,
        bg_enabled: bool,
        max_tiles: u32,
    ) -> u32 {
        let start_position = self.pipeline.position_in_line;
        if !self.can_fetch_tile_row_at_once(start_position) {
            self.draw_background_tile(colors, bg_enabled);
            return 1;
        }

        // What the first step of the first tile does before the run settles: on CGB the window
        // can't start during it (`update_wx_glitch`), and a disabled window stops being fetched.
        let lcdc = self.lcdc;
        let is_cgb = self.is_cgb();
        if is_cgb {
            self.pipeline.cgb_wx_glitch = false;
        }
        if !window_enabled(lcdc) {
            self.pipeline.wx_triggered = false;
        }
        let wx_triggered = self.pipeline.wx_triggered;
        let y = self.current_fetcher_y();
        if self.caches_fetcher_y() {
            self.pipeline.cached_fetcher_y = y;
        }
        let high_map = if wx_triggered {
            window_uses_high_tile_map(lcdc)
        } else {
            bg_uses_high_tile_map(lcdc)
        };
        let map_row = if high_map {
            HIGH_TILE_MAP
        } else {
            LOW_TILE_MAP
        } + u16::from(y / 8) * 32;
        let unsigned_tile_data = uses_8000_addressing(lcdc);
        let scx = self.scx;
        let wx = self.wx;
        let cgb_window_active = is_cgb && window_enabled(lcdc) && self.window_y_active();
        let next_object_x = if self.pipeline.visible_object_count == 0 {
            None
        } else {
            Some(self.pipeline.visible_object_x[self.pipeline.next_object()])
        };
        // Whether a tile that starts at `position` after the first is plain too, and its fetch
        // free of glitches: the same as `can_draw_plain_tile` and `can_fetch_tile_row_at_once`
        // with what can't change within the run left out.
        let next_tile_is_plain = |position: u8| {
            let last_position = position + FIFO_LENGTH as u8 - 1;
            position < GB_SCREEN_WIDTH as u8 - 2 - (FIFO_LENGTH as u8 - 1)
                && !(wx_triggered
                    && (position + WX_OFFSET..=last_position + WX_OFFSET).contains(&wx))
                && next_object_x.is_none_or(|x| x > last_position + OBJECT_X_OFFSET)
                && !(cgb_window_active
                    && wx != 0
                    && [1, 3, 5]
                        .iter()
                        .any(|&dot| position + dot + WX_OFFSET == wx))
        };

        let first_pixel =
            usize::from(self.ly) * GB_SCREEN_WIDTH as usize + usize::from(self.pipeline.lcd_x);
        let mut row = self.pipeline.bg_fifo;
        let mut position = start_position;
        let mut tiles = 0;
        let mut window_tile_x = self.pipeline.window_tile_x;
        let mut attributes = self.pipeline.current_tile_attributes;
        let (tile_index_address, tile, high_address, row_bytes) = loop {
            self.draw_tile_row(colors, bg_enabled, row, first_pixel + 8 * tiles);

            // Steps 1 and 2, the first a dot into the tile: the tile number, and on CGB its
            // attributes.
            let map_x = if wx_triggered {
                window_tile_x
            } else {
                let fetch_position = u32::from(position) + 1;
                (((u32::from(scx) + fetch_position + 8 - u32::from(is_cgb)) / 8) & 0x1F) as u8
            };
            let tile_index_address = map_row + u16::from(map_x);
            let tile = self.video_ram[usize::from(tile_index_address)];
            if is_cgb {
                attributes = self.video_ram[usize::from(tile_index_address + VRAM_BANK_SIZE)];
            }

            // Steps 3 to 6: the row's two bytes (see `tile_data_address`).
            let mut tile_address = if unsigned_tile_data {
                u16::from(tile) * TILE_BYTES
            } else {
                SIGNED_TILE_DATA_CENTER
                    .wrapping_add_signed(i16::from(tile.cast_signed()) * TILE_BYTES.cast_signed())
            };
            if is_bit_set(attributes, VRAM_BANK_BIT) {
                tile_address += VRAM_BANK_SIZE;
            }
            let y_flip = if is_bit_set(attributes, Y_FLIP_BIT) {
                7
            } else {
                0
            };
            let low_address = tile_address + u16::from((y & 7) ^ y_flip) * 2;
            let high_address = low_address + 1;
            let row_bytes = [
                self.video_ram[usize::from(low_address)],
                self.video_ram[usize::from(high_address)],
            ];
            if wx_triggered {
                window_tile_x = (window_tile_x + 1) % TILE_MAP_WIDTH;
            }

            // Steps 7 and 8: the push, once the tile's last pixel has left the FIFO.
            row.push_row(
                row_bytes[0],
                row_bytes[1],
                attributes & PALETTE_BITS,
                is_bit_set(attributes, PRIORITY_BIT),
                is_bit_set(attributes, X_FLIP_BIT),
            );
            position += FIFO_LENGTH as u8;
            tiles += 1;
            if tiles == max_tiles as usize || !next_tile_is_plain(position) {
                break (tile_index_address, tile, high_address, row_bytes);
            }
        };

        let dots = (FIFO_LENGTH * tiles) as u8;
        self.line_dot += u16::from(dots);
        let pipeline = &mut self.pipeline;
        pipeline.lcd_x += dots;
        pipeline.position_in_line = position;
        pipeline.during_object_fetch = false;
        pipeline.window_is_being_fetched = false;
        pipeline.window_start_backup = None;
        pipeline.last_tile_index_address = tile_index_address;
        pipeline.current_tile = tile;
        pipeline.current_tile_attributes = attributes;
        pipeline.last_tileset = unsigned_tile_data;
        pipeline.last_tile_data_address = high_address;
        pipeline.current_tile_data = row_bytes;
        pipeline.data_for_sel_glitch = row_bytes[1];
        if unsigned_tile_data {
            pipeline.last_unsigned_fetch_data = row_bytes[1];
        }
        pipeline.window_tile_x = window_tile_x;
        pipeline.bg_fifo = row;
        pipeline.fetcher_state = FetcherState::GetTileT1;
        tiles as u32
    }

    /// A plain tile (see `can_draw_plain_tile`) without object pixels whose fetcher steps are
    /// taken one by one: the eight pixels of the background row, with the colors from `colors`
    /// (only color 0 unless `bg_enabled`), and the fetcher's steps, each seeing the position and
    /// background FIFO it would see on its own dot. The same as eight dots of `draw_plain_pixel`
    /// and a fetcher step, without working out each pixel's color on its own.
    /// Always inlined so that `colors` is a constant in it (see `PixelColors`).
    #[inline(always)]
    fn draw_background_tile(&mut self, colors: PixelColors, bg_enabled: bool) {
        let first_pixel =
            usize::from(self.ly) * GB_SCREEN_WIDTH as usize + usize::from(self.pipeline.lcd_x);
        self.draw_tile_row(colors, bg_enabled, self.pipeline.bg_fifo, first_pixel);

        self.line_dot += FIFO_LENGTH as u16;
        let pipeline = &mut self.pipeline;
        pipeline.lcd_x += FIFO_LENGTH as u8;
        pipeline.during_object_fetch = false;
        pipeline.window_is_being_fetched = false;
        pipeline.window_start_backup = None;
        let position = pipeline.position_in_line;
        self.take_tile_steps(|gpu, pixels| gpu.move_on_in_tile(position, pixels));
    }

    /// Draws the eight pixels of the background row `row` into the frame buffer from
    /// `first_pixel` on, with the colors from `colors` (only color 0 unless `bg_enabled`).
    /// Always inlined so that `colors` is a constant in it (see `PixelColors`).
    #[inline(always)]
    fn draw_tile_row(
        &mut self,
        colors: PixelColors,
        bg_enabled: bool,
        row: BackgroundFifo,
        first_pixel: usize,
    ) {
        let row_color = |color_id: u8| {
            self.background_color_in(colors, row.palette, if bg_enabled { color_id } else { 0 })
        };
        let row_colors = [row_color(0), row_color(1), row_color(2), row_color(3)];
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
    }

    /// Whether `draw_background_tiles` can take the fetcher's steps over a plain tile that starts
    /// at `start_position` in one go instead of one by one (`take_tile_steps`): none of the
    /// glitches the steps handle can come up in them. Those are the LCDC tile data area write glitches
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
