use crate::address_bus::bank_utils::{banked_read, banked_write};
use crate::address_bus::cartridge::{Cartridge, CartridgeMapper};
use crate::address_bus::constants::*;
use crate::serializable::Serializable;
use std::io::{Read, Write};

#[derive(Debug)]
pub(super) struct MBC5CartridgeMapper {
    cartridge: Cartridge,
    ram_enabled: bool,
    rumble: bool,
    rom_bank_number: u16,
    ram_bank_number: u8,
}

fn ram_supported(cartridge: &Cartridge) -> bool {
    cartridge.header.type_code == CART_TYPE_MBC5_RAM ||
    cartridge.header.type_code == CART_TYPE_MBC5_RAM_BATTERY ||
    cartridge.header.type_code == CART_TYPE_MBC5_RUMBLE_RAM ||
    cartridge.header.type_code == CART_TYPE_MBC5_RUMBLE_RAM_BATTERY
}

fn rumble_supported(cartridge: &Cartridge) -> bool {
    cartridge.header.type_code == CART_TYPE_MBC5_RUMBLE ||
    cartridge.header.type_code == CART_TYPE_MBC5_RUMBLE_RAM ||
    cartridge.header.type_code == CART_TYPE_MBC5_RUMBLE_RAM_BATTERY
}

fn battery_supported(cartridge: &Cartridge) -> bool {
    cartridge.header.type_code == CART_TYPE_MBC5_RAM_BATTERY ||
    cartridge.header.type_code == CART_TYPE_MBC5_RUMBLE_RAM_BATTERY
}

impl MBC5CartridgeMapper {
    pub(super) fn new(cartridge: Cartridge) -> Self {
        MBC5CartridgeMapper {
            cartridge,
            ram_enabled: false,
            rumble: false,
            rom_bank_number: 1,
            ram_bank_number: 0,
        }    
    }

    fn set_rom_bank_number(&mut self, next_rom_bank_number: u16) {
        if next_rom_bank_number < self.cartridge.header.max_banks {
            self.rom_bank_number = next_rom_bank_number;
        }
    }

    fn set_ram_bank_number(&mut self, next_ram_bank_number: u8) {
        if next_ram_bank_number < self.cartridge.header.max_ram_banks {
            self.ram_bank_number = next_ram_bank_number;
        }
    }
}

impl CartridgeMapper for MBC5CartridgeMapper {
    fn read_rom(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x3FFF =>
                self.cartridge.rom[address as usize],
            0x4000..=0x7FFF => {
                banked_read(&self.cartridge.rom, 0x4000, address, self.rom_bank_number)
            },
            _ => panic!("Invalid ROM address: {:#X}", address),
        }
    }
    
    fn write_rom(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x1FFF => {
                if ram_supported(&self.cartridge) {
                    self.ram_enabled = (value & 0xF) == 0x0A;
                }
            },
            0x2000..=0x2FFF => {
                let next_rom_bank_number = (self.rom_bank_number & 0x100) | (value as u16 & 0xFF);
                self.set_rom_bank_number(next_rom_bank_number);
            },
            0x3000..=0x3FFF => {
                let next_rom_bank_number = (self.rom_bank_number & 0xFF) | ((value as u16 & 0x1) << 8);
                self.set_rom_bank_number(next_rom_bank_number);
            },
            0x4000..=0x5FFF => {
                let next_ram_bank_number = if rumble_supported(&self.cartridge) {
                    self.rumble = (value & 0x8) == 0x1;
                    value & 0x7
                }
                else {
                    value & 0xF
                };
                self.set_ram_bank_number(next_ram_bank_number);
            },
            0x6000..=0x7FFF => {},
            _ => panic!("Invalid ROM address: {:#X}", address),
        }
    }
    
    fn read_ram(&self, address: u16) -> u8 {
        if self.ram_enabled {
            banked_read(&self.cartridge.ram, 0x2000, address, self.ram_bank_number as u16)
        } else {
            0xFF
        }
    }

    fn write_ram(&mut self, address: u16, value: u8) {
        if self.ram_enabled {
            banked_write(&mut self.cartridge.ram, 0x2000, address, self.ram_bank_number as u16, value);
            if battery_supported(&self.cartridge) {
                self.cartridge.effects.save_ram(&self.cartridge.header.title, &self.cartridge.ram);
            }   
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

impl Serializable for MBC5CartridgeMapper {
    fn serialize(&self, writer: &mut dyn Write)-> std::io::Result<()> {
        self.cartridge.ram.serialize(writer)?;
        self.ram_enabled.serialize(writer)?;
        self.rumble.serialize(writer)?;
        self.rom_bank_number.serialize(writer)?;
        self.ram_bank_number.serialize(writer)?;
        Ok(())
    }

    fn deserialize(&mut self, reader: &mut dyn Read)-> std::io::Result<()> {
        self.cartridge.ram.deserialize(reader)?;
        self.ram_enabled.deserialize(reader)?;
        self.rumble.deserialize(reader)?;
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
    fn enable_external_ram_if_correct_cartridge_type() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC5_RAM, ROM_SIZE_64KB, RAM_SIZE_8KB);

        mapper.write_rom(0x0000, 0xA);

        mapper.write_ram(0x0000, 0xC2);
        assert_eq!(mapper.read_ram(0x0000), 0xC2);
    }

    #[test]
    fn enable_external_ram_if_correct_cartridge_type_scenario_two() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC5_RAM_BATTERY, ROM_SIZE_64KB, RAM_SIZE_8KB);

        mapper.write_rom(0x0000, 0xA);

        mapper.write_ram(0x0000, 0xC2);
        assert_eq!(mapper.read_ram(0x000), 0xC2);
    }

    #[test]
    fn enable_external_ram_if_lower_nibble_is_equal_to_a() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC5_RAM, ROM_SIZE_64KB, RAM_SIZE_8KB);

        mapper.write_rom(0x0000, 0x1A);

        mapper.write_ram(0x0000, 0xC2);
        assert_eq!(mapper.read_ram(0x000), 0xC2);
    }

    #[test]
    fn not_enable_external_ram_if_incorrect_cartridge_type() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC5, ROM_SIZE_64KB, RAM_SIZE_8KB);

        mapper.write_rom(0x0000, 0xA);

        mapper.write_ram(0x0000, 0xC2);
        assert_eq!(mapper.read_ram(0x000), 0xFF);
    }

    #[test]
    fn disable_external_ram_if_correct_cartridge_type() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC5_RAM, ROM_SIZE_64KB, RAM_SIZE_8KB);

        mapper.write_rom(0x0000, 0xA);

        mapper.write_ram(0x0000, 0xC2);
        assert_eq!(mapper.read_ram(0x000), 0xC2);

        mapper.write_rom(0x0000, 0xB);

        mapper.write_ram(0x0000, 0xD2);
        assert_eq!(mapper.read_ram(0x000), 0xFF);
    }

    #[test]
    fn set_rom_bank_number() {
        let mut rom = build_rom(CART_TYPE_MBC5_RAM, ROM_SIZE_128KB, RAM_SIZE_8KB);
        rom[0xC005] = 0xA1;
        let mut mapper = load_rom_buffer(rom, empty_cartridge_effects()).unwrap(); 

        mapper.write_rom(0x2000, 0x3);
        
        let byte = mapper.read_rom(0x4005);
        assert_eq!(byte, 0xA1);        
    }

    #[test]
    fn set_rom_bank_number_to_zero() {
        let mut rom = build_rom(CART_TYPE_MBC5_RAM, ROM_SIZE_64KB, RAM_SIZE_8KB);
        rom[0x0005] = 0xA1;
        let mut mapper = load_rom_buffer(rom, empty_cartridge_effects()).unwrap(); 

        mapper.write_rom(0x2000, 0x0);
        
        let byte = mapper.read_rom(0x4005);
        assert_eq!(byte, 0xA1); 
    }

    #[test]
    fn correctly_sets_lower_and_upper_bits_of_the_rom_bank_number() {
        let mut rom = build_rom(CART_TYPE_MBC5_RAM, ROM_SIZE_8MB, RAM_SIZE_8KB);
        rom[0x414005] = 0xA1;
        let mut mapper = load_rom_buffer(rom, empty_cartridge_effects()).unwrap();

        mapper.write_rom(0x2000, 0x5);
        mapper.write_rom(0x3000, 0x1);

        let byte = mapper.read_rom(0x4005);
        assert_eq!(byte, 0xA1);
    }

    #[test]
    fn sets_ram_bank_number() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC5_RAM_BATTERY, ROM_SIZE_64KB, RAM_SIZE_128KB);

        // Enable RAM
        mapper.write_rom(0x0000, 0xA);

        // Set RAM to bank 9
        mapper.write_rom(0x4000, 0x9);

        mapper.write_ram(0x0005, 0xCC);
        let first_byte = mapper.read_ram(0x0005);
        assert_eq!(first_byte, 0xCC);

        // Set RAM to bank 0
        mapper.write_rom(0x4000, 0);
        let second_byte = mapper.read_ram(0x0005);
        assert_eq!(second_byte, 0x00);
    }

    #[test]
    fn saves_bit_three_for_rumble_flag() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC5_RUMBLE_RAM_BATTERY, ROM_SIZE_64KB, RAM_SIZE_128KB);

        // Enable RAM
        mapper.write_rom(0x0000, 0xA);

        // Set RAM to bank 1 (because bit 3 is used for rumble flag)
        mapper.write_rom(0x4000, 0x9);

        mapper.write_ram(0x0005, 0xCC);
        let first_byte = mapper.read_ram(0x0005);
        assert_eq!(first_byte, 0xCC);

        // Set RAM to bank 1 again
        mapper.write_rom(0x4000, 0x1);
        let second_byte = mapper.read_ram(0x0005);
        assert_eq!(second_byte, 0xCC);
    }


    #[test]
    fn only_allow_reading_from_ram_if_it_is_enabled() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC5_RAM_BATTERY, ROM_SIZE_64KB, RAM_SIZE_32KB);

        // Try writing to RAM even though RAM is not enabled
        mapper.write_ram(0x0005, 0xCC);
        let first_byte = mapper.read_ram(0x0005);
        assert_eq!(first_byte, 0xFF);

        // Enable RAM
        mapper.write_rom(0x0000, 0xA);

        mapper.write_ram(0x0005, 0xCC);
        let second_byte = mapper.read_ram(0x0005);
        assert_eq!(second_byte, 0xCC);
    }
}