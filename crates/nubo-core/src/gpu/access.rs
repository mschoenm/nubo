//! What the CPU can reach while the PPU works: when OAM and VRAM are blocked, the VRAM banks,
//! VBK and OPRI, and how OAM DMA and STOP mode cut the PPU itself off from memory.

use crate::gpu::{
    DOTS_PER_LINE, DOUBLE_SPEED_VRAM_BLOCK_DOT, DRAWING_START_DOT, DmaOamWrite,
    FRAME_SCANLINE_COUNT, Gpu, LineState, OAM_SCAN_END_DOT, OamDmaAccess, StopBlocks,
    TimingVariant, VBLANK_START_LINE, VRAM_MODE,
};
use crate::model::Model;

/// On DMG, OAM writes are ignored from this dot of OAM scan (SameBoy).
const DMG_OAM_WRITE_BLOCK_DOT: u16 = 4;
/// In double speed OAM reads are still allowed at dot 0 of a line, where writes are already
/// ignored, except on CGB-E and newer (Gambatte's oam_access preread and prewrite _ds tests;
/// AGE's oam-read tests).
const DOUBLE_SPEED_OAM_READ_BLOCK_DOT: u16 = 1;
/// The dot of OAM scan from which CPU writes to OAM are ignored on CGB: from the start of the line
/// (SameBoy; AGE's oam-write test; Gambatte's oam_access tests at both speeds). On DMG it's
/// `DMG_OAM_WRITE_BLOCK_DOT`.
const CGB_OAM_WRITE_BLOCK_DOT: u16 = 0;
/// The first line after the LCD is switched on has no OAM scan, but OAM writes are already
/// ignored two dots before mode 3 (SameBoy; AGE's oam-write test).
const FIRST_LINE_OAM_WRITE_BLOCK_DOT: u16 = DRAWING_START_DOT - 2;
/// On CGB at normal speed the CPU can't reach VRAM from this dot of OAM scan, just before mode 3
/// (fitted to Gambatte's vram_m3 and late_gdma tests); the DMG's reads are blocked from the end of
/// OAM scan.
const CGB_VRAM_BLOCK_DOT: u16 = 83;

impl Gpu {
    /// Whether the line was just drawn but STAT still shows mode 3, which happens for a dot in
    /// double speed. OAM and VRAM stay blocked until STAT changes (SameBoy; AGE's vram-read test,
    /// Gambatte's vram_m3 and vramw_m3end _ds tests).
    fn drawing_just_ended(&self) -> bool {
        self.line_state == LineState::HBlank && self.visible_mode == VRAM_MODE
    }

    /// Whether the CPU reads 0xFF from OAM now because the PPU is using it (modes 2 and 3).
    pub(crate) fn oam_read_blocked(&self) -> bool {
        self.lcd_enabled()
            && match self.line_state {
                LineState::OamScan => {
                    !self.first_line_after_lcd_on
                        && (!self.in_double_speed()
                            || self.model >= Model::CgbE
                            || self.line_dot >= DOUBLE_SPEED_OAM_READ_BLOCK_DOT)
                }
                LineState::Drawing => true,
                // CGB-E keeps OAM blocked for the dot in which drawing ends at normal speed
                // (AGE's oam-read tests).
                LineState::HBlank => {
                    self.drawing_just_ended()
                        || (self.model >= Model::CgbE
                            && !self.in_double_speed()
                            && self.mode_0_interrupt_delay > 0)
                }
                LineState::VBlank => false,
            }
    }

    /// Whether CPU writes to OAM are ignored now. On DMG writes are blocked slightly later than
    /// reads.
    pub(crate) fn oam_write_blocked(&self) -> bool {
        self.lcd_enabled()
            && match self.line_state {
                LineState::OamScan if self.first_line_after_lcd_on => {
                    self.line_dot >= FIRST_LINE_OAM_WRITE_BLOCK_DOT
                }
                LineState::OamScan => {
                    let block_dot = if self.is_cgb() {
                        CGB_OAM_WRITE_BLOCK_DOT
                    } else {
                        DMG_OAM_WRITE_BLOCK_DOT
                    };
                    // The DMG lets writes through in the dots between OAM scan and mode 3;
                    // the CGB keeps them blocked (SameBoy, Gambatte's oam_access midwrite
                    // tests).
                    self.line_dot >= block_dot
                        && (self.is_cgb() || self.line_dot < OAM_SCAN_END_DOT)
                }
                LineState::Drawing => true,
                LineState::HBlank => self.drawing_just_ended(),
                LineState::VBlank => false,
            }
    }

    /// Whether the CPU reads 0xFF from VRAM now because the PPU is drawing.
    pub(crate) fn vram_read_blocked(&self) -> bool {
        self.lcd_enabled()
            && match self.line_state {
                LineState::OamScan => {
                    let block_dot = match self.timing {
                        TimingVariant::Dmg => OAM_SCAN_END_DOT,
                        TimingVariant::Cgb => CGB_VRAM_BLOCK_DOT,
                        TimingVariant::CgbDoubleSpeed => DOUBLE_SPEED_VRAM_BLOCK_DOT,
                    };
                    !self.first_line_after_lcd_on && self.line_dot >= block_dot
                }
                LineState::Drawing => self.countdowns.first_line_vram_grace == 0,
                LineState::HBlank => self.drawing_just_ended(),
                LineState::VBlank => false,
            }
    }

    /// Whether CPU writes to VRAM are ignored now.
    pub(crate) fn vram_write_blocked(&self) -> bool {
        self.lcd_enabled()
            && match self.line_state {
                LineState::OamScan => {
                    let block_dot = match self.timing {
                        // The DMG blocks VRAM reads in the dots between OAM scan and mode 3 (see
                        // `vram_read_blocked`) but still lets writes through (SameBoy).
                        TimingVariant::Dmg => return false,
                        TimingVariant::Cgb => CGB_VRAM_BLOCK_DOT,
                        TimingVariant::CgbDoubleSpeed => DOUBLE_SPEED_VRAM_BLOCK_DOT,
                    };
                    !self.first_line_after_lcd_on && self.line_dot >= block_dot
                }
                LineState::Drawing => self.countdowns.first_line_vram_grace == 0,
                LineState::HBlank => self.drawing_just_ended(),
                LineState::VBlank => false,
            }
    }

    /// Maps a CPU offset into the 8 KiB VRAM window to `video_ram`, using the bank selected by VBK
    /// on CGB.
    fn banked_video_ram_index(&self, index: u16) -> u16 {
        if self.is_cgb() && self.cgb_vbk & 0b1 == 1 {
            index + 0x2000
        } else {
            index
        }
    }

    /// Video RAM (both banks on a CGB) and OAM, as they are stored.
    pub(crate) fn video_memory(&self) -> (&[u8], &[u8]) {
        (&self.video_ram, &self.object_attribute_memory)
    }

    /// Video RAM and OAM, to change directly (see `AddressBus::memory_mut`).
    pub(crate) fn video_memory_mut(&mut self) -> (&mut [u8], &mut [u8]) {
        (&mut self.video_ram, &mut self.object_attribute_memory)
    }

    pub(crate) fn video_ram_byte(&self, index: u16) -> u8 {
        self.video_ram[self.banked_video_ram_index(index) as usize]
    }

    pub(crate) fn set_video_ram_byte(&mut self, index: u16, value: u8) {
        let banked_index = self.banked_video_ram_index(index);
        self.video_ram[banked_index as usize] = value;
    }

    pub(crate) fn object_attribute_memory_byte(&self, index: u16) -> u8 {
        self.object_attribute_memory[index as usize]
    }

    pub(crate) fn set_object_attribute_memory_byte(&mut self, index: u16, value: u8) {
        self.object_attribute_memory[index as usize] = value;
    }

    /// Reads VBK. Unused bits read as 1, and the whole register reads 0xFF on DMG.
    pub(super) fn cgb_vbk(&self) -> u8 {
        if self.is_cgb() {
            self.cgb_vbk | 0b1111_1110
        } else {
            0xFF
        }
    }

    pub(super) fn set_cgb_vbk(&mut self, value: u8) {
        if self.is_cgb() {
            self.cgb_vbk = value;
        }
    }

    /// Reads OPRI. Unused bits read as 1, and the whole register reads 0xFF on DMG.
    pub(super) fn cgb_opri(&self) -> u8 {
        if self.is_cgb() {
            self.cgb_opri | 0b1111_1110
        } else {
            0xFF
        }
    }

    pub(super) fn set_cgb_opri(&mut self, value: u8) {
        if self.is_cgb() {
            self.cgb_opri = value & 0b1;
        }
    }

    /// Whether OAM DMA leaves the PPU's OAM reads alone and has nothing left to apply, so that
    /// `set_oam_dma_access` with no transfer running would change nothing.
    pub(crate) fn oam_dma_access_settled(&self) -> bool {
        matches!(self.oam_dma_access, OamDmaAccess::Free)
            && matches!(self.next_oam_dma_access, OamDmaAccess::Free)
            && self.countdowns.oam_dma_access == 0
            && self.pending_dma_oam_write.is_none()
    }

    /// Sets how OAM DMA affects the PPU's OAM reads, from `delay` dots into the coming M-cycle. The
    /// byte OAM DMA copied in this M-cycle reaches OAM at the same time.
    pub(crate) fn set_oam_dma_access(&mut self, access: OamDmaAccess, delay: u8) {
        if delay == 0 || !self.lcd_enabled() {
            self.oam_dma_access = access;
            self.countdowns.oam_dma_access = 0;
            self.apply_dma_oam_write();
        } else if access == self.oam_dma_access
            && self.countdowns.oam_dma_access == 0
            && self.pending_dma_oam_write.is_none()
        {
            // Nothing would change when the delay runs out, so it isn't started. This keeps the
            // PPU's dots steady (see `has_steady_shape`) while no OAM DMA runs.
            self.next_oam_dma_access = access;
        } else {
            self.next_oam_dma_access = access;
            self.countdowns.oam_dma_access = delay + 1;
        }
    }

    /// Writes a byte OAM DMA copies. It reaches OAM when the PPU starts seeing the M-cycle's DMA
    /// state (`set_oam_dma_access`).
    pub(crate) fn write_oam_from_dma(&mut self, index: u16, value: u8) {
        debug_assert!(usize::from(index) < self.object_attribute_memory.len());
        self.apply_dma_oam_write();
        self.pending_dma_oam_write = Some(DmaOamWrite { index, value });
    }

    pub(super) fn apply_dma_oam_write(&mut self) {
        if let Some(write) = self.pending_dma_oam_write.take() {
            self.object_attribute_memory[usize::from(write.index)] = write.value;
        }
    }

    /// Called when the CPU enters STOP mode. A monochrome model's PPU stands still, and its next
    /// white frame is due when the next vblank would have started. The CGB's PPU keeps running
    /// but loses access to whatever the CPU could access at that moment (SameBoy), so STOP
    /// outside mode 3 turns the screen black while STOP in mode 3 leaves the picture alone
    /// (daid's stop_instr tests).
    pub(crate) fn enter_stop_mode(&mut self) {
        self.stopped = true;
        if self.is_cgb() {
            self.stop_blocks = StopBlocks {
                oam: !self.oam_read_blocked(),
                vram: !self.vram_read_blocked(),
                palettes: !self.palette_ram_blocked,
            };
        } else if self.lcd_enabled() {
            let lines_since_vblank = (u32::from(self.line) + u32::from(FRAME_SCANLINE_COUNT)
                - u32::from(VBLANK_START_LINE))
                % u32::from(FRAME_SCANLINE_COUNT);
            self.idle_dots =
                lines_since_vblank * u32::from(DOTS_PER_LINE) + u32::from(self.line_dot);
        }
    }

    /// Called when a button press ends STOP mode.
    pub(crate) fn leave_stop_mode(&mut self) {
        self.stopped = false;
        self.stop_blocks = StopBlocks::default();
    }
}
