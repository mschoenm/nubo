//! The PPU's shortcuts, which `Gpu::run_dots` takes so that it doesn't have to run every dot one
//! at a time: steady stretches, in which nothing but the dot count, OAM scan and the renderer
//! changes, run in bulk (`run_steady_dots`), and so do a line's first dots and HBlank's first dots
//! while the STAT line can't rise (`run_line_start`, `run_hblank_start`), which a line would
//! otherwise run one at a time for their timing events. Quiet stretches, in which nothing the rest
//! of the hardware sees can happen, are left for later (`quiet_dots`). All must give exactly what
//! `Gpu::tick_dot` gives dot by dot, which the randomized tests in `tests/random_scenes.rs` check.

use crate::address_bus::hdma::HdmaState;
use crate::gpu::pipeline::Pipeline;
use crate::gpu::{
    Countdowns, DOTS_PER_FRAME, DOTS_PER_LINE, DOUBLE_SPEED_MODE_3_STAT_DOT,
    DOUBLE_SPEED_PALETTE_BLOCK_DOT, DOUBLE_SPEED_STAT_SAMPLE_DOT, DRAWING_START_DOT, Gpu,
    HBLANK_MODE, LAST_LINE, LAST_LINE_ZERO_COMPARE_DOT, LAST_VISIBLE_LINE, LineState,
    MODE_2_STAT_DOT, MODE_3_STAT_DOT, NO_OAM_SCAN_ROW, OAM_MODE, OAM_SCAN_END_DOT,
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

// `Gpu::run_line_start` handles the events of a line's first dots, up to `STEADY_START_DOT`, but
// not the ones that come later in OAM scan.
const _: () = assert!(
    STEADY_START_DOT < MODE_3_STAT_DOT
        && STEADY_START_DOT < DOUBLE_SPEED_MODE_3_STAT_DOT
        && STEADY_START_DOT < DOUBLE_SPEED_PALETTE_BLOCK_DOT
        && STEADY_START_DOT < OAM_SCAN_END_DOT
);

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

    /// Whether the coming dots are the start of one of lines 1-143, from the last dot of the line
    /// before up to the steady stretch of OAM scan (`STEADY_START_DOT`), that `run_line_start` can
    /// take in one go: the STAT line stays low (`stat_line_stays_low`), so it can't request an
    /// interrupt, and nothing else is on its way, as `has_steady_shape` asks of a steady stretch.
    pub(super) fn has_quiet_line_start(&self) -> bool {
        let at_line_start = match self.line_state {
            LineState::HBlank => {
                self.line_dot == LAST_DOT_OF_LINE && self.line + 1 < VBLANK_START_LINE
            }
            LineState::OamScan => {
                self.line_dot < STEADY_START_DOT && (1..VBLANK_START_LINE).contains(&self.line)
            }
            LineState::Drawing | LineState::VBlank => false,
        };
        at_line_start
            && !self.first_line_after_lcd_on
            && self.mode_0_interrupt_delay == 0
            && self.hdma_start_delay == 0
            && self.countdowns.is_idle()
            && self.lyc_source_line.is_none()
            && self.previous_lcdc == self.lcdc
            && self.stat_line_stays_low()
    }

    /// Runs up to `dots` dots of a line's start (see `has_quiet_line_start`) and returns how many
    /// it ran, with the effects `tick_dot` has at each of those dots: the new line, STAT's mode 2,
    /// the WY check and the OAM entries checked every two dots, a double speed CPU's look at STAT,
    /// and the LY=LYC flag, which only the last two dots decide. The STAT line stays low.
    pub(super) fn run_line_start(&mut self, dots: u32) -> u32 {
        let first_dot = if self.line_state == LineState::HBlank {
            0
        } else {
            self.line_dot + 1
        };
        let count = dots_up_to(dots, STEADY_START_DOT + 1 - first_dot);
        let last_dot = first_dot + count - 1;
        if self.line_state == LineState::HBlank {
            self.start_next_line();
        }
        let runs_through = |dot: u16| (first_dot..=last_dot).contains(&dot);

        if self.in_double_speed() && runs_through(DOUBLE_SPEED_STAT_SAMPLE_DOT) {
            self.stat_at_line_start = self.stat;
        }
        let (_, window_y_check_dot, _) = self.window_y_check_dots();
        if runs_through(window_y_check_dot) {
            self.update_window_y_trigger();
        }
        if runs_through(MODE_2_STAT_DOT) {
            self.visible_mode = OAM_MODE;
        }
        // Every second dot from dot 2 on checks the next OAM entry; each even dot from dot 0 on
        // sets the row OAM scan reads.
        let last_entry_dot = last_dot / 2 * 2;
        let first_entry_dot = first_dot.max(1).next_multiple_of(2);
        if first_entry_dot <= last_entry_dot {
            self.scan_oam_entries(first_entry_dot / 2 - 1..last_entry_dot / 2);
        }
        if last_entry_dot >= first_dot {
            self.oam_scan_row = (last_entry_dot * 2) as u8;
        }

        let lyc_flag_at = |gpu: &mut Self, dot: u16| {
            gpu.line_dot = dot;
            gpu.ly_for_comparison() == Some(gpu.compared_lyc)
        };
        if count > 1 {
            self.previous_lyc_flag = lyc_flag_at(self, last_dot - 1);
        } else {
            self.previous_lyc_flag = self.lyc_flag;
        }
        self.lyc_flag = lyc_flag_at(self, last_dot);
        self.stat_line_high = false;
        u32::from(count)
    }

    /// Whether the coming dots are the start of HBlank, while the countdowns that start with it
    /// (`mode_0_interrupt_delay`, `hdma_start_delay`, and the mode 0 interrupt source and palette
    /// RAM unblocking in `Countdowns`) still run, that `run_hblank_start` can take in one go: the
    /// STAT line stays low, as `has_quiet_line_start` asks, and no other countdown runs.
    pub(super) fn has_quiet_hblank_start(&self) -> bool {
        self.line_state == LineState::HBlank
            && self.line_dot < LAST_DOT_OF_LINE
            && Countdowns {
                early_hblank: 0,
                palette_unblock: 0,
                ..self.countdowns
            }
            .is_idle()
            && self.lyc_source_line.is_none()
            && self.previous_lcdc == self.lcdc
            && self.stat_line_stays_low()
    }

    /// Runs up to `dots` dots of HBlank's start (see `has_quiet_hblank_start`), until its
    /// countdowns have run out, and returns how many it ran, with the effects `tick_dot` has at
    /// each of those dots: STAT shows mode 0, the countdowns count down, `hdma` hears that HBlank
    /// started, and palette RAM is unblocked. The STAT line stays low.
    pub(super) fn run_hblank_start(&mut self, dots: u32, hdma: &mut HdmaState) -> u32 {
        let countdowns_left = self
            .mode_0_interrupt_delay
            .max(self.hdma_start_delay)
            .max(self.countdowns.early_hblank)
            .max(self.countdowns.palette_unblock)
            .max(1);
        let count = dots_up_to(
            dots,
            u16::from(countdowns_left).min(LAST_DOT_OF_LINE - self.line_dot),
        );
        let first_dot = self.line_dot + 1;
        let last_dot = self.line_dot + count;
        // Whether a countdown at `left` runs out within these dots, after which it is idle.
        let count_down_through = |left: &mut u8| {
            let runs_out = *left != 0 && u16::from(*left) <= count;
            *left = u8::try_from(u16::from(*left).saturating_sub(count)).unwrap_or(0);
            runs_out
        };

        if count_down_through(&mut self.countdowns.early_hblank) {
            self.early_hblank_interrupt = true;
        }
        if count_down_through(&mut self.countdowns.palette_unblock) {
            self.palette_ram_blocked = false;
        }
        let (_, _, window_y_check_dot) = self.window_y_check_dots();
        if self.line < LAST_VISIBLE_LINE && (first_dot..=last_dot).contains(&window_y_check_dot) {
            self.update_window_y_trigger();
        }
        self.visible_mode = HBLANK_MODE;
        count_down_through(&mut self.mode_0_interrupt_delay);
        if count_down_through(&mut self.hdma_start_delay) {
            hdma.set_hblank_started(true);
        }

        if count > 1 {
            self.line_dot = last_dot - 1;
            self.update_lyc_flag();
        }
        self.previous_lyc_flag = self.lyc_flag;
        self.line_dot = last_dot;
        self.update_lyc_flag();
        self.stat_line_high = false;
        u32::from(count)
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
