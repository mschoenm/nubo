//! The MBC5 memory bank controller, used by most Game Boy Color games. It
//! supports up to 8 MiB of ROM and 128 KiB of RAM, and some cartridges have a
//! rumble motor.

use crate::address_bus::bank_utils::{banked_read, banked_write};
use crate::address_bus::constants::{
    CART_TYPE_MBC5_RAM, CART_TYPE_MBC5_RAM_BATTERY, CART_TYPE_MBC5_RUMBLE,
    CART_TYPE_MBC5_RUMBLE_RAM, CART_TYPE_MBC5_RUMBLE_RAM_BATTERY, RAM_BANK_SIZE,
};
use serializable_derive::Serializable;

/// The registers of an MBC5, with or without RAM and rumble.
#[derive(Debug, Serializable)]
pub(super) struct Mbc5CartridgeMapper {
    ram_enabled: bool,
    /// Whether the game has switched the rumble motor on.
    is_rumbling: bool,
    /// The 9-bit ROM bank visible at 0x4000-0x7FFF. Unlike older controllers,
    /// bank 0 can be selected here.
    rom_bank_number: u16,
    ram_bank_number: u8,
    /// Whether the cartridge type has RAM, which the game can then enable.
    #[serializable(skip)]
    has_ram: bool,
    /// Whether the cartridge has a rumble motor, which takes over bit 3 of
    /// the RAM bank number.
    #[serializable(skip)]
    has_rumble: bool,
}

impl Mbc5CartridgeMapper {
    /// The controller at power-on, for a cartridge with this type byte.
    pub(super) fn new(cartridge_type: u8) -> Self {
        Mbc5CartridgeMapper {
            ram_enabled: false,
            is_rumbling: false,
            rom_bank_number: 1,
            ram_bank_number: 0,
            has_ram: matches!(
                cartridge_type,
                CART_TYPE_MBC5_RAM
                    | CART_TYPE_MBC5_RAM_BATTERY
                    | CART_TYPE_MBC5_RUMBLE_RAM
                    | CART_TYPE_MBC5_RUMBLE_RAM_BATTERY
            ),
            has_rumble: matches!(
                cartridge_type,
                CART_TYPE_MBC5_RUMBLE
                    | CART_TYPE_MBC5_RUMBLE_RAM
                    | CART_TYPE_MBC5_RUMBLE_RAM_BATTERY
            ),
        }
    }

    /// The ROM banks visible at 0x0000-0x3FFF and 0x4000-0x7FFF.
    pub(super) fn rom_banks(&self) -> [u16; 2] {
        [0, self.rom_bank_number]
    }

    /// The RAM bank visible at 0xA000-0xBFFF.
    pub(super) fn ram_bank(&self) -> u8 {
        self.ram_bank_number
    }

    /// Sets a register by writing to the ROM area.
    pub(super) fn write_rom(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x1FFF => {
                if self.has_ram {
                    self.ram_enabled = (value & 0xF) == 0x0A;
                }
            }
            0x2000..=0x2FFF => {
                self.rom_bank_number = (self.rom_bank_number & 0x100) | u16::from(value);
            }
            0x3000..=0x3FFF => {
                self.rom_bank_number =
                    (self.rom_bank_number & 0xFF) | ((u16::from(value) & 0x1) << 8);
            }
            0x4000..=0x5FFF => {
                self.ram_bank_number = if self.has_rumble {
                    self.is_rumbling = (value & 0x8) != 0;
                    value & 0x7
                } else {
                    value & 0xF
                };
            }
            0x6000..=0x7FFF => {}
            _ => unreachable!("the address bus only routes ROM addresses here, got {address:#X}"),
        }
    }

    /// Reads `ram` at `address` (relative to 0xA000), or 0xFF while the RAM
    /// is disabled.
    pub(super) fn read_ram(&self, ram: &[u8], address: u16) -> u8 {
        if self.ram_enabled {
            banked_read(ram, RAM_BANK_SIZE, address, u16::from(self.ram_bank_number))
        } else {
            0xFF
        }
    }

    /// Writes `ram` at `address` (relative to 0xA000) if the RAM is enabled.
    /// Returns whether a byte was stored.
    pub(super) fn write_ram(&mut self, ram: &mut [u8], address: u16, value: u8) -> bool {
        self.ram_enabled
            && banked_write(
                ram,
                RAM_BANK_SIZE,
                address,
                u16::from(self.ram_bank_number),
                value,
            )
    }
}

#[cfg(test)]
mod tests {
    use super::Mbc5CartridgeMapper;
    use crate::address_bus::cartridge::test_utils::*;
    use crate::address_bus::cartridge::*;
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
        let mut mapper =
            build_cartridge_mapper(CART_TYPE_MBC5_RAM_BATTERY, ROM_SIZE_64KB, RAM_SIZE_8KB);

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
        let mut mapper = load_cartridge(rom, empty_cartridge_effects()).unwrap();

        mapper.write_rom(0x2000, 0x3);

        let byte = mapper.read_rom(0x4005);
        assert_eq!(byte, 0xA1);
    }

    #[test]
    fn set_rom_bank_number_to_zero() {
        let mut rom = build_rom(CART_TYPE_MBC5_RAM, ROM_SIZE_64KB, RAM_SIZE_8KB);
        rom[0x0005] = 0xA1;
        let mut mapper = load_cartridge(rom, empty_cartridge_effects()).unwrap();

        mapper.write_rom(0x2000, 0x0);

        let byte = mapper.read_rom(0x4005);
        assert_eq!(byte, 0xA1);
    }

    #[test]
    fn correctly_sets_lower_and_upper_bits_of_the_rom_bank_number() {
        let mut rom = build_rom(CART_TYPE_MBC5_RAM, ROM_SIZE_8MB, RAM_SIZE_8KB);
        rom[0x0041_4005] = 0xA1;
        let mut mapper = load_cartridge(rom, empty_cartridge_effects()).unwrap();

        mapper.write_rom(0x2000, 0x5);
        mapper.write_rom(0x3000, 0x1);

        let byte = mapper.read_rom(0x4005);
        assert_eq!(byte, 0xA1);
    }

    #[test]
    fn sets_ram_bank_number() {
        let mut mapper =
            build_cartridge_mapper(CART_TYPE_MBC5_RAM_BATTERY, ROM_SIZE_64KB, RAM_SIZE_128KB);

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
        let mut mapper = build_cartridge_mapper(
            CART_TYPE_MBC5_RUMBLE_RAM_BATTERY,
            ROM_SIZE_64KB,
            RAM_SIZE_128KB,
        );

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
    fn sets_rumble_flag_from_bit_three() {
        let mut mapper = Mbc5CartridgeMapper::new(CART_TYPE_MBC5_RUMBLE);

        mapper.write_rom(0x4000, 0x8);
        assert!(mapper.is_rumbling);

        mapper.write_rom(0x4000, 0x1);
        assert!(!mapper.is_rumbling);
    }

    #[test]
    fn only_allow_reading_from_ram_if_it_is_enabled() {
        let mut mapper =
            build_cartridge_mapper(CART_TYPE_MBC5_RAM_BATTERY, ROM_SIZE_64KB, RAM_SIZE_32KB);

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

    #[test]
    fn battery_backed_ram_is_saved_once_per_report_while_enabled() {
        assert_eq!(ram_saves_after_writing(CART_TYPE_MBC5_RAM_BATTERY, 0x0A), 1);
        assert_eq!(
            ram_saves_after_writing(CART_TYPE_MBC5_RUMBLE_RAM_BATTERY, 0x0A),
            1
        );
        assert_eq!(ram_saves_after_writing(CART_TYPE_MBC5_RAM_BATTERY, 0x00), 0);
        assert_eq!(ram_saves_after_writing(CART_TYPE_MBC5_RAM, 0x0A), 0);
    }
}
