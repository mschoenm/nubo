//! The MBC1 memory bank controller, used by most early Game Boy games.
//!
//! MBC1 has two bank registers. BANK1 holds the lower five bits of the ROM
//! bank at 0x4000-0x7FFF. BANK2 holds two more bits, which either extend the
//! ROM bank or select the RAM bank, depending on the banking mode.

use crate::address_bus::bank_utils::{banked_read, banked_write};
use crate::address_bus::constants::{
    CART_TYPE_MBC1_RAM, CART_TYPE_MBC1_RAM_BATTERY, RAM_BANK_SIZE,
};
use nubo_serializable_derive::Serializable;

/// What BANK2 is used for, chosen by writing to 0x6000-0x7FFF.
#[derive(Debug, PartialEq, Serializable)]
pub(super) enum Mbc1BankingMode {
    /// BANK2 only extends the ROM bank at 0x4000-0x7FFF; RAM bank 0 is used.
    Rom,
    /// BANK2 also selects the ROM bank at 0x0000-0x3FFF and the RAM bank.
    Ram,
}

/// The registers of an MBC1, including MBC1M multicarts that hold several
/// games.
#[derive(Debug, Serializable)]
pub(super) struct Mbc1CartridgeMapper {
    ram_enabled: bool,
    /// The lower five bits of the ROM bank number, never zero.
    bank1: u8,
    /// Two more bank bits, for the ROM bank or the RAM bank.
    bank2: u8,
    mode: Mbc1BankingMode,
    /// MBC1M multicarts wire BANK2 one bit lower, so BANK1 only uses four bits.
    #[serializable(skip)]
    is_multicart: bool,
    /// Whether the cartridge type has RAM, which the game can then enable.
    #[serializable(skip)]
    has_ram: bool,
}

const MULTICART_ROM_SIZE: usize = 0x0010_0000;
const MULTICART_GAME_SIZE: usize = 0x40000;
/// Where the Nintendo logo is stored in the header of each game.
const LOGO_RANGE: std::ops::Range<usize> = 0x104..0x134;

/// Detects a 1 MiB multicart: one of the other 256 KiB games in the ROM has
/// the same Nintendo logo as the first one.
fn is_multicart(rom: &[u8]) -> bool {
    rom.len() == MULTICART_ROM_SIZE
        && (1..4).any(|game| {
            let game_start = game * MULTICART_GAME_SIZE;
            rom[LOGO_RANGE] == rom[game_start + LOGO_RANGE.start..game_start + LOGO_RANGE.end]
        })
}

impl Mbc1CartridgeMapper {
    /// The controller at power-on, for a cartridge with this ROM and type byte.
    pub(super) fn new(rom: &[u8], cartridge_type: u8) -> Self {
        Mbc1CartridgeMapper {
            ram_enabled: false,
            bank1: 1,
            bank2: 0,
            mode: Mbc1BankingMode::Rom,
            is_multicart: is_multicart(rom),
            has_ram: matches!(
                cartridge_type,
                CART_TYPE_MBC1_RAM | CART_TYPE_MBC1_RAM_BATTERY
            ),
        }
    }

    /// BANK2 shifted into place above the BANK1 bits of a ROM bank number.
    fn upper_bank_bits(&self) -> u16 {
        let shift = if self.is_multicart { 4 } else { 5 };
        u16::from(self.bank2) << shift
    }

    /// The ROM bank visible at 0x0000-0x3FFF.
    fn lower_rom_bank(&self) -> u16 {
        match self.mode {
            Mbc1BankingMode::Rom => 0,
            Mbc1BankingMode::Ram => self.upper_bank_bits(),
        }
    }

    /// The ROM bank visible at 0x4000-0x7FFF.
    fn upper_rom_bank(&self) -> u16 {
        let bank1_mask = if self.is_multicart { 0x0F } else { 0x1F };
        self.upper_bank_bits() | u16::from(self.bank1 & bank1_mask)
    }

    /// The ROM banks visible at 0x0000-0x3FFF and 0x4000-0x7FFF.
    pub(super) fn rom_banks(&self) -> [u16; 2] {
        [self.lower_rom_bank(), self.upper_rom_bank()]
    }

    /// The RAM bank visible at 0xA000-0xBFFF.
    pub(super) fn ram_bank(&self) -> u8 {
        match self.mode {
            Mbc1BankingMode::Rom => 0,
            Mbc1BankingMode::Ram => self.bank2,
        }
    }

    /// Sets a register by writing to the ROM area.
    pub(super) fn write_rom(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x1FFF => {
                if self.has_ram {
                    self.ram_enabled = (value & 0xF) == 0x0A;
                }
            }
            0x2000..=0x3FFF => {
                let bank1 = value & 0x1F;
                self.bank1 = if bank1 == 0 { 1 } else { bank1 };
            }
            0x4000..=0x5FFF => {
                self.bank2 = value & 0x3;
            }
            0x6000..=0x7FFF => {
                self.mode = if value & 0x1 == 1 {
                    Mbc1BankingMode::Ram
                } else {
                    Mbc1BankingMode::Rom
                };
            }
            _ => unreachable!("the address bus only routes ROM addresses here, got {address:#X}"),
        }
    }

    /// Reads `ram` at `address` (relative to 0xA000), or 0xFF while the RAM
    /// is disabled.
    pub(super) fn read_ram(&self, ram: &[u8], address: u16) -> u8 {
        if self.ram_enabled {
            banked_read(ram, RAM_BANK_SIZE, address, u16::from(self.ram_bank()))
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
                u16::from(self.ram_bank()),
                value,
            )
    }
}

#[cfg(test)]
mod tests {
    use crate::address_bus::cartridge::test_utils::*;
    use crate::address_bus::cartridge::*;
    use crate::address_bus::constants::*;
    use crate::address_bus::effects::empty_cartridge_effects;
    use crate::address_bus::test_utils::*;

    #[test]
    fn enable_external_ram_if_correct_cartridge_type() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC1_RAM, ROM_SIZE_64KB, RAM_SIZE_2KB);

        mapper.write_rom(0x0000, 0xA);

        mapper.write_ram(0x0000, 0xC2);
        assert_eq!(mapper.read_ram(0x000), 0xC2);
    }

    #[test]
    fn enable_external_ram_if_correct_cartridge_type_scenario_two() {
        let mut mapper =
            build_cartridge_mapper(CART_TYPE_MBC1_RAM_BATTERY, ROM_SIZE_64KB, RAM_SIZE_2KB);

        mapper.write_rom(0x0000, 0xA);

        mapper.write_ram(0x0000, 0xC2);
        assert_eq!(mapper.read_ram(0x000), 0xC2);
    }

    #[test]
    fn enable_external_ram_if_lower_nibble_is_equal_to_a() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC1_RAM, ROM_SIZE_64KB, RAM_SIZE_2KB);

        mapper.write_rom(0x0000, 0x1A);

        mapper.write_ram(0x0000, 0xC2);
        assert_eq!(mapper.read_ram(0x000), 0xC2);
    }

    #[test]
    fn not_enable_external_ram_if_incorrect_cartridge_type() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC1, ROM_SIZE_64KB, RAM_SIZE_2KB);

        mapper.write_rom(0x0000, 0xA);

        mapper.write_ram(0x0000, 0xC2);
        assert_eq!(mapper.read_ram(0x000), 0xFF);
    }

    #[test]
    fn disable_external_ram_if_correct_cartridge_type() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC1_RAM, ROM_SIZE_64KB, RAM_SIZE_2KB);

        mapper.write_rom(0x0000, 0xA);

        mapper.write_ram(0x0000, 0xC2);
        assert_eq!(mapper.read_ram(0x000), 0xC2);

        mapper.write_rom(0x0000, 0xB);

        mapper.write_ram(0x0000, 0xD2);
        assert_eq!(mapper.read_ram(0x000), 0xFF);
    }

    #[test]
    fn set_rom_bank_number() {
        let mut rom = build_rom(CART_TYPE_MBC1, ROM_SIZE_128KB, RAM_SIZE_2KB);
        rom[0xC005] = 0xA1;
        let mut mapper = load_cartridge(rom, empty_cartridge_effects()).unwrap();

        mapper.write_rom(0x2000, 0x3);

        let byte = mapper.read_rom(0x4005);
        assert_eq!(byte, 0xA1);
    }

    #[test]
    fn correctly_sets_lower_and_upper_bits_of_the_rom_bank_number() {
        let mut rom = build_rom(CART_TYPE_MBC1, ROM_SIZE_2MB, RAM_SIZE_2KB);
        rom[0x0011_0005] = 0xA1;
        let mut mapper = load_cartridge(rom, empty_cartridge_effects()).unwrap();

        mapper.write_rom(0x4000, 0x02);
        mapper.write_rom(0x2000, 0x4);

        let byte = mapper.read_rom(0x4005);
        assert_eq!(byte, 0xA1);
    }

    #[test]
    fn reads_bank_zero_as_bank_one() {
        let mut rom = build_rom(CART_TYPE_MBC1, ROM_SIZE_64KB, RAM_SIZE_2KB);
        rom[0x4005] = 0xCC;
        let mut mapper = load_cartridge(rom, empty_cartridge_effects()).unwrap();

        mapper.write_rom(0x2000, 0x0);

        let byte = mapper.read_rom(0x4005);
        assert_eq!(byte, 0xCC);
    }

    #[test]
    fn masks_bank_number_to_required_number_of_bits() {
        let mut rom = build_rom(CART_TYPE_MBC1, ROM_SIZE_256KB, RAM_SIZE_2KB);
        rom[0] = 0xB1;
        rom[1] = 0xD2;
        rom[0x8000] = 0xBB;
        rom[0x8001] = 0xD1;
        let mut mapper = load_cartridge(rom, empty_cartridge_effects()).unwrap();

        mapper.write_rom(0x2000, 0x12);

        let byte = mapper.read_rom(0x4001);
        assert_eq!(byte, 0xD1);
    }

    #[test]
    fn sets_ram_bank_number() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC1_RAM, ROM_SIZE_64KB, RAM_SIZE_32KB);

        // Enable RAM
        mapper.write_rom(0x0000, 0xA);

        // Switch to RAM mode
        mapper.write_rom(0x6000, 0x1);

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

    #[test]
    fn only_allow_reading_from_ram_if_it_is_enabled() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC1_RAM, ROM_SIZE_64KB, RAM_SIZE_32KB);

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
    fn maps_upper_bank_bits_into_the_lower_rom_area_in_mode_one() {
        let mut rom = build_rom(CART_TYPE_MBC1, ROM_SIZE_2MB, RAM_SIZE_2KB);
        rom[0x0010_0005] = 0xA1;
        let mut mapper = load_cartridge(rom, empty_cartridge_effects()).unwrap();

        mapper.write_rom(0x4000, 0x02);
        assert_eq!(mapper.read_rom(0x0005), 0x00);

        mapper.write_rom(0x6000, 0x01);
        assert_eq!(mapper.read_rom(0x0005), 0xA1);
    }

    #[test]
    fn wraps_ram_banks_beyond_the_ram_size() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC1_RAM, ROM_SIZE_64KB, RAM_SIZE_8KB);

        mapper.write_rom(0x0000, 0xA);
        mapper.write_rom(0x6000, 0x1);
        mapper.write_rom(0x4000, 0x3);
        mapper.write_ram(0x0005, 0xCC);

        mapper.write_rom(0x4000, 0x0);
        assert_eq!(mapper.read_ram(0x0005), 0xCC);
    }

    #[test]
    fn uses_four_bit_bank_numbers_on_multicarts() {
        let mut rom = build_rom(CART_TYPE_MBC1, ROM_SIZE_1MB, RAM_SIZE_2KB);
        for game_start in [0x40000, 0x80000, 0xC0000] {
            rom[game_start + 0x104] = 0xCE;
        }
        rom[0x104] = 0xCE;
        rom[0x44005] = 0xA1;
        let mut mapper = load_cartridge(rom, empty_cartridge_effects()).unwrap();

        mapper.write_rom(0x4000, 0x01);
        mapper.write_rom(0x2000, 0x01);

        assert_eq!(mapper.read_rom(0x4005), 0xA1);
    }
}
