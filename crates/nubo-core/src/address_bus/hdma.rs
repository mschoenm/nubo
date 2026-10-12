//! CGB VRAM DMA (also called HDMA): copies data into VRAM (video RAM) in
//! blocks of 16 bytes. The address bus only routes HDMA1-HDMA5 here on a CGB,
//! so the registers need no check for the model. It runs either all at once ("general purpose" mode,
//! the CPU waits until it finishes) or one block per HBlank (the pause at the
//! end of each screen line). The registers are HDMA1-HDMA5 at 0xFF51-0xFF55.

use crate::address_bus::{AddressBus, MemoryMapped};
use crate::utils::is_bit_set;
use getset::{CopyGetters, Setters};
use serializable_derive::Serializable;

/// How a VRAM DMA transfer is paced.
#[derive(Serializable, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum VramTransferMode {
    /// Copies everything at once while the CPU waits.
    GeneralPurpose,
    /// Copies one 16-byte block at the start of each HBlank.
    HBlank,
}

/// The state of the VRAM DMA unit and its registers.
#[derive(Clone, Debug, Serializable, CopyGetters, Setters)]
pub(crate) struct HdmaState {
    /// The address the next byte is read from (HDMA1/HDMA2).
    source: u16,
    /// The destination counter (HDMA3/HDMA4). Only its low 13 bits address
    /// VRAM, but it counts on past 0x9FFF, and the transfer stops when it
    /// wraps around to 0 (SameBoy, Gambatte's dma_dst_wrap tests).
    destination: u16,
    /// HDMA5 bits 0-6: the number of 16-byte blocks left to copy, minus one.
    #[getset(get_copy = "pub(super)", set = "pub(super)")]
    transfer_length: u8,
    #[getset(get_copy = "pub(super)")]
    transfer_mode: VramTransferMode,
    #[getset(get_copy = "pub(crate)", set = "pub(super)")]
    in_progress: bool,
    /// The last transfer copied all its blocks. False while a transfer runs
    /// and after one was cancelled part-way; HDMA5 reads differently then.
    #[getset(get_copy = "pub(super)", set = "pub(super)")]
    completed: bool,
    /// An HBlank started, so an HBlank-mode transfer should copy its next
    /// block.
    hblank_started: bool,
    /// The CPU halted outside HBlank, so a block that becomes due while it is
    /// halted can still be copied when it wakes up.
    halted_outside_hblank: bool,
    /// A block was due when STOP switched speed, so it is copied when the CPU
    /// wakes up whatever mode the PPU is in by then.
    keep_block_through_halt: bool,
    /// The block kept through a speed switch halt starts copying in the
    /// M-cycle the CPU wakes up in, which already ran.
    copy_starts_on_wake: bool,
    /// The transfer was cancelled while a block was already due, so it stops
    /// once that block is copied.
    cancelled_after_block: bool,
}

const HBLANK_MODE_BIT: u8 = 7;
const BLOCK_SIZE: u8 = 16;
/// Half an M-cycle at normal speed, in dots.
const HALF_M_CYCLE_DOTS: u8 = 2;
const VRAM_START: u16 = 0x8000;
const VRAM_ADDRESS_MASK: u16 = 0x1FFF;

impl HdmaState {
    pub(crate) fn new() -> Self {
        HdmaState {
            source: 0x0,
            destination: 0x0,
            transfer_length: 0x0,
            transfer_mode: VramTransferMode::GeneralPurpose,
            in_progress: false,
            completed: true,
            hblank_started: false,
            halted_outside_hblank: false,
            keep_block_through_halt: false,
            copy_starts_on_wake: false,
            cancelled_after_block: false,
        }
    }

    /// Sets the high byte of the source address.
    fn set_hdma1(&mut self, value: u8) {
        self.source = (u16::from(value) << 8) | (self.source & 0x00F0);
    }

    /// Sets the low byte of the source address. The lower four bits are
    /// ignored, so transfers start on a 16-byte boundary.
    fn set_hdma2(&mut self, value: u8) {
        self.source = (self.source & 0xFF00) | (u16::from(value) & 0xF0);
    }

    /// Sets the high byte of the VRAM destination.
    fn set_hdma3(&mut self, value: u8) {
        self.destination = (u16::from(value) << 8) | (self.destination & 0x00F0);
    }

    /// Sets the low byte of the VRAM destination. The lower four bits are
    /// ignored.
    fn set_hdma4(&mut self, value: u8) {
        self.destination = (self.destination & 0xFF00) | (u16::from(value) & 0xF0);
    }

    /// Writes HDMA5. Starts a transfer of `(value & 0x7F) + 1` blocks, in
    /// HBlank mode if bit 7 is set. Writing with bit 7 clear while a transfer
    /// runs cancels it instead, though a block that is already due is still
    /// copied (SameBoy).
    fn set_hdma5(&mut self, value: u8) {
        if self.in_progress && !is_bit_set(value, HBLANK_MODE_BIT) {
            if self.hblank_started {
                self.cancelled_after_block = true;
            } else {
                self.in_progress = false;
            }
            self.transfer_length = value & 0b0111_1111;
        } else {
            self.transfer_length = value & 0b0111_1111;

            self.transfer_mode = if is_bit_set(value, HBLANK_MODE_BIT) {
                VramTransferMode::HBlank
            } else {
                VramTransferMode::GeneralPurpose
            };
            self.hblank_started = false;
            self.copy_starts_on_wake = false;

            self.in_progress = true;
            self.completed = false;
        }
    }

    /// Reads HDMA5: the blocks left minus one, with bit 7 set when no transfer
    /// runs. Reads 0xFF once a transfer has copied all its blocks.
    fn hdma5(&self) -> u8 {
        if self.in_progress {
            self.transfer_length & 0b0111_1111
        } else if self.completed {
            0xFF
        } else {
            0b1000_0000 | (self.transfer_length & 0b0111_1111)
        }
    }

    /// Records that an HBlank started. Ignored when no transfer runs.
    pub(crate) fn set_hblank_started(&mut self, value: bool) {
        if self.in_progress {
            self.hblank_started = value;
        }
    }

    pub(super) fn source_address(&self) -> u16 {
        self.source
    }

    pub(super) fn destination_address(&self) -> u16 {
        VRAM_START + (self.destination & VRAM_ADDRESS_MASK)
    }

    fn advance_addresses(&mut self) {
        self.source = self.source.wrapping_add(1);
        self.destination = self.destination.wrapping_add(1);
    }

    /// An HBlank-mode transfer is running and waits for the next HBlank.
    pub(super) fn hblank_transfer_waiting(&self) -> bool {
        self.in_progress && self.transfer_mode == VramTransferMode::HBlank
    }

    pub(super) fn hblank_started(&self) -> bool {
        self.hblank_started
    }
}

impl AddressBus {
    /// Copies one 16-byte block, two bytes per M-cycle (an M-cycle is four
    /// clock ticks), and ends the transfer after its last block. With
    /// `trailer` the copy is followed by one more M-cycle in which the CPU
    /// still waits.
    ///
    /// The HBlank that requested the block is dealt with once the last bytes
    /// are copied. At normal speed that is half an M-cycle before the block
    /// ends, so an HBlank that starts in that last half M-cycle requests the
    /// next block (SameBoy; Gambatte's hdma_transition_halt_late_unhalt tests).
    ///
    /// An OAM DMA running meanwhile copies the byte this transfer just read
    /// instead of its own, to the OAM index given by the low byte of this
    /// transfer's source address (measured against Gambatte; Gambatte's
    /// hdma_transition_oamdma and oamdmasrcC000_hdmasrc0000 tests).
    fn hdma_transfer_block(&mut self, trailer: bool) {
        for pair in 0..BLOCK_SIZE / 2 {
            for byte in 0..2 {
                let source = self.hdma.source_address();
                let source_byte = match source {
                    // VRAM DMA only reaches the cartridge and work RAM; from VRAM,
                    // OAM, the I/O registers and HRAM it copies 0xFF (SameBoy,
                    // Gambatte's dma_*_read tests).
                    0x8000..=0x9FFF | 0xE000.. => 0xFF,
                    address => self.peek(address),
                };
                let destination = self.hdma.destination_address() & VRAM_ADDRESS_MASK;
                self.catch_up_ppu_before_change();
                self.gpu.set_video_ram_byte(destination, source_byte);
                self.hdma.advance_addresses();
                self.vram_dma_bus = Some((source, source_byte));
                // In double speed each byte takes one M-cycle of the faster CPU clock.
                if self.speed_switch.cgb_double_speed() {
                    self.sync();
                    if std::mem::take(&mut self.pause_oam_dma_after_cycle) {
                        self.dma_halt_started();
                    }
                } else if byte == 1 {
                    if pair == BLOCK_SIZE / 2 - 1 && !trailer {
                        self.sync_except_last_dots(HALF_M_CYCLE_DOTS);
                        self.hdma.hblank_started = false;
                        self.finish_sync(HALF_M_CYCLE_DOTS);
                    } else {
                        self.sync();
                    }
                    if std::mem::take(&mut self.pause_oam_dma_after_cycle) {
                        self.dma_halt_started();
                    }
                }
            }
        }
        self.vram_dma_bus = None;
        if trailer {
            if self.speed_switch.cgb_double_speed() {
                self.sync();
            } else {
                self.sync_except_last_dots(HALF_M_CYCLE_DOTS);
                self.hdma.hblank_started = false;
                self.finish_sync(HALF_M_CYCLE_DOTS);
            }
        }
        if self.speed_switch.cgb_double_speed() {
            self.hdma.hblank_started = false;
        }

        if self.hdma.transfer_length() == 0 {
            self.hdma.set_completed(true);
            self.hdma.set_in_progress(false);
        } else {
            let transfer_length = self.hdma.transfer_length();
            self.hdma.set_transfer_length(transfer_length - 1);
            if self.hdma.destination == 0 {
                self.hdma.set_in_progress(false);
            }
        }
    }

    /// Runs VRAM DMA before the CPU's next instruction: the whole transfer in
    /// general purpose mode, or one block in HBlank mode once an HBlank has
    /// started. The CPU waits meanwhile.
    pub(crate) fn hdma_step(&mut self) {
        if self.cgb_mode && self.hdma.in_progress() {
            let is_hblank_mode = self.hdma.transfer_mode() == VramTransferMode::HBlank;
            if is_hblank_mode && self.hdma.hblank_started() {
                let trailer = !std::mem::take(&mut self.hdma.copy_starts_on_wake);
                self.hdma_transfer_block(trailer);
                if std::mem::take(&mut self.hdma.cancelled_after_block) {
                    self.hdma.in_progress = false;
                    self.hdma.completed = false;
                }
            } else if !is_hblank_mode {
                while self.hdma.in_progress() {
                    self.hdma_transfer_block(false);
                }
                self.sync();
            }
        }
    }

    /// Copies an HBlank block while the CPU is halted by a speed switch, which
    /// ends the transfer: HDMA5 then reads bit 7 set with the length from
    /// before the block. OAM DMA runs during the block's first M-cycle and
    /// then stands still like after any halt (Gambatte's
    /// hdma_transition_speedchange_oamdma test).
    pub(crate) fn copy_hdma_block_ending_transfer(&mut self) {
        let length = self.hdma.transfer_length;
        self.pause_oam_dma_after_cycle = true;
        self.hdma_transfer_block(true);
        self.pause_oam_dma_after_cycle = false;
        self.hdma.transfer_length = length;
        self.hdma.in_progress = false;
        self.hdma.completed = false;
    }

    /// Whether an HBlank-mode transfer is waiting to copy its next block.
    pub(crate) fn hdma_block_pending(&self) -> bool {
        self.cgb_mode
            && self.hdma.in_progress()
            && self.hdma.transfer_mode() == VramTransferMode::HBlank
            && self.hdma.hblank_started
    }

    /// Called when the CPU halts after a speed switch with an HBlank block
    /// already due: the block waits until the CPU wakes up.
    pub(crate) fn keep_hdma_block_through_halt(&mut self) {
        self.hdma.keep_block_through_halt = true;
    }

    /// Called when the CPU halts after a switch back to normal speed with an
    /// HBlank block already due: the block waits until the CPU wakes up, and
    /// its copy starts in the M-cycle the CPU wakes up in (Gambatte's
    /// hdma_late_m3speedchange_tima _ds tests).
    pub(crate) fn keep_hdma_block_through_speed_switch_halt(&mut self) {
        self.hdma.keep_block_through_halt = true;
        self.hdma.copy_starts_on_wake = true;
    }

    /// Called when the CPU halts. Remembers whether it halted outside HBlank
    /// (HBlank is mode 0 in STAT, the LCD status register at 0xFF41), or
    /// before the HBlank's DMA trigger.
    pub(crate) fn hdma_halt_started(&mut self) {
        self.catch_up_ppu();
        self.hdma.halted_outside_hblank = !self.gpu.hblank_dma_triggered();
    }

    /// Called when the CPU wakes from HALT. A pending HBlank block is dropped
    /// unless the CPU halted outside HBlank and wakes up inside one.
    pub(crate) fn hdma_halt_ended(&mut self) {
        if std::mem::take(&mut self.hdma.keep_block_through_halt) {
            return;
        }
        self.catch_up_ppu();
        if !self.hdma.halted_outside_hblank || self.gpu.mode() != 0 {
            self.hdma.hblank_started = false;
        }
    }
}

impl MemoryMapped for HdmaState {
    fn read_byte(&self, address: u16) -> u8 {
        match address {
            0xFF55 => self.hdma5(),
            _ => {
                unreachable!("the address bus only routes HDMA addresses here, got 0x{address:04X}")
            }
        }
    }

    fn write_byte(&mut self, address: u16, value: u8) {
        match address {
            0xFF51 => self.set_hdma1(value),
            0xFF52 => self.set_hdma2(value),
            0xFF53 => self.set_hdma3(value),
            0xFF54 => self.set_hdma4(value),
            0xFF55 => self.set_hdma5(value),
            _ => {
                unreachable!("the address bus only routes HDMA addresses here, got 0x{address:04X}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address_bus::AddressBus;
    use crate::address_bus::constants::*;
    use crate::address_bus::effects::empty_cartridge_effects;
    use crate::address_bus::test_utils::*;

    #[test]
    fn should_calculate_vram_dma_source() {
        let mut hdma = HdmaState::new();
        hdma.set_hdma1(0x71);
        hdma.set_hdma2(0xA2);
        assert_eq!(hdma.source_address(), 0x71A0);
    }

    #[test]
    fn should_calculate_vram_dma_destination() {
        let mut hdma = HdmaState::new();
        hdma.set_hdma3(0x71);
        hdma.set_hdma4(0xA2);
        assert_eq!(hdma.destination_address(), 0x91A0);
    }

    #[test]
    fn should_get_general_purpose_for_vram_transfer_mode() {
        let mut hdma = HdmaState::new();
        hdma.set_hdma5(0x71);
        assert_eq!(hdma.transfer_mode(), VramTransferMode::GeneralPurpose);
    }

    #[test]
    fn should_get_hblank_for_vram_transfer_mode() {
        let mut hdma = HdmaState::new();
        hdma.set_hdma5(0xF1);
        assert_eq!(hdma.transfer_mode(), VramTransferMode::HBlank);
    }

    #[test]
    fn should_get_hdma5_when_transfer_is_not_active() {
        let hdma = HdmaState::new();
        assert_eq!(hdma.hdma5(), 0xFF);
    }

    #[test]
    fn should_get_hdma5_when_transfer_is_active() {
        let mut hdma = HdmaState::new();
        hdma.set_in_progress(true);
        hdma.set_transfer_length(0b0101_0110);
        assert_eq!(hdma.hdma5(), 0b0101_0110);
    }

    #[test]
    fn should_terminate_active_vram_dma_transfer() {
        let mut hdma = HdmaState::new();
        hdma.set_in_progress(true);
        hdma.set_completed(false);
        hdma.set_transfer_length(0b0101_0110);
        hdma.set_hdma5(0x0);
        assert!(!hdma.in_progress());
        assert_eq!(hdma.hdma5(), 0b1000_0000);
    }

    #[test]
    fn should_transfer_sixteen_bytes_in_hblank_mode() {
        let mut address_bus = AddressBus::new(false);
        address_bus.cgb_mode = true;

        let mut test_instructions = build_rom(CART_TYPE_MBC1, ROM_SIZE_64KB, RAM_SIZE_2KB);
        for i in 0..32 {
            test_instructions[0x71A0 + i] = 0xA1;
        }

        address_bus
            .load_rom(test_instructions, empty_cartridge_effects())
            .unwrap();

        address_bus.hdma.set_hdma1(0x71);
        address_bus.hdma.set_hdma2(0xA2);
        address_bus.hdma.set_hdma3(0x71);
        address_bus.hdma.set_hdma4(0xA2);
        address_bus.hdma.set_hdma5(0x81); // Two blocks of 16 bytes.

        address_bus.gpu.set_mode(0);
        address_bus.hdma.set_hblank_started(true);

        address_bus.hdma_step();

        for i in 0..16 {
            assert_eq!(address_bus.gpu.video_ram_byte(0x11A0 + i), 0xA1);
        }

        for i in 16..32 {
            assert_eq!(address_bus.gpu.video_ram_byte(0x11A0 + i), 0x0);
        }

        assert!(!address_bus.hdma.hblank_started());
    }

    #[test]
    fn should_transfer_all_bytes_in_general_purpose_mode() {
        let mut address_bus = AddressBus::new(false);
        address_bus.cgb_mode = true;

        let mut test_instructions = build_rom(CART_TYPE_MBC1, ROM_SIZE_64KB, RAM_SIZE_2KB);
        for i in 0..32 {
            test_instructions[0x71A0 + i] = 0xA1;
        }

        address_bus
            .load_rom(test_instructions, empty_cartridge_effects())
            .unwrap();

        address_bus.hdma.set_hdma1(0x71);
        address_bus.hdma.set_hdma2(0xA2);
        address_bus.hdma.set_hdma3(0x71);
        address_bus.hdma.set_hdma4(0xA2);
        address_bus.hdma.set_hdma5(0x01); // Two blocks of 16 bytes.

        address_bus.hdma_step();

        for i in 0..32 {
            assert_eq!(address_bus.gpu.video_ram_byte(0x11A0 + i), 0xA1);
        }
    }
}
