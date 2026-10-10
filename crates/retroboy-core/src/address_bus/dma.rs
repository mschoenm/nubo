use crate::address_bus::{AddressBus, MemoryMapped};
use crate::serializable::Serializable;
use getset::{CopyGetters, Setters};
use serializable_derive::Serializable;

#[derive(Debug, Serializable, CopyGetters, Setters)]
#[getset(get_copy = "pub(super)", set = "pub(super)")]
pub(super) struct DMAState {
    #[getset(skip)]
    source: u16,
    offset: u8,
    delay: u8,
    in_progress: bool
}

pub(super) const DMA_TRANSFER_BYTES: u8 = 160;

impl DMAState {
    pub(super) fn new() -> Self {
        DMAState {
            source: 0x0,
            offset: 0x0,
            delay: 0,
            in_progress: false
        }
    }

    pub(super) fn start_dma(&mut self, source: u8) {
        self.source = (source as u16) << 8;

        if !self.in_progress {
            self.offset = 0x0;
            self.delay = 2;
            self.in_progress = true;
        }
    }

    pub(super) fn source(&self) -> u8 {
        (self.source >> 8) as u8
    }

    pub(super) fn source_address(&self) -> u16 {
        self.source
    }

    #[cfg(test)]
    pub(super) fn set_source_address(&mut self, source: u16) {
        self.source = source;
    }
}

impl AddressBus {
    fn dma_transfer_byte(&mut self) {
        let offset = self.dma.offset() as u16;
        let address = self.dma.source_address() + offset;
        let byte_to_transfer = self.read_byte(address);
        self.gpu.set_object_attribute_memory_byte(offset, byte_to_transfer);
    }

    pub(super) fn dma_step(&mut self) {
        if self.dma.in_progress() {
            let delay = self.dma.delay();
            if delay > 0 {
                self.dma.set_delay(delay - 1);
            }
            else {
                self.dma_transfer_byte();
        
                let next_offset = self.dma.offset() + 1;
                self.dma.set_offset(next_offset);
        
                if next_offset == DMA_TRANSFER_BYTES {
                    self.dma.set_in_progress(false);
                } 
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::address_bus::constants::*;
    use crate::address_bus::effects::empty_cartridge_effects;
    use crate::address_bus::test_utils::*;
    use super::*;

    #[test]
    fn should_start_dma_transfer() {
        let mut dma = DMAState::new();
        dma.start_dma(0x12);
        assert_eq!(dma.source_address(), 0x1200);
        assert_eq!(dma.offset(), 0x0);
        assert_eq!(dma.in_progress(), true);
    }

    #[test]
    fn should_allow_modifications_to_dma_register_if_transfer_is_already_in_progress() {
        let mut dma = DMAState::new();
        dma.set_in_progress(true);
        dma.start_dma(0x12);
        assert_eq!(dma.source_address(), 0x1200);
        assert_eq!(dma.offset(), 0x0);
        assert_eq!(dma.in_progress(), true);
    }

    #[test]
    fn should_transfer_byte_from_source_to_destination() {
        let mut address_bus = initialize_test_address_bus();

        let mut rom = build_rom(CART_TYPE_MBC1, ROM_SIZE_64KB, RAM_SIZE_2KB);
        rom[0x1200] = 0x12;
        address_bus.load_rom_buffer(rom, empty_cartridge_effects()).unwrap();

        address_bus.dma.set_source_address(0x1200);
        address_bus.dma.set_offset(0x0);
        address_bus.dma.set_in_progress(true);
        
        address_bus.dma_step();
        
        assert_eq!(address_bus.gpu.get_object_attribute_memory_byte(0), 0x12);
        assert_eq!(address_bus.dma.source_address(), 0x1200);
        assert_eq!(address_bus.dma.offset(), 1);
        assert_eq!(address_bus.dma.in_progress(), true);
    }

    #[test]
    fn should_stop_dma_transfer_after_transferring_160_bytes() {
        let mut address_bus = initialize_test_address_bus();

        let mut rom = build_rom(CART_TYPE_MBC1, ROM_SIZE_64KB, RAM_SIZE_2KB);
        rom[0x129F] = 0x12;
        address_bus.load_rom_buffer(rom, empty_cartridge_effects()).unwrap();

        address_bus.dma.set_source_address(0x1200);
        address_bus.dma.set_offset(0x0);
        address_bus.dma.set_in_progress(true);
        
        for _ in 0..DMA_TRANSFER_BYTES {
            address_bus.dma_step();
        }
        
        assert_eq!(address_bus.gpu.get_object_attribute_memory_byte(0x9F), 0x12);
        assert_eq!(address_bus.dma.offset(), DMA_TRANSFER_BYTES);
        assert_eq!(address_bus.dma.in_progress(), false);
    }

    #[test]
    fn should_do_nothing_if_no_dma_transfer_is_in_progress() {
        let mut address_bus = initialize_test_address_bus();
        address_bus.dma_step();
        assert_eq!(address_bus.dma.source_address(), 0x0);
        assert_eq!(address_bus.dma.offset(), 0x0);
        assert_eq!(address_bus.dma.in_progress(), false);
    }
}