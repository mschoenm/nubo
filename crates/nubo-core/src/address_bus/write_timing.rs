/*
    CPU writes to some PPU registers land a dot or two away from the M-cycle boundary, and some
    briefly expose an intermediate value. These offsets follow SameBoy's access conflict tables
    (https://github.com/LIJI32/SameBoy, Expat License, see the notice in apu.rs); which STAT bit the
    CGB changes late is fitted to Gambatte's m2enable tests instead, SCY in double speed lands
    early like SCX (Gambatte's scy_during_m3_ds tests), and the DMG's fine scroll bits of SCX land
    a dot after the coarse ones (Gambatte's scx_m3_extend and scx_0761c0 tests).
*/

//! When CPU writes to PPU registers take effect, measured in dots (a dot is
//! one tick of the 4 MHz PPU clock; an M-cycle is four dots, or two in CGB
//! double speed mode).
//!
//! A write normally lands at the end of its M-cycle. For the registers listed
//! in `write_timing` the PPU is first run a few dots short of
//! the boundary ("early") or a dot past it ("late"), the write happens, and
//! the difference is kept in `ppu_dot_debt` so the next M-cycle makes it up.

use crate::address_bus::{AddressBus, MemoryMapped, reaches_ppu, writes_plain_memory};
use crate::gpu::CGB_STAT_CLEAR_DELAY_DOTS;
use crate::model::Model;

const LCDC_ADDRESS: u16 = 0xFF40;
const LCDC_BG_ENABLE: u8 = 0x01;
const LCDC_OBJ_ENABLE: u8 = 0x02;
const LCDC_OBJ_SIZE: u8 = 0x04;
const LCDC_TILE_SELECT: u8 = 0x10;
const LCDC_WINDOW_ENABLE: u8 = 0x20;
const LCDC_ENABLE: u8 = 0x80;
/// The STAT interrupt sources the DMG's STAT write bug enables: mode 0, mode 1
/// and LY=LYC. Mode 2's source is only a pulse as the mode starts, so the bug
/// neither enables nor disables it.
const STAT_WRITE_BUG_SOURCES: u8 = 0x58;
const STAT_MODE_2_SOURCE: u8 = 0x20;
const STAT_MODE_0_SOURCE: u8 = 0x08;
const STAT_MODE_1_SOURCE: u8 = 0x10;
const STAT_LYC_SOURCE: u8 = 0x40;
/// LCDC bits a DMG write changes one dot early: object size, background tile
/// map, tile data select and window tile map.
const LCDC_EARLY_BITS: u8 = 0x5C;
/// The fine scroll bits of SCX, which pick the pixel within a tile.
const SCX_FINE_BITS: u8 = 0x07;

/// How a CPU write to a register lines up with the PPU's dots.
enum WriteTiming {
    /// Lands on the M-cycle boundary like any other write.
    Immediate,
    /// Lands this many dots before the boundary.
    Early(u8),
    /// Lands this many dots after the boundary.
    Late(u8),
    /// DMG palette write: for one dot the PPU sees the old and new values
    /// ORed together.
    DmgPalette,
    /// DMG LCDC (the LCD control register at 0xFF40) write: some bits change
    /// one dot before the rest.
    DmgLcdc,
    /// SGB LCDC write: the write is applied and undone one dot early, so its
    /// side effects happen then but the PPU sees the new value a dot later.
    SgbLcdc,
    /// DMG SCX (horizontal scroll) write: the coarse scroll (bits 3-7)
    /// changes two dots early and the fine scroll (bits 0-2) a dot later.
    DmgScx,
    /// DMG WX (window X position) write: the PPU is told WX just changed for
    /// one dot.
    DmgWx,
    /// CGB LCDC write that may trigger the tile select glitch for one dot.
    /// One that changes the object size makes OAM scan see either size in
    /// the dot it lands in.
    CgbLcdc,
    /// CGB LCDC write in double speed mode: everything but the LCD and
    /// background enable bits, and the window enable bit when it is set,
    /// changes one dot early.
    CgbDoubleSpeedLcdc,
    /// DMG STAT (0xFF41) write: for one dot STAT acts as if every interrupt
    /// source but mode 2 was enabled, which can request an interrupt (the
    /// "STAT write bug"), then the new value takes effect.
    DmgStat,
    /// CGB STAT write at normal speed: the mode 2 source bit changes a dot
    /// after the others, and disabled LY=LYC and mode 0 sources keep
    /// requesting interrupts for two more dots.
    CgbStat,
    /// CGB STAT write in double speed mode: disabled LY=LYC and mode 0
    /// sources keep requesting interrupts for one more dot.
    CgbDoubleSpeedStat,
}

impl AddressBus {
    /// Picks the timing of a CPU write to `address` for the current model and
    /// speed. Only some PPU registers differ from [`WriteTiming::Immediate`].
    fn write_timing(&self, address: u16) -> WriteTiming {
        if self.processor_test_mode() || address & 0xFF80 != 0xFF00 {
            return WriteTiming::Immediate;
        }

        let double_speed = self.speed_switch.cgb_double_speed();
        let cgb_palette_lead = if self.model >= Model::CgbD { 2 } else { 1 };
        let sgb = self.model.is_sgb();
        match (self.cgb_mode, double_speed, address & 0xFF) {
            // IF lands a dot late on every model, so an interrupt requested in the
            // write's last dot is cleared by it (Gambatte's m2int_m0irq_scx3_ifw tests).
            (_, _, 0x0F) => WriteTiming::Late(1),
            (false, _, 0x40) if sgb => WriteTiming::SgbLcdc,
            (false, _, 0x47..=0x49) if sgb => WriteTiming::Early(1),
            (false, _, 0x40) => WriteTiming::DmgLcdc,
            (false, _, 0x42) => WriteTiming::Early(2),
            (false, _, 0x43) => WriteTiming::DmgScx,
            (false, _, 0x47..=0x49) => WriteTiming::DmgPalette,
            (false, _, 0x4B) => WriteTiming::DmgWx,
            (false, _, 0x41) => WriteTiming::DmgStat,
            (true, false, 0x40) => WriteTiming::CgbLcdc,
            (true, false, 0x47..=0x49) => WriteTiming::Early(cgb_palette_lead),
            (true, false, 0x4B) => WriteTiming::Late(1),
            (true, true, 0x40) => WriteTiming::CgbDoubleSpeedLcdc,
            (true, false, 0x41) => WriteTiming::CgbStat,
            (true, true, 0x41) => WriteTiming::CgbDoubleSpeedStat,
            (true, true, 0x42 | 0x43) => WriteTiming::Early(1),
            _ => WriteTiming::Immediate,
        }
    }

    /// Runs one M-cycle in which the CPU writes `value` to `address`, with the
    /// write landing at the dot the hardware would.
    #[inline]
    pub(crate) fn cpu_write_cycle(&mut self, address: u16, value: u8) {
        // Unlike a read (see `cpu_read_byte`), the write's own M-cycle runs here, before the write
        // lands, and it can start a requested OAM DMA transfer that the write then collides with.
        // So plain writes only take this path while no transfer is requested either.
        if writes_plain_memory(address) && self.dma.is_idle() && !self.processor_test_mode() {
            self.sync();
            self.write_plain_memory(address, value);
        } else {
            self.cpu_write_cycle_elsewhere(address, value);
        }
    }

    /// `cpu_write_cycle` for everything but plain memory written while no OAM
    /// DMA is requested or running.
    #[inline(never)]
    fn cpu_write_cycle_elsewhere(&mut self, address: u16, value: u8) {
        let plain_write = writes_plain_memory(address);
        if !plain_write {
            self.run_deferred_cycles_before_change();
        }
        let ppu_write = reaches_ppu(address);
        if ppu_write {
            self.catch_up_ppu();
            self.ppu_eager = true;
        }
        self.timed_write_cycle(address, value);
        if ppu_write {
            self.ppu_eager = false;
            self.ppu_quiet_dots = 0;
        }
        if !plain_write {
            // The write may have changed what decides how many M-cycles can wait.
            self.run_deferred_cycles_before_change();
        }
    }

    /// Runs the M-cycle of a CPU write, with the PPU run up to the dot the write lands on.
    fn timed_write_cycle(&mut self, address: u16, value: u8) {
        match self.write_timing(address) {
            WriteTiming::Immediate => {
                self.sync();
                self.cpu_write_byte(address, value);
            }
            WriteTiming::Early(lead) => {
                self.sync_with_ppu_lead(lead);
                self.poke(address, value);
                self.ppu_dot_debt += i16::from(lead);
            }
            WriteTiming::Late(delay) => {
                self.sync();
                self.advance_ppu(delay);
                self.poke(address, value);
                self.ppu_dot_debt -= i16::from(delay);
            }
            WriteTiming::DmgPalette => {
                self.sync_with_ppu_lead(2);
                let old_value = self.gpu.read_byte(address);
                self.gpu.write_byte(address, old_value | value);
                self.advance_ppu(1);
                self.gpu.write_byte(address, value);
                self.ppu_dot_debt += 1;
            }
            WriteTiming::DmgLcdc => {
                let mut old_value = self.gpu.lcdc();
                self.sync_with_ppu_lead(2);
                if value & LCDC_OBJ_ENABLE == 0
                    && self
                        .gpu
                        .object_fetch_conflicts_with_lcdc_write(self.model != Model::Mgb)
                {
                    old_value &= !LCDC_OBJ_ENABLE;
                }
                let early_value = ((old_value | (value & LCDC_BG_ENABLE))
                    & !LCDC_EARLY_BITS
                    & (value | !LCDC_WINDOW_ENABLE))
                    | (value & LCDC_EARLY_BITS);
                if old_value & LCDC_WINDOW_ENABLE != 0
                    && early_value & LCDC_WINDOW_ENABLE == 0
                    && self.gpu.window_is_being_fetched()
                {
                    self.gpu.disable_window_pixel_insertion_glitch();
                }
                self.gpu.write_byte(LCDC_ADDRESS, early_value);
                self.gpu
                    .set_written_bg_enable(Some(value & LCDC_BG_ENABLE != 0));
                self.advance_ppu(1);
                self.gpu.set_written_bg_enable(None);
                self.gpu.write_byte(LCDC_ADDRESS, value);
                if old_value & LCDC_WINDOW_ENABLE != 0
                    && value & LCDC_WINDOW_ENABLE == 0
                    && self.gpu.window_is_being_fetched()
                {
                    self.gpu.disable_window_pixel_insertion_glitch();
                }
                self.ppu_dot_debt += 1;
            }
            WriteTiming::SgbLcdc => {
                let old_value = self.gpu.lcdc();
                self.sync_with_ppu_lead(2);
                self.gpu.write_byte(LCDC_ADDRESS, value);
                self.gpu.write_byte(LCDC_ADDRESS, old_value);
                self.advance_ppu(1);
                self.gpu.write_byte(LCDC_ADDRESS, value);
                self.ppu_dot_debt += 1;
            }
            WriteTiming::DmgWx => {
                self.sync();
                self.gpu.write_byte(address, value);
                self.gpu.set_wx_just_changed(true);
                self.advance_ppu(1);
                self.gpu.set_wx_just_changed(false);
                self.ppu_dot_debt -= 1;
            }
            WriteTiming::DmgScx => {
                self.sync_with_ppu_lead(2);
                let old_value = self.gpu.read_byte(address);
                self.gpu.write_byte(
                    address,
                    (value & !SCX_FINE_BITS) | (old_value & SCX_FINE_BITS),
                );
                self.advance_ppu(1);
                self.gpu.write_byte(address, value);
                self.ppu_dot_debt += 1;
            }
            WriteTiming::CgbLcdc => {
                let old_value = self.gpu.lcdc();
                if (value ^ old_value) & LCDC_OBJ_SIZE != 0 {
                    // OAM scan finds objects of either size in the write's last dot, while
                    // mode 3 sees the new size only once the write lands (Gambatte's sprites
                    // late_sizechange tests; Mealybug's m3_lcdc_obj_size_change).
                    self.sync_with_ppu_lead(1);
                    self.gpu.set_tall_objects_glitch(true);
                    self.advance_ppu(1);
                    self.gpu.set_tall_objects_glitch(false);
                } else {
                    self.sync();
                }
                self.gpu.write_byte(LCDC_ADDRESS, value);
                if !value & old_value & LCDC_TILE_SELECT != 0 {
                    self.gpu.set_tile_sel_glitch(true);
                    self.advance_ppu(1);
                    self.gpu.set_tile_sel_glitch(false);
                    self.ppu_dot_debt -= 1;
                }
            }
            WriteTiming::DmgStat => {
                self.sync();
                // The glitch also happens with the LCD off, where the LY=LYC flag keeps its last
                // value and can still request an interrupt (Gambatte's lycEnable lcdoff_lycirqen
                // tests).
                let mut bug_sources =
                    STAT_WRITE_BUG_SOURCES | (self.gpu.stat_sources() & STAT_MODE_2_SOURCE);
                // The glitch only catches an HBlank that is already requesting interrupts,
                // not one that starts during its dot (Gambatte's m0enable disable_scx tests).
                if !self.gpu.hblank_source_active() {
                    bug_sources &= !STAT_MODE_0_SOURCE;
                }
                self.gpu.write_stat(bug_sources);
                // Only the glitch's mode 0 and mode 1 sources last for the rest of its dot;
                // LY=LYC and mode 2 already follow the written value (Gambatte's m0enable
                // lycdisable_ff41 tests).
                self.gpu
                    .write_stat((bug_sources & (STAT_MODE_0_SOURCE | STAT_MODE_1_SOURCE)) | value);
                self.advance_ppu(1);
                self.ppu_dot_debt -= 1;
                self.gpu.write_stat(value);
            }
            WriteTiming::CgbStat => {
                self.sync();
                let old_value = self.gpu.stat_sources();
                self.gpu.write_stat_clearing_late(
                    (old_value & STAT_MODE_2_SOURCE) | (value & !STAT_MODE_2_SOURCE),
                    old_value & !value & self.gpu.cgb_late_cleared_stat_sources(),
                    CGB_STAT_CLEAR_DELAY_DOTS,
                );
                self.advance_ppu(1);
                self.gpu.write_stat(value);
                self.ppu_dot_debt -= 1;
            }
            WriteTiming::CgbDoubleSpeedStat => {
                // Measured against Gambatte (Gambatte's m0enable disable_scx5_ds and
                // lycdisable_ff41_scx1_ds tests).
                self.sync();
                let late_cleared = self.gpu.stat_sources() & (STAT_MODE_0_SOURCE | STAT_LYC_SOURCE);
                self.gpu.write_stat(value | late_cleared);
                self.advance_ppu(1);
                self.gpu.write_stat(value);
                self.ppu_dot_debt -= 1;
            }
            WriteTiming::CgbDoubleSpeedLcdc => {
                let old_value = self.gpu.lcdc();
                self.sync_with_ppu_lead(1);
                // Setting the window enable bit lands with the late bits, a dot after clearing
                // it would (Gambatte's window late_enable and late_reenable _ds tests), as on DMG.
                let kept_bits =
                    LCDC_BG_ENABLE | LCDC_ENABLE | (value & !old_value & LCDC_WINDOW_ENABLE);
                self.gpu
                    .write_byte(LCDC_ADDRESS, (value & !kept_bits) | (old_value & kept_bits));
                self.gpu
                    .set_tile_sel_glitch((value ^ old_value) & LCDC_TILE_SELECT != 0);
                self.advance_ppu(1);
                self.gpu.set_tile_sel_glitch(false);
                self.gpu.write_byte(LCDC_ADDRESS, value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A write timed to the dot runs the PPU up to that dot first, which only
    /// works if the PPU caught up before the write's M-cycle (`reaches_ppu`).
    #[test]
    fn every_register_with_a_timed_write_reaches_the_ppu() {
        for model in Model::ALL {
            for double_speed in [false, true] {
                if double_speed && !model.is_cgb() {
                    continue;
                }
                let mut address_bus = AddressBus::new(false);
                address_bus.set_model(model);
                address_bus.set_cgb_double_speed(double_speed);
                for address in 0xFF00..=0xFF7F {
                    if !matches!(address_bus.write_timing(address), WriteTiming::Immediate) {
                        assert!(
                            reaches_ppu(address),
                            "{model:?} (double speed: {double_speed}) times writes to \
                             {address:#06X}, which the PPU doesn't catch up for"
                        );
                    }
                }
            }
        }
    }
}
