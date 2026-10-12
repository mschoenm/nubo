//! STAT (0xFF41) and LY=LYC: what STAT and LY read, the LY=LYC comparison and the STAT interrupt
//! line with its four sources (mode 0, mode 1, mode 2 and LY=LYC), and the late effects of CGB
//! STAT and LYC writes. The dots are measured against Gambatte and SameBoy; each constant says
//! which tests it was fitted to.

use crate::gpu::{
    DOTS_PER_LINE, Gpu, HBLANK_MODE, HBLANK_MODE_STAT_SOURCE_BIT, LAST_LINE,
    LAST_LINE_ZERO_COMPARE_DOT, LINE_0_OAM_SOURCE_DOT, LYC_STAT_SOURCE_BIT, LineState,
    OAM_MODE_STAT_SOURCE_BIT, STAT_WRITABLE_BITS, TimingVariant, VBLANK_MODE,
    VBLANK_MODE_STAT_SOURCE_BIT, VBLANK_START_DOT, VBLANK_START_LINE, count_down,
};
use crate::model::Model;
use crate::utils::is_bit_set;

/// How many dots late a CGB LYC write at normal speed reaches the LY=LYC comparison, which picks
/// LYC up every four dots of the line: four dots for a write in step with the LCD, fewer when the
/// LCD runs out of step with the CPU (Gambatte's lycEnable lycwirq_trigger_ly00_stat50_lcdoffset1
/// tests). Like `CGB_LYC_INTERRUPT_DELAY_DOTS` and `CGB_STAT_CLEAR_DELAY_DOTS`, fitted to
/// Gambatte's lycEnable, m0enable and miscmstatirq tests, whose DMG and CGB results differ because
/// these writes take effect later on CGB.
const CGB_LYC_WRITE_DELAY_DOTS: u8 = 4;
/// LY=LYC compares against the new line from this dot of lines 1-152, after a gap in which it
/// compares against nothing. In double speed dot 0 still compares against the previous line.
const LYC_COMPARE_DOT: u16 = 2;
/// The last dot of lines 1-143 at which the mode 2 interrupt source is active: on DMG for the
/// length of the old mode change (SameBoy; GBMicrotest), on CGB at both speeds only as long as a
/// STAT write enabling it requests an interrupt (measured against Gambatte).
const DMG_OAM_SOURCE_LAST_DOT: u16 = 4;
const CGB_OAM_SOURCE_LAST_DOT: u16 = 2;
/// The previous line's mode 0 (hblank) STAT interrupt source stays active until this dot of the
/// next line on DMG, one dot before LY=LYC compares against the new line, as in SameBoy. On CGB it
/// ends as the line starts, at both speeds, even though STAT shows mode 0 for another dot in
/// double speed (measured against Gambatte; Gambatte's miscmstatirq lycstatwirq_trigger_m0 and
/// m0enable late_enable_ds tests).
const DMG_HBLANK_SOURCE_END_DOT: u16 = 1;
/// A delayed CGB LYC write that starts a match requests its interrupt this many dots after it
/// reaches the comparison, too late for the CPU to see it in the M-cycle where the comparison
/// changes (measured against Gambatte; Gambatte's lycEnable lyc_ff45_trigger_delay tests).
const CGB_LYC_INTERRUPT_DELAY_DOTS: u8 = 1;
/// The STAT interrupt sources a CGB STAT write at normal speed keeps active for
/// `CGB_STAT_CLEAR_DELAY_DOTS` more dots when it disables them (see
/// `Gpu::cgb_late_cleared_stat_sources`).
const CGB_LATE_CLEARED_STAT_SOURCES: u8 =
    (1 << LYC_STAT_SOURCE_BIT) | (1 << HBLANK_MODE_STAT_SOURCE_BIT);
/// On CGB at normal speed an LYC write in the last dots of a line that moves a match on to the
/// next line keeps the LY=LYC interrupt source active through the change of line, so no new
/// interrupt is requested (measured against Gambatte; Gambatte's lycEnable lyc153_late_ff45_enable
/// and ff45_enable_weirdpoint tests).
const CGB_LYC_NEXT_LINE_DOTS: u16 = 4;
/// An LYC write that ends an LY=LYC match leaves the interrupt source active for this many more
/// dots in double speed and on DMG, so it still blocks a mode 2 or mode 0 interrupt due right
/// after it (measured against Gambatte; Gambatte's m2enable lyc1_m2irq_late_lyc255 and m0enable
/// lycdisable_ff45 tests).
const DOUBLE_SPEED_LYC_SOURCE_HOLD_DOTS: u8 = 2;
const DMG_LYC_SOURCE_HOLD_DOTS: u8 = 1;
/// On CGB at normal speed the source stays this many dots more after a delayed LYC write reaches
/// the comparison, six dots after the write (measured against Gambatte; Gambatte's m0enable
/// lycdisable_ff45 tests).
const CGB_LYC_SOURCE_HOLD_DOTS: u8 = 2;
/// An LYC write in this many last dots of a line is compared with the next line for interrupts,
/// in double speed and on DMG (measured against Gambatte; Gambatte's lycEnable late_ff45_enable
/// tests). STAT's flag still compares it with the current line.
const DOUBLE_SPEED_LYC_NEXT_LINE_DOTS: u16 = 3;
const DMG_LYC_NEXT_LINE_DOTS: u16 = 2;
/// The mode 0 interrupt source starts this many dots after the second to last pixel of a line is
/// drawn, even when an object or window fetch for the last pixel keeps mode 3 going longer: STAT
/// still shows mode 3 until the line is done (Gambatte's 10spritesPrLine_10xposA7, m2int_wxA6 and
/// enable_wxA6_2x_spxA7 tests). Without such a fetch this is the same dot as
/// `MODE_0_INTERRUPT_DELAY_DOTS` after mode 3.
const EARLY_HBLANK_INTERRUPT_DOTS: u8 = 2;
/// The dot of line 153 at which LY=LYC stops comparing against 153, a dot later in double speed.
/// It then compares against nothing until `LAST_LINE_ZERO_COMPARE_DOT`, from which it compares
/// against 0 at both speeds (Gambatte's ly0 and lyc153 tests).
const LAST_LINE_COMPARE_END_DOT: u16 = 8;
const DOUBLE_SPEED_LAST_LINE_COMPARE_END_DOT: u16 = 9;
/// On line 153, LY already reads 0 from this dot.
const LAST_LINE_LY_RESET_DOT: u16 = 4;
const LATE_LAST_LINE_LY_RESET_DOT: u16 = 5;
const DOUBLE_SPEED_LAST_LINE_LY_RESET_DOT: u16 = 5;

impl Gpu {
    /// The mode shown in STAT bits 0-1. It reads 0 while the LCD is off, and on DMG and CGB-C
    /// and older at normal speed also in the first dots of line 0, though the vblank interrupt
    /// source lasts until mode 2 (SameBoy; GBMicrotest's poweron_stat tests, AGE's stat-mode
    /// tests).
    pub(crate) fn mode(&self) -> u8 {
        let shows_hblank_at_line_0 = self.model <= Model::CgbC && !self.in_double_speed();
        if !self.lcd_enabled()
            || (shows_hblank_at_line_0 && self.line == 0 && self.visible_mode == VBLANK_MODE)
        {
            HBLANK_MODE
        } else {
            self.visible_mode
        }
    }

    /// Reads STAT: bit 7 always set, the enabled interrupt sources (bits 3-6), the LY=LYC flag
    /// (bit 2) and the mode (bits 0-1).
    pub(super) fn stat_value(&self) -> u8 {
        let shown_flag = if self.model > Model::CgbC && self.lcd_enabled() {
            self.previous_lyc_flag
        } else {
            self.lyc_flag
        };
        let lyc_flag = if shown_flag { 0b100 } else { 0 };
        0x80 | (self.stat & STAT_WRITABLE_BITS) | lyc_flag | self.mode()
    }

    /// The line LYC is compared against at this dot, or `None` during the short gaps where no
    /// value matches, such as the first dots of a line.
    pub(super) fn ly_for_comparison(&self) -> Option<u8> {
        match (self.line, self.line_dot) {
            (0, _) => Some(0),
            (LAST_LINE, dot) => {
                let compare_end_dot = if self.in_double_speed() {
                    DOUBLE_SPEED_LAST_LINE_COMPARE_END_DOT
                } else {
                    LAST_LINE_COMPARE_END_DOT
                };
                if dot < LYC_COMPARE_DOT {
                    self.in_double_speed().then_some(LAST_LINE - 1)
                } else if dot < compare_end_dot {
                    Some(LAST_LINE)
                } else if dot < LAST_LINE_ZERO_COMPARE_DOT {
                    None
                } else {
                    Some(0)
                }
            }
            (line, 0) if self.in_double_speed() => Some(line - 1),
            (_, dot) if dot < LYC_COMPARE_DOT => None,
            (line, _) => Some(line),
        }
    }

    /// Reads LY. As a CGB's LY counts up at the end of a line, its bits change one after the
    /// other, so for a moment it reads `LY & (LY + 1)`: in the last dot of the line, and at normal
    /// speed on CGB-C and older also in the dot before (AGE's lcd-align-ly tests; Gambatte's
    /// lcd_offset ly_count tests).
    pub(super) fn ly_value(&self) -> u8 {
        let glitch_start_dot = if !self.in_double_speed() && self.model <= Model::CgbC {
            DOTS_PER_LINE - 2
        } else {
            DOTS_PER_LINE - 1
        };
        if self.is_cgb()
            && self.lcd_enabled()
            && self.line < LAST_LINE
            && self.line_dot >= glitch_start_dot
        {
            self.ly & self.ly.wrapping_add(1)
        } else {
            self.ly
        }
    }

    /// The dot of line 153 from which LY reads 0: a dot later on CGB-D and newer (AGE's ly and
    /// lcd-align-ly tests) and in double speed (Gambatte's ly0 _ds tests).
    pub(super) fn last_line_ly_reset_dot(&self) -> u16 {
        if self.in_double_speed() {
            DOUBLE_SPEED_LAST_LINE_LY_RESET_DOT
        } else if self.model > Model::CgbC {
            LATE_LAST_LINE_LY_RESET_DOT
        } else {
            LAST_LINE_LY_RESET_DOT
        }
    }

    /// Recomputes the LY=LYC flag. While the LCD is off the flag keeps its last value.
    pub(super) fn update_lyc_flag(&mut self) {
        if self.lyc_source_line.is_some() && self.lyc_source_line == self.ly_for_comparison() {
            self.lyc_source_line = None;
        }
        if self.lcd_enabled() {
            self.lyc_flag = self.ly_for_comparison() == Some(self.compared_lyc);
        }
    }

    /// Whether the LY=LYC interrupt source sees a match, which can differ from STAT's flag after
    /// a late LYC write and at a DMG line's first dot.
    fn lyc_source_match(&self) -> bool {
        match self.lyc_source_line {
            Some(line) => line == self.compared_lyc,
            // On DMG the previous line's match still holds the interrupt line at dot 0, while
            // STAT's flag already reads clear (measured against Gambatte; Gambatte's m1 lyc143
            // and m2enable lycint tests).
            None if !self.is_cgb() && self.line > 0 && self.line_dot == 0 => {
                self.compared_lyc == self.line - 1
            }
            None => self.lyc_flag,
        }
    }

    /// The level the STAT interrupt line would have if only the sources selected in
    /// `enabled_sources` (STAT bits 3-6) were enabled.
    pub(super) fn stat_line_level(&self, enabled_sources: u8) -> bool {
        let lyc_source = is_bit_set(enabled_sources, LYC_STAT_SOURCE_BIT)
            && (self.lyc_source_match() || self.countdowns.lyc_source_hold > 0);
        if !self.lcd_enabled() {
            return lyc_source;
        }

        lyc_source
            || (is_bit_set(enabled_sources, HBLANK_MODE_STAT_SOURCE_BIT)
                && self.hblank_source_active())
            || (is_bit_set(enabled_sources, VBLANK_MODE_STAT_SOURCE_BIT)
                && self.vblank_source_active())
            || ((is_bit_set(enabled_sources, OAM_MODE_STAT_SOURCE_BIT)
                || self.oam_source_kept_from_line_start())
                && self.oam_source_active())
    }

    /// Whether the mode 0 (HBlank) interrupt source is active. It can start before mode 3 ends
    /// (`EARLY_HBLANK_INTERRUPT_DOTS`), and then stays active through the end of mode 3 into
    /// HBlank instead of waiting for `mode_0_interrupt_delay` again, which would drop it for a
    /// dot and request a second interrupt. In the first dots of lines 1-143 the previous line's
    /// source ends at a fixed dot, whatever STAT shows. The first line after
    /// LCD on shows mode 0 during its would-be OAM scan, but that isn't an HBlank and doesn't
    /// request mode 0 interrupts (SameBoy).
    pub(crate) fn hblank_source_active(&self) -> bool {
        if self.line_state == LineState::OamScan && (1..VBLANK_START_LINE).contains(&self.line) {
            return !self.is_cgb() && self.line_dot < DMG_HBLANK_SOURCE_END_DOT;
        }
        if self.is_cgb() && self.line == VBLANK_START_LINE {
            // STAT shows mode 0 in line 144's first dots, but on CGB only a source that was
            // already enabled counts (Gambatte's m1 ly143_late_m0enable and m1irq_m0disable
            // tests).
            return self.visible_mode == HBLANK_MODE
                && self.enabled_at_line_start(HBLANK_MODE_STAT_SOURCE_BIT);
        }
        (self.early_hblank_interrupt
            && matches!(self.line_state, LineState::Drawing | LineState::HBlank))
            || (self.visible_mode == HBLANK_MODE
                && self.mode_0_interrupt_delay == 0
                && !(self.first_line_after_lcd_on && self.line_state == LineState::OamScan))
    }

    /// Called when the second to last pixel of a line is drawn; see
    /// `EARLY_HBLANK_INTERRUPT_DOTS`.
    pub(crate) fn start_hblank_interrupt_countdown(&mut self) {
        self.countdowns.early_hblank = EARLY_HBLANK_INTERRUPT_DOTS;
    }

    /// Whether the mode 1 (VBlank) interrupt source is active. On CGB at normal speed STAT still
    /// shows mode 1 at dot 0 of line 0, but only a source that was already enabled counts there
    /// (Gambatte's m1irq_late_enable tests).
    fn vblank_source_active(&self) -> bool {
        self.visible_mode == VBLANK_MODE
            && !(self.timing == TimingVariant::Cgb
                && self.line == 0
                && !self.enabled_at_line_start(VBLANK_MODE_STAT_SOURCE_BIT))
    }

    /// Whether the STAT interrupt source `bit` was enabled when the current line started.
    fn enabled_at_line_start(&self, bit: u8) -> bool {
        is_bit_set(self.stat_at_line_start, bit)
    }

    /// Whether the mode 2 (OAM scan) interrupt source is active. It is only a pulse as a line
    /// starts, not a level that lasts through mode 2, so enabling the source later in mode 2
    /// doesn't request an interrupt (SameBoy; Gambatte's m2enable, miscmstatirq and lyc tests).
    /// Two more cases were measured against Gambatte: line 0's pulse is lost when the vblank
    /// interrupt was enabled too, and on DMG a STAT write can't bring back a pulse that wasn't
    /// enabled when the line started. Line 144 starts with the source active,
    /// until vblank begins on DMG (GBMicrotest's line_144_oam_int tests) and for its first dot on
    /// CGB.
    fn oam_source_active(&self) -> bool {
        if self.first_line_after_lcd_on {
            return false;
        }
        match self.line {
            0 => self.line_dot == LINE_0_OAM_SOURCE_DOT && !self.line_0_oam_interrupt_blocked,
            1..VBLANK_START_LINE if self.is_cgb() => self.line_dot <= CGB_OAM_SOURCE_LAST_DOT,
            1..VBLANK_START_LINE => {
                self.line_dot <= DMG_OAM_SOURCE_LAST_DOT
                    && self.enabled_at_line_start(OAM_MODE_STAT_SOURCE_BIT)
            }
            VBLANK_START_LINE if self.is_cgb() => {
                self.line_dot == 0
                    || (self.line_dot <= VBLANK_START_DOT
                        && self.enabled_at_line_start(OAM_MODE_STAT_SOURCE_BIT))
            }
            VBLANK_START_LINE => self.line_dot <= VBLANK_START_DOT,
            _ => false,
        }
    }

    /// Whether the mode 2 source stays active even though STAT no longer enables it: on CGB, after
    /// line 144's first dot the source only depends on whether it was enabled when the line
    /// started, so a STAT write disabling it there doesn't end it (Gambatte's m1
    /// m1irq_m2disable_lycdisable tests).
    fn oam_source_kept_from_line_start(&self) -> bool {
        self.is_cgb() && self.line == VBLANK_START_LINE && self.line_dot > 0
    }

    /// Updates the STAT line and requests a STAT interrupt when it rises.
    pub(super) fn update_stat_line(&mut self) {
        let level = self.stat_line_level(self.stat | self.late_cleared_stat_sources);
        if level && !self.stat_line_high {
            self.stat_interrupt = true;
        }
        self.stat_line_high = level;
    }

    /// Writes STAT's interrupt source bits. The CPU's writes go through `write_timing.rs`, which
    /// adds the DMG's STAT write bug and the CGB's late mode 2 bit.
    pub(crate) fn write_stat(&mut self, value: u8) {
        self.stat = value & STAT_WRITABLE_BITS;
        self.update_stat_line();
    }

    /// Writes STAT's interrupt source bits, keeping `late_cleared_sources` (which the write
    /// disables) active for `dots` more dots, as a CGB STAT write does.
    pub(crate) fn write_stat_clearing_late(
        &mut self,
        value: u8,
        late_cleared_sources: u8,
        dots: u8,
    ) {
        if self.lcd_enabled() && late_cleared_sources != 0 {
            self.late_cleared_stat_sources = late_cleared_sources;
            self.countdowns.stat_clear = dots;
        }
        self.write_stat(value);
    }

    pub(crate) fn stat_sources(&self) -> u8 {
        self.stat
    }

    /// The STAT interrupt sources a CGB STAT write at normal speed keeps active for
    /// `CGB_STAT_CLEAR_DELAY_DOTS` more dots when it disables them: LY=LYC and mode 0, and mode 1
    /// while STAT shows it, so it still blocks an LY=LYC interrupt due right after the write
    /// (Gambatte's lycEnable m1disable tests).
    pub(crate) fn cgb_late_cleared_stat_sources(&self) -> u8 {
        if self.mode() == VBLANK_MODE {
            CGB_LATE_CLEARED_STAT_SOURCES | (1 << VBLANK_MODE_STAT_SOURCE_BIT)
        } else {
            CGB_LATE_CLEARED_STAT_SOURCES
        }
    }

    pub(super) fn write_lyc(&mut self, value: u8) {
        self.lyc = value;
        if self.timing == TimingVariant::Cgb && self.lcd_enabled() {
            // The comparison picks LYC up every `CGB_LYC_WRITE_DELAY_DOTS` dots of the line, at
            // the next pickup after the write (a full period later when the write lands on one).
            let delay = CGB_LYC_WRITE_DELAY_DOTS - (self.line_dot % 4) as u8;
            if self.lyc_flag
                && self.line < LAST_LINE
                && self.line_dot >= DOTS_PER_LINE - CGB_LYC_NEXT_LINE_DOTS
                && value == self.line + 1
            {
                // The source stays active at least until the write reaches the comparison.
                let next_line_hold = (DOTS_PER_LINE - self.line_dot + LYC_COMPARE_DOT) as u8;
                self.countdowns.lyc_source_hold = next_line_hold.max(delay + 1);
            }
            self.countdowns.lyc_write = delay;
        } else {
            self.countdowns.lyc_write = 0;
            let old_match = self.lyc_source_match();
            self.compared_lyc = value;
            self.update_lyc_flag();
            if self.lcd_enabled() {
                self.adjust_lyc_source_after_write(value, old_match);
            }
            self.update_stat_line();
        }
    }

    /// Updates the LY=LYC interrupt source after an LYC write that reaches the comparison right
    /// away (on DMG and in double speed), matching what Gambatte does with the same writes.
    /// `old_match` is whether the source saw a match before the write. A write that ends a match
    /// keeps the source active for a dot or two. A write in the last dots of a line, or before the
    /// next line's comparison starts, is compared with the next line instead, unless it starts a
    /// match with that line from no match, which then rises as the comparison reaches the line.
    /// A DMG match that still holds the source at a line's first dot counts as a match, so moving
    /// LYC on to the new line there doesn't request another interrupt (Gambatte's lycEnable
    /// ff45_enable_weirdpoint and lyc153_late_ff45_enable tests).
    fn adjust_lyc_source_after_write(&mut self, value: u8, old_match: bool) {
        let next_line_dots = if self.in_double_speed() {
            if old_match && !self.lyc_flag {
                self.countdowns.lyc_source_hold = DOUBLE_SPEED_LYC_SOURCE_HOLD_DOTS;
            }
            DOUBLE_SPEED_LYC_NEXT_LINE_DOTS
        } else {
            if old_match && !self.lyc_flag {
                self.countdowns.lyc_source_hold = DMG_LYC_SOURCE_HOLD_DOTS;
            }
            DMG_LYC_NEXT_LINE_DOTS
        };
        // Line 153 starts comparing against 0 a few dots in. A write in the same number of dots
        // before that as before a line's comparison starts is compared with 0 the same way
        // (Gambatte's lycEnable lyc153_late_ff45_enable_ds tests).
        let zero_compare_dot = if self.in_double_speed() {
            DOUBLE_SPEED_LAST_LINE_COMPARE_END_DOT
        } else {
            LAST_LINE_COMPARE_END_DOT
        } + 1;
        let next_line = if self.line < LAST_LINE && self.line_dot >= DOTS_PER_LINE - next_line_dots
        {
            self.line + 1
        } else if self.line == LAST_LINE
            && (zero_compare_dot - next_line_dots - LYC_COMPARE_DOT..zero_compare_dot)
                .contains(&self.line_dot)
        {
            0
        } else if self.line > 0 && self.line_dot < LYC_COMPARE_DOT {
            self.line
        } else {
            return;
        };
        if value != next_line || old_match {
            self.lyc_source_line = Some(next_line);
        }
    }

    /// Counts down the late effects of CGB LYC and STAT writes, after the dot's own STAT line
    /// update, and applies them when they are due.
    pub(super) fn finish_late_register_writes(&mut self) {
        if count_down(&mut self.countdowns.lyc_source_hold) {
            self.update_stat_line();
        }
        if count_down(&mut self.countdowns.lyc_write) {
            let old_flag = self.lyc_flag;
            self.compared_lyc = self.lyc;
            self.update_lyc_flag();
            if old_flag && !self.lyc_flag {
                self.countdowns.lyc_source_hold = self
                    .countdowns
                    .lyc_source_hold
                    .max(CGB_LYC_SOURCE_HOLD_DOTS);
            }
            let requested = self.stat_interrupt;
            self.update_stat_line();
            if self.stat_interrupt && !requested {
                self.stat_interrupt = false;
                self.countdowns.lyc_interrupt = CGB_LYC_INTERRUPT_DELAY_DOTS;
            }
        }
        if count_down(&mut self.countdowns.stat_clear) {
            self.late_cleared_stat_sources = 0;
            self.update_stat_line();
        }
    }
}
