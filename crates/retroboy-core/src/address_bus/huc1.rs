use serializable_derive::Serializable;

use crate::address_bus::bank_utils::{banked_read, banked_write};
use crate::address_bus::cartridge::{Cartridge, CartridgeMapper};
use crate::serializable::Serializable;
use std::io::{Read, Write};

#[derive(Debug, PartialEq, Serializable)]
pub(super) enum HUC1Mode {
    RAM,
    IR
}

#[derive(Debug)]
pub(super) struct HUC1CartridgeMapper {
    cartridge: Cartridge,
    mode: HUC1Mode,
    ir_transmitter: bool,
    rom_bank_number: u8,
    ram_bank_number: u8, 
}

impl HUC1CartridgeMapper {
    pub(super) fn new(cartridge: Cartridge) -> Self {
        HUC1CartridgeMapper {
            cartridge,
            mode: HUC1Mode::RAM,
            ir_transmitter: false,
            rom_bank_number: 1,
            ram_bank_number: 0,
        }
    }
}

impl CartridgeMapper for HUC1CartridgeMapper {
    fn read_rom(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x3FFF =>
                self.cartridge.rom[address as usize],
            0x4000..=0x7FFF => {
                banked_read(&self.cartridge.rom, 0x4000, address, self.rom_bank_number as u16)
            },
            _ => panic!("Invalid ROM address: {:#X}", address),
        }
    }
    
    fn write_rom(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x1FFF => {
                self.mode = if value == 0xE { HUC1Mode::IR } else { HUC1Mode::RAM };
            },
            0x2000..=0x3FFF => {
                let next_rom_bank_number = (value & 0x3F) as u16;
                if next_rom_bank_number < self.cartridge.header.max_banks {
                    self.rom_bank_number = next_rom_bank_number as u8;
                }
            },
            0x4000..=0x5FFF => {
                let next_ram_bank_number = value & 0x03;
                if next_ram_bank_number < self.cartridge.header.max_ram_banks {
                    self.ram_bank_number = next_ram_bank_number;
                }
            },
            0x6000..=0x7FFF => (), // No behavior observed in this address range for HuC1 cartridges
            _ => panic!("Invalid ROM address: {:#X}", address),
        }
    }
    
    fn read_ram(&self, address: u16) -> u8 {
        if self.mode == HUC1Mode::RAM && self.cartridge.header.max_ram_banks > 0 {
            banked_read(&self.cartridge.ram, 0x2000, address, self.ram_bank_number as u16)
        } else if self.mode == HUC1Mode::IR {
            0xC0
        } else {
            0xFF
        }
    }

    fn write_ram(&mut self, address: u16, value: u8) {
        if self.mode == HUC1Mode::RAM && self.cartridge.header.max_ram_banks > 0 {
            banked_write(&mut self.cartridge.ram, 0x2000, address, self.ram_bank_number as u16, value);
            self.cartridge.effects.save_ram(&self.cartridge.header.title, &self.cartridge.ram);
        }
        else if self.mode == HUC1Mode::IR {
            self.ir_transmitter = value == 0x1;
        } else {
            panic!("Invalid RAM write: {:#X}", address);
        }
    }

    fn get_cartridge(&self) -> &Cartridge {
        &self.cartridge
    }

    fn set_cartridge_ram(&mut self, ram: Vec<u8>) {
        self.cartridge.ram = ram;
    }
    
    fn get_ram_bank(&self) -> u8 {
        self.ram_bank_number
    }
}

impl Serializable for HUC1CartridgeMapper {
    fn serialize(&self, writer: &mut dyn Write)-> std::io::Result<()> {
        self.cartridge.ram.serialize(writer)?;
        self.mode.serialize(writer)?;
        self.ir_transmitter.serialize(writer)?;
        self.rom_bank_number.serialize(writer)?;
        self.ram_bank_number.serialize(writer)?;
        Ok(())
    }

    fn deserialize(&mut self, reader: &mut dyn Read)-> std::io::Result<()> {
        self.cartridge.ram.deserialize(reader)?;
        self.mode.deserialize(reader)?;
        self.ir_transmitter.deserialize(reader)?;
        self.rom_bank_number.deserialize(reader)?;
        self.ram_bank_number.deserialize(reader)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::address_bus::cartridge::*;
    use crate::address_bus::cartridge::test_utils::*;
    use crate::address_bus::constants::*;
    use crate::address_bus::effects::empty_cartridge_effects;
    use crate::address_bus::test_utils::*;

    #[test]
    fn enable_ir_mode() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_HUC1_RAM_BATTERY, ROM_SIZE_64KB, RAM_SIZE_2KB);
        
        mapper.write_rom(0x0000, 0xE);

        assert_eq!(mapper.read_ram(0x000), 0xC0);
    }

    #[test]
    fn enable_external_ram() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_HUC1_RAM_BATTERY, ROM_SIZE_64KB, RAM_SIZE_2KB);

        mapper.write_rom(0x0000, 0xA);

        mapper.write_ram(0x0000, 0xDD);
        assert_eq!(mapper.read_ram(0x000), 0xDD);
    }

    #[test]
    fn set_rom_bank_number() {
        let mut rom = build_rom(CART_TYPE_HUC1_RAM_BATTERY, ROM_SIZE_128KB, RAM_SIZE_2KB);
        rom[0xC005] = 0xA1;
        let mut mapper = load_rom_buffer(rom, empty_cartridge_effects()).unwrap(); 

        mapper.write_rom(0x2000, 0x3);
        
        let byte = mapper.read_rom(0x4005);
        assert_eq!(byte, 0xA1);        
    }

    #[test]
    fn sets_ram_bank_number() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_HUC1_RAM_BATTERY, ROM_SIZE_64KB, RAM_SIZE_32KB);

        // Enable RAM
        mapper.write_rom(0x0000, 0xA);

        // Set RAM to bank 3
        mapper.write_rom(0x4000, 0x3);

        mapper.write_ram(0x0005, 0xCC);
        let first_byte = mapper.read_ram(0x0005);
        assert_eq!(first_byte, 0xCC);

        // Set RAM to bank 0
        mapper.write_rom(0x4000, 0);
        let second_byte = mapper.read_ram(0x0005);
        assert_eq!(second_byte, 0x00);
    }
}