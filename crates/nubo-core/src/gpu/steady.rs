//! The PPU's shortcuts, which `Gpu::run_dots` takes so that it doesn't have to run every dot one
//! at a time: steady stretches, in which nothing but the dot count, OAM scan and the renderer
//! changes, run in bulk (`run_steady_dots`), and quiet stretches, in which nothing the rest of the
//! hardware sees can happen, are left for later (`quiet_dots`). Both must give exactly what
//! `Gpu::tick_dot` gives dot by dot, which the randomized tests in `tests/random_scenes.rs` check.

use crate::gpu::pipeline::Pipeline;
use crate::gpu::{
    DOTS_PER_FRAME, DOTS_PER_LINE, DRAWING_START_DOT, Gpu, HBLANK_MODE, LAST_LINE,
    LAST_LINE_ZERO_COMPARE_DOT, LAST_VISIBLE_LINE, LineState, NO_OAM_SCAN_ROW, OAM_SCAN_END_DOT,
    STAT_WRITABLE_BITS, VBLANK_START_DOT, VBLANK_START_LINE, dots_up_to,
};

/// `Gpu::run_steady_dots` only runs dots of lines 0-143 from this one on: every timing event at
/// the start of those lines is over by then (the last are the end of mode 2's interrupt source on
/// DMG after dot 4, and line 0's WY check at dot 5 in double speed). HBlank and VBlank stay steady
/// until the line's last dot, OAM scan until mode 3 starts.
const STEADY_START_DOT: u16 = 5;
/// Like `STEADY_START_DOT`, for lines 144-152, where the last is the end of line 144's mode 2
/// interrupt source after dot 2 (VBlank starts at dot 2 and LY=LYC compares against the new line
/// from dot 2).
const VBLANK_STEADY_START_DOT: u16 = 3;
/// Like `STEADY_START_DOT`, for line 153, where the last is the LY=LYC comparison switching to 0.
const LAST_LINE_STEADY_START_DOT: u16 = LAST_LINE_ZERO_COMPARE_DOT;
/// Mode 3's steady stretches end before this dot, ahead of the window's WY check near the end of
/// the line. Mode 3 is long over by then.
const STEADY_END_DOT: u16 = 451;
/// The last dot of a line.
const LAST_DOT_OF_LINE: u16 = DOTS_PER_LINE - 1;

impl Gpu {
    /// How many of the coming dots the PPU can be left to run later, because nothing the rest of
    /// the hardware sees can come out of them: no interrupt request, no frame and no HBlank
    /// signal for VRAM DMA. The CPU's own accesses to the PPU let it catch up first, so only these
    /// matter. The address bus counts the dots and runs them in one go once there are more.
    /// `settled` says the LY=LYC flag and the STAT line are known to be up to date.
    ///
    /// When no HBlank DMA runs (`hblank_dma_running`) and STAT interrupt requests can't matter,
    /// only VBlank can be seen: the frame and then the VBlank interrupt. STAT requests can't
    /// matter when the STAT line can't rise (see `stat_line_stays_low`), or when IE doesn't enable
    /// the STAT interrupt (`stat_interrupt_enabled`): then only reads of IF see them, and those
    /// let the PPU catch up first, as do writes to IE. Otherwise, while the PPU is steady (see
    /// `has_steady_shape`), nothing can happen before the line ends in HBlank and VBlank, before
    /// the mode 0 interrupt countdown can start in mode 3 (after the second to last pixel), and in
    /// OAM scan before that countdown in the coming mode 3: the dots between OAM scan and mode 3
    /// change only what the CPU sees when it looks.
    pub(super) fn quiet_dots(
        &self,
        settled: bool,
        hblank_dma_running: bool,
        stat_interrupt_enabled: bool,
    ) -> u32 {
        if !self.lcd_enabled() || (self.stopped && !self.is_cgb()) {
            self.idle_dots_until_frame() - 1
        } else if !hblank_dma_running && (!stat_interrupt_enabled || self.stat_line_stays_low()) {
            self.dots_before_vblank_events()
        } else if !(self.has_steady_shape() && (settled || self.stat_settled())) {
            0
        } else {
            match self.line_state {
                LineState::HBlank | LineState::VBlank => {
                    u32::from(DOTS_PER_LINE - 1 - self.line_dot)
                }
                LineState::OamScan => u32::from(
                    DRAWING_START_DOT - self.line_dot + Pipeline::DOTS_BEFORE_LAST_PIXELS_OF_LINE,
                ),
                LineState::Drawing => u32::from(self.pipeline.dots_before_last_pixels()),
            }
        }
    }

    /// Whether the STAT interrupt line stays low whatever the PPU does: no STAT interrupt source is
    /// enabled, as STAT is now, as it was when the line started, or as a CGB STAT write keeps a
    /// disabled source going for a few dots, and no delayed LY=LYC interrupt is due. Only a write
    /// to STAT can change that.
    pub(crate) fn stat_line_stays_low(&self) -> bool {
        (self.stat | self.late_cleared_stat_sources | self.stat_at_line_start) & STAT_WRITABLE_BITS
            == 0
            && self.countdowns.lyc_interrupt == 0
    }

    /// How many dots pass before the next thing VBlank does that the rest of the hardware sees:
    /// the finished frame at the start of line 144, then the VBlank interrupt.
    fn dots_before_vblank_events(&self) -> u32 {
        let now = u32::from(self.line) * u32::from(DOTS_PER_LINE) + u32::from(self.line_dot);
        let vblank_start = u32::from(VBLANK_START_LINE) * u32::from(DOTS_PER_LINE);
        let vblank_interrupt = vblank_start + u32::from(VBLANK_START_DOT);
        let next_event = if now < vblank_start {
            vblank_start
        } else if now < vblank_interrupt {
            vblank_interrupt
        } else {
            DOTS_PER_FRAME + vblank_start
        };
        next_event - now - 1
    }

    /// Whether the coming dots can take a shortcut through `run_steady_dots`, provided the LY=LYC
    /// flag and the STAT line are up to date (`stat_settled`): they are away from the timing
    /// events at the start and end of a line, no countdown runs (`Countdowns`, and the line
    /// state's own ones) and no register write's effect is still on its way: a write to LYC
    /// compared with the next line (`lyc_source_line`), the LY=LYC flag STAT shows a dot late
    /// (`previous_lyc_flag`) and the LCDC some reads see a dot late (`previous_lcdc`). The STAT
    /// interrupt sources then stay as they are until the line state changes, so `tick_dot`'s work
    /// for them would change nothing.
    ///
    /// The one-dot flags `write_timing.rs` sets around a write don't need checking: the bulk paths
    /// handle them as `tick_dot` would. `tile_sel_glitch` goes to the same fetcher steps
    /// (`read_tile_data_low` and `read_tile_data_high`), `tall_objects_glitch` to the same OAM scan
    /// (`scan_oam_entries` checks it), `wx_just_changed` only matters when the window can start,
    /// which keeps mode 3 off the plain pixels (`window_can_start`), and `draw_plain_pixels`
    /// leaves the dot of a split LCDC write (`written_bg_enable`) to `tick_drawing`.
    pub(super) fn has_steady_shape(&self) -> bool {
        let steady_start_dot = match self.line {
            0..VBLANK_START_LINE => STEADY_START_DOT,
            LAST_LINE => LAST_LINE_STEADY_START_DOT,
            _ => VBLANK_STEADY_START_DOT,
        };
        if self.line_dot < steady_start_dot {
            return false;
        }
        let steady_line_state = match self.line_state {
            LineState::HBlank => {
                self.line_dot < LAST_DOT_OF_LINE
                    && self.visible_mode == HBLANK_MODE
                    && self.mode_0_interrupt_delay == 0
                    && self.hdma_start_delay == 0
            }
            LineState::VBlank => self.line_dot < LAST_DOT_OF_LINE,
            LineState::Drawing => self.line_dot < STEADY_END_DOT,
            LineState::OamScan => {
                !self.first_line_after_lcd_on && self.line_dot < DRAWING_START_DOT
            }
        };
        steady_line_state
            && self.countdowns.is_idle()
            && self.lyc_source_line.is_none()
            && self.previous_lyc_flag == self.lyc_flag
            && self.previous_lcdc == self.lcdc
    }

    /// Whether the LY=LYC flag and the STAT line are what this dot's `tick_dot` would make them.
    /// Every dot leaves them so, but register writes between dots may not.
    pub(super) fn stat_settled(&self) -> bool {
        self.lyc_flag == (self.ly_for_comparison() == Some(self.compared_lyc))
            && self.stat_line_high
                == self.stat_line_level(self.stat | self.late_cleared_stat_sources)
    }

    /// Runs up to `dots` dots of a steady stretch (see `has_steady_shape`) and returns how many it
    /// ran: HBlank and VBlank only move `line_dot` on (and check WY for the window near the end of
    /// the line), OAM scan checks an entry every two dots and then starts mode 3, and mode 3 runs
    /// the renderer. It stops where the stretch ends, or after the dot in which drawing ends or the
    /// mode 0 interrupt countdown starts.
    pub(super) fn run_steady_dots(&mut self, dots: u32) -> u32 {
        match self.line_state {
            LineState::HBlank | LineState::VBlank => {
                let count = dots_up_to(dots, LAST_DOT_OF_LINE - self.line_dot);
                let end_dot = self.line_dot + count;
                let (_, _, window_y_check_dot) = self.window_y_check_dots();
                if self.line < LAST_VISIBLE_LINE
                    && (self.line_dot + 1..=end_dot).contains(&window_y_check_dot)
                {
                    self.update_window_y_trigger();
                }
                self.line_dot = end_dot;
                u32::from(count)
            }
            LineState::OamScan => {
                let count = dots_up_to(dots, DRAWING_START_DOT - self.line_dot);
                let end_dot = self.line_dot + count;
                // Every second dot up to `OAM_SCAN_END_DOT` checks the next OAM entry.
                let scan_end_dot = end_dot.min(OAM_SCAN_END_DOT);
                let first_entry_dot = self.line_dot / 2 * 2 + 2;
                if first_entry_dot <= scan_end_dot {
                    self.scan_oam_entries(first_entry_dot / 2 - 1..scan_end_dot / 2);
                    let last_entry_dot = scan_end_dot / 2 * 2;
                    self.oam_scan_row = if last_entry_dot < OAM_SCAN_END_DOT {
                        (last_entry_dot * 2) as u8
                    } else {
                        NO_OAM_SCAN_ROW
                    };
                }
                self.line_dot = self.line_dot.max(scan_end_dot);
                while self.line_dot < end_dot {
                    // The dots up to mode 3, which changes only what STAT shows and starts at
                    // the last of them.
                    self.line_dot += 1;
                    self.tick_oam_scan();
                }
                u32::from(count)
            }
            LineState::Drawing => {
                let count = dots.min(u32::from(STEADY_END_DOT - self.line_dot));
                let mut ran = 0;
                while ran < count {
                    if self.fetcher_start_delay == 0 && !self.window_can_start() {
                        ran += self.draw_plain_pixels(count - ran);
                        if ran == count {
                            break;
                        }
                    }
                    ran += 1;
                    self.line_dot += 1;
                    self.tick_mode_3();
                    if self.line_state != LineState::Drawing {
                        self.update_lyc_flag();
                        self.update_stat_line();
                        return ran;
                    }
                    if self.countdowns.early_hblank != 0 {
                        return ran;
                    }
                }
                count
            }
        }
    }
}
