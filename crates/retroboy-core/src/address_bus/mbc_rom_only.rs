use crate::address_bus::cartridge::{Cartridge, CartridgeMapper};
use crate::serializable::Serializable;
use std::io::{Read, Write};

#[derive(Debug)]
pub(super) struct MBCRomOnlyCartridgeMapper {
    cartridge: Cartridge
}

impl MBCRomOnlyCartridgeMapper {
    pub(super) fn new(cartridge: Cartridge) -> Self {
        MBCRomOnlyCartridgeMapper {
            cartridge
        }
    }
}

impl CartridgeMapper for MBCRomOnlyCartridgeMapper {
    fn read_rom(&self, address: u16) -> u8 {
        self.cartridge.rom[address as usize]
    }

    fn write_rom(&mut self, _: u16, _: u8) {
        ()
    }

    fn read_ram(&self, _: u16) -> u8 {
        0xFF
    }

    fn write_ram(&mut self, _: u16, _: u8) {
        ()
    }

    fn get_cartridge(&self) -> &Cartridge {
        &self.cartridge
    }

    fn set_cartridge_ram(&mut self, _: Vec<u8>) {
        ()
    }

    fn get_ram_bank(&self) -> u8 {
        0
    }
}

impl Serializable for MBCRomOnlyCartridgeMapper {
    fn serialize(&self, writer: &mut dyn Write)-> std::io::Result<()> {
        self.cartridge.ram.serialize(writer)?;
        Ok(())
    }

    fn deserialize(&mut self, reader: &mut dyn Read)-> std::io::Result<()> {
        self.cartridge.ram.deserialize(reader)?;
        Ok(())
    }
}
