//! The PPU's save states: everything but the model and the frontend's side of the frame
//! hand-over, checked when it is loaded so that a corrupted save state can't make the PPU index
//! out of bounds.

use crate::gpu::constants::{GB_SCREEN_HEIGHT, GB_SCREEN_WIDTH};
use crate::gpu::{
    DOTS_PER_FRAME, DOTS_PER_LINE, FRAME_SCANLINE_COUNT, Gpu, LAST_LINE, LineState, TimingVariant,
    VBLANK_START_LINE, VRAM_MODE,
};
use crate::serializable::{FieldWriter, Serializable, read_fields};
use std::io::{Read, Write};

/// Implements `Serializable` like `#[derive(Serializable)]` would, for a struct whose loaded values
/// need checking, which the derive can't do: it stores the fields after `saved` with their names,
/// leaves out the ones after `skipped` (loading a state leaves them as they are), and then calls
/// the method after `check`, which rejects values the emulator never produces. The pattern names
/// every field of the struct, so a new field that is in neither list doesn't compile.
macro_rules! impl_serializable_with_check {
    ($type:ident {
        saved: $($saved:ident),* $(,)?;
        skipped: $($skipped:ident),* $(,)?;
        check: $check:ident
    }) => {
        impl Serializable for $type {
            fn serialize(&self, writer: &mut dyn Write) -> std::io::Result<()> {
                let $type { $($saved,)* $($skipped: _,)* } = self;
                let mut fields = FieldWriter::new(writer);
                $(fields.field(stringify!($saved), $saved)?;)*
                fields.finish()
            }

            fn deserialize(&mut self, reader: &mut dyn Read) -> std::io::Result<()> {
                let $type { $($saved,)* $($skipped: _,)* } = self;
                read_fields(reader, |name, bytes| {
                    match name {
                        $(stringify!($saved) => $saved.deserialize(bytes)?,)*
                        _ => return Ok(false),
                    }
                    Ok(true)
                })?;
                self.$check()
            }
        }
    };
}

impl_serializable_with_check!(Gpu {
    saved: line, line_dot, line_state, visible_mode, first_line_after_lcd_on,
        first_frame_after_lcd_on, lyc_flag, previous_lyc_flag, stat_line_high, window_y_triggered,
        delayed_wy, line_0_oam_interrupt_blocked, stat_at_line_start, lyc_source_line, idle_dots,
        stopped, stop_blocks, hdma_start_delay, lcd_off_hblank_dma, fetcher_start_delay,
        mode_0_interrupt_delay, early_hblank_interrupt, pipeline, lcdc, previous_lcdc, scy, scx,
        wx, wy, ly, lyc, compared_lyc, stat, late_cleared_stat_sources, palettes, cgb_vbk, cgb_opri,
        key0, frame_buffer, video_ram, object_attribute_memory, timing, palette_ram_blocked,
        countdowns, stat_interrupt, vblank_interrupt, oam_dma_access, next_oam_dma_access,
        pending_dma_oam_write, oam_scan_y_bus, oam_scan_x_bus, oam_scan_row, sgb;
    // The model is set when the emulator is made and the save state's header names it. The
    // finished frame and whether the frontend took it are the frontend's side of the hand-over:
    // they stay as they are until the loaded state finishes its next frame.
    skipped: model, completed_frame, frame_ready;
    check: check_loaded_state
});

impl Gpu {
    /// Rejects a loaded save state with values the PPU never has, which could otherwise make it
    /// index out of bounds or overflow, or keep it from ever leaving them: a position outside the
    /// frame, a line state or LY that doesn't fit the line, a line outside the frame for LYC to be
    /// compared with, a mode STAT can't show, timing of another model, a frame buffer of
    /// the wrong size, an OAM DMA write outside OAM, a renderer state that doesn't fit
    /// (`Pipeline::is_consistent`), or a Super Game Boy on a model without one, or none on a
    /// model with one.
    fn check_loaded_state(&self) -> std::io::Result<()> {
        let line_state_fits = match self.line_state {
            LineState::OamScan | LineState::Drawing | LineState::HBlank => {
                self.line < VBLANK_START_LINE
            }
            LineState::VBlank => self.line >= VBLANK_START_LINE,
        };
        let drawing_line = (self.line_state == LineState::Drawing && self.fetcher_start_delay == 0)
            .then_some(self.ly);
        let valid = self.line < FRAME_SCANLINE_COUNT
            && self.line_dot < DOTS_PER_LINE
            && self.idle_dots < DOTS_PER_FRAME
            && line_state_fits
            && (self.ly == self.line || (self.line == LAST_LINE && self.ly == 0))
            && self.lyc_source_line.is_none_or(|line| line <= LAST_LINE)
            && self.visible_mode <= VRAM_MODE
            && (self.timing == TimingVariant::Dmg) != self.model.is_cgb()
            && self.frame_buffer.len() == (GB_SCREEN_WIDTH * GB_SCREEN_HEIGHT) as usize
            && self
                .pending_dma_oam_write
                .is_none_or(|write| usize::from(write.index) < self.object_attribute_memory.len())
            && self.pipeline.is_consistent(drawing_line)
            && self.sgb.is_some() == self.model.is_sgb();
        if valid {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "PPU state out of range",
            ))
        }
    }
}
