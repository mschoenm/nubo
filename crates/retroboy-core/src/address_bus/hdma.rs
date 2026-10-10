use crate::address_bus::{AddressBus, MemoryMapped};
use crate::utils::is_bit_set;
use crate::serializable::Serializable;
use getset::{CopyGetters, Getters, Setters};
use serializable_derive::Serializable;

#[derive(Serializable, Debug, PartialEq, Eq)]
pub(super) enum VRAMTransferMode {
    GeneralPurpose,
    HBlank
}

#[derive(Debug, Serializable, CopyGetters, Getters, Setters)]
pub(crate) struct HDMAState {
    hdma1: u8,
    hdma2: u8,
    hdma3: u8,
    hdma4: u8,
    #[getset(get_copy = "pub(super)", set = "pub(super)")]
    offset: u16,
    #[getset(get_copy = "pub(super)", set = "pub(super)")]
    transfer_length: u8,
    #[getset(get = "pub(super)")]
    transfer_mode: VRAMTransferMode,
    #[getset(get_copy = "pub(crate)", set = "pub(super)")]
    in_progress: bool,
    #[getset(get_copy = "pub(super)", set = "pub(super)")]
    completed: bool,
    hblank_started: bool,
    #[getset(set = "pub(super)")]
    cgb_mode: bool,
    #[getset(get_copy = "pub(super)", set = "pub(super)")]
    cgb_double_speed: bool
}

const VRAM_TRANSFER_INDEX: u8 = 7;
const BLOCK_SIZE: u8 = 16;

impl HDMAState {
    pub(crate) fn new() -> Self {
        HDMAState {
            hdma1: 0x0,
            hdma2: 0x0,
            hdma3: 0x0,
            hdma4: 0x0,
            offset: 0x0,
            transfer_length: 0x0,
            transfer_mode: VRAMTransferMode::GeneralPurpose,
            in_progress: false,
            completed: true,
            hblank_started: false,
            cgb_mode: false,
            cgb_double_speed: false
        }
    }

    fn set_hdma1(&mut self, value: u8) {
        if self.cgb_mode {
            self.hdma1 = value;
        }
    }

    fn set_hdma2(&mut self, value: u8) {
        if self.cgb_mode {
            self.hdma2 = value;
        }
    }

    fn set_hdma3(&mut self, value: u8) {
        if self.cgb_mode {
            self.hdma3 = value;
        }
    }

    fn set_hdma4(&mut self, value: u8) {
        if self.cgb_mode {
            self.hdma4 = value;
        }
    }

    fn set_hdma5(&mut self, value: u8) {
        if self.cgb_mode {
            if self.in_progress && !is_bit_set(value, VRAM_TRANSFER_INDEX) {
                self.in_progress = false;
                self.offset = 0;
            }
            else {
                self.transfer_length = value & 0b01111111;
       
                let transfer_bit_set = is_bit_set(value, VRAM_TRANSFER_INDEX);
                let mode = if transfer_bit_set { VRAMTransferMode::HBlank } else { VRAMTransferMode::GeneralPurpose };
                self.transfer_mode = mode;
       
                self.in_progress = true;
                self.completed = false;
            }
        }
    }

    fn hdma5(&self) -> u8 {
        if self.cgb_mode {
            if self.in_progress {
                self.transfer_length & 0b01111111
            } else if self.completed {
                0xFF
            } else {
                0b10000000 | (self.transfer_length & 0b01111111)
            }
        }
        else {
            0xFF
        }
    }

    pub(crate) fn set_hblank_started(&mut self, value: bool) {
        if self.in_progress {
            self.hblank_started = value; 
        }
    }

    pub(super) fn get_vram_dma_source(&self) -> u16 {
        ((self.hdma1 as u16) << 8) | ((self.hdma2 as u16) & 0b11110000)
    }

    pub(super) fn get_vram_dma_destination(&self) -> u16 {
        let offset = (((self.hdma3 as u16) & 0b00011111) << 8) |
            ((self.hdma4 as u16) & 0b11110000);

        0x8000 + offset
    }

    pub(super) fn hblank_started(&self) -> bool {
        self.hblank_started
    }
}

impl AddressBus {
    fn hdma_transfer_block(&mut self, source: u16, destination: u16) {
        for _ in (0..BLOCK_SIZE).step_by(2) {
            for _ in 0..2 {
                let offset = self.hdma.offset();
                let source_byte = self.read_byte(source + offset);
                self.write_byte(destination + offset, source_byte);
                self.hdma.set_offset(offset + 1);
            }

            // Takes one machine cycle (or two "fast" machine cycles in double speed mode)
            // to transfer two bytes during VRAM DMA.
            let cycle_count = if self.hdma.cgb_double_speed { 2 } else { 1 };
            for _ in 0..cycle_count {
                self.sync();
            }
        }

        if self.hdma.transfer_length() == 0 {
            self.hdma.set_completed(true);
            self.hdma.set_in_progress(false);
            self.hdma.set_offset(0);
        }
        else {
            let transfer_length = self.hdma.transfer_length();
            self.hdma.set_transfer_length(transfer_length - 1);
        }
    }

    pub(crate) fn hdma_step(&mut self) {
        if self.cgb_mode && self.hdma.in_progress() {
            let source = self.hdma.get_vram_dma_source();
            let destination = self.hdma.get_vram_dma_destination();
            
            let is_hblank_mode = self.hdma.transfer_mode() == &VRAMTransferMode::HBlank;
            if is_hblank_mode && self.hdma.hblank_started() {
                self.hdma_transfer_block(source, destination);
                self.hdma.set_hblank_started(false);
            }
            else if !is_hblank_mode {
                while !self.hdma.completed() {
                    self.hdma_transfer_block(source, destination);
                }
            }
        }
    }
}

impl MemoryMapped for HDMAState {
    fn read_byte(&self, address: u16) -> u8 {
        match address {
            0xFF55 => self.hdma5(),
            _ => panic!("Invalid HDMA address: 0x{:04X}", address)
        }
    }

    fn write_byte(&mut self, address: u16, value: u8) {
        match address {
            0xFF51 => self.set_hdma1(value),
            0xFF52 => self.set_hdma2(value),
            0xFF53 => self.set_hdma3(value),
            0xFF54 => self.set_hdma4(value),
            0xFF55 => self.set_hdma5(value),
            _ => panic!("Invalid HDMA address: 0x{:04X}", address)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::address_bus::AddressBus;
    use crate::address_bus::constants::*;
    use crate::address_bus::effects::empty_cartridge_effects;
    use crate::address_bus::test_utils::*;
    use super::*;

    #[test]
    fn should_calculate_vram_dma_source() {
        let mut hdma = HDMAState::new();
        hdma.set_cgb_mode(true);
        hdma.set_hdma1(0x71);
        hdma.set_hdma2(0xA2);
        let vram_source = hdma.get_vram_dma_source();
        assert_eq!(vram_source, 0x71A0);
    }

    #[test]
    fn should_calculate_vram_dma_destination() {
        let mut hdma = HDMAState::new();
        hdma.set_cgb_mode(true);
        hdma.set_hdma3(0x71);
        hdma.set_hdma4(0xA2);
        let vram_destination = hdma.get_vram_dma_destination();
        assert_eq!(vram_destination, 0x91A0);
    }

    #[test]
    fn should_get_general_purpose_for_vram_transfer_mode() {
        let mut hdma = HDMAState::new();
        hdma.set_cgb_mode(true);
        hdma.set_hdma5(0x71);
        assert_eq!(hdma.transfer_mode(), &VRAMTransferMode::GeneralPurpose);
    }

    #[test]
    fn should_get_hblank_for_vram_transfer_mode() {
        let mut hdma = HDMAState::new();
        hdma.set_cgb_mode(true);
        hdma.set_hdma5(0xF1);
        assert_eq!(hdma.transfer_mode(), &VRAMTransferMode::HBlank);
    }

    #[test]
    fn should_get_hdma5_when_transfer_is_not_active() {
        let mut hdma = HDMAState::new();
        hdma.set_cgb_mode(true);
        let hdma5 = hdma.hdma5();
        assert_eq!(hdma5, 0xFF);
    }

    #[test]
    fn should_get_hdma5_when_transfer_is_active() {
        let mut hdma = HDMAState::new();
        hdma.set_cgb_mode(true);
        hdma.set_in_progress(true);
        hdma.set_transfer_length(0b01010110);
        let hdma5 = hdma.hdma5();
        assert_eq!(hdma5, 0b01010110);
    }

    #[test]
    fn should_terminate_active_vram_dma_transfer() {
        let mut hdma = HDMAState::new();
        hdma.set_cgb_mode(true);
        hdma.set_in_progress(true);
        hdma.set_completed(false);
        hdma.set_transfer_length(0b01010110);
        hdma.set_hdma5(0x0);
        assert_eq!(hdma.in_progress(), false);
        let hdma5 = hdma.hdma5();
        assert_eq!(hdma5, 0b11010110);
    }

    #[test]
    fn should_transfer_sixteen_bytes_in_hblank_mode() {
        let mut address_bus = AddressBus::new(false);
        address_bus.cgb_mode = true;
        address_bus.hdma.set_cgb_mode(true);

        let mut test_instructions = build_rom(CART_TYPE_MBC1, ROM_SIZE_64KB, RAM_SIZE_2KB);
        for i in 0..32 {
            test_instructions[0x71A0 + i] = 0xA1;
        }

        address_bus.load_rom_buffer(test_instructions, empty_cartridge_effects()).unwrap();

        address_bus.hdma.set_hdma1(0x71);
        address_bus.hdma.set_hdma2(0xA2);
        address_bus.hdma.set_hdma3(0x71);
        address_bus.hdma.set_hdma4(0xA2);
        address_bus.hdma.set_hdma5(0x81); // transfer length of 0x20

        address_bus.gpu.set_mode(0); // mode 0 = hblank mode
        address_bus.hdma.set_hblank_started(true);

        address_bus.hdma_step();

        for i in 0..16 {
            assert_eq!(address_bus.gpu.get_video_ram_byte(0x11A0 + i), 0xA1);
        }
        
        for i in 16..32 {
            assert_eq!(address_bus.gpu.get_video_ram_byte(0x11A0 + i), 0x0);
        }

        assert_eq!(address_bus.hdma.hblank_started(), false);
    }

    #[test]
    fn should_transfer_all_bytes_in_general_purpose_mode() {
        let mut address_bus = AddressBus::new(false);
        address_bus.cgb_mode = true;
        address_bus.hdma.set_cgb_mode(true);

        let mut test_instructions = build_rom(CART_TYPE_MBC1, ROM_SIZE_64KB, RAM_SIZE_2KB);
        for i in 0..32 {
            test_instructions[0x71A0 + i] = 0xA1;
        }

        address_bus.load_rom_buffer(test_instructions, empty_cartridge_effects()).unwrap();

        address_bus.hdma.set_hdma1(0x71);
        address_bus.hdma.set_hdma2(0xA2);
        address_bus.hdma.set_hdma3(0x71);
        address_bus.hdma.set_hdma4(0xA2);
        address_bus.hdma.set_hdma5(0x01); // transfer length of 0x20

        address_bus.hdma_step();

        for i in 0..32 {
            assert_eq!(address_bus.gpu.get_video_ram_byte(0x11A0 + i), 0xA1);
        }
    }
}
