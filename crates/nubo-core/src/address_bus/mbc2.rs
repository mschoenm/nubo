//! The MBC2 memory bank controller, which has up to 256 KiB of ROM and a
//! small RAM built into the chip.
//!
//! The built-in RAM holds 512 half-bytes (4-bit values). It repeats across the
//! whole 0xA000-0xBFFF area, and the upper four bits of each byte read as 1s.

use serializable_derive::Serializable;

/// The number of half-byte cells in MBC2's built-in RAM.
pub(super) const MBC2_RAM_SIZE: usize = 0x200;

/// The registers of an MBC2.
#[derive(Debug, Serializable)]
pub(super) struct Mbc2CartridgeMapper {
    ram_enabled: bool,
    /// The ROM bank visible at 0x4000-0x7FFF, from 1 to 15.
    rom_bank_number: u8,
}

fn ram_index(address: u16) -> usize {
    usize::from(address) & (MBC2_RAM_SIZE - 1)
}

impl Mbc2CartridgeMapper {
    /// The controller at power-on.
    pub(super) fn new() -> Self {
        Mbc2CartridgeMapper {
            ram_enabled: false,
            rom_bank_number: 1,
        }
    }

    /// The ROM banks visible at 0x0000-0x3FFF and 0x4000-0x7FFF.
    pub(super) fn rom_banks(&self) -> [u16; 2] {
        [0, u16::from(self.rom_bank_number)]
    }

    /// Sets a register by writing to the ROM area. MBC2 has one register
    /// area, 0x0000-0x3FFF. Bit 8 of the address picks the register: clear
    /// enables RAM, set selects the ROM bank.
    pub(super) fn write_rom(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x3FFF if address & 0x100 == 0 => {
                self.ram_enabled = (value & 0xF) == 0x0A;
            }
            0x0000..=0x3FFF => {
                let bank = value & 0xF;
                self.rom_bank_number = if bank == 0 { 1 } else { bank };
            }
            0x4000..=0x7FFF => {}
            _ => unreachable!("the address bus only routes ROM addresses here, got {address:#X}"),
        }
    }

    /// Reads the half-byte cell for `address`, with the upper four bits set,
    /// or 0xFF while the RAM is disabled or when `ram` has no such cell.
    pub(super) fn read_ram(&self, ram: &[u8], address: u16) -> u8 {
        match ram.get(ram_index(address)) {
            Some(value) if self.ram_enabled => value | 0xF0,
            _ => 0xFF,
        }
    }

    /// Stores the lower four bits of `value` in the cell for `address` if the
    /// RAM is enabled. Returns whether they were stored.
    pub(super) fn write_ram(&mut self, ram: &mut [u8], address: u16, value: u8) -> bool {
        if !self.ram_enabled {
            return false;
        }
        match ram.get_mut(ram_index(address)) {
            Some(cell) => {
                *cell = value & 0x0F;
                true
            }
            None => false,
        }
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
    fn selects_rom_bank_only_when_address_bit_eight_is_set() {
        let mut rom = build_rom(CART_TYPE_MBC2, ROM_SIZE_256KB, 0);
        rom[0xC005] = 0xA1;
        let mut mapper = load_cartridge(rom, empty_cartridge_effects()).unwrap();

        mapper.write_rom(0x2000, 0x03);
        assert_eq!(mapper.read_rom(0x4005), 0x00);

        mapper.write_rom(0x2100, 0x13);
        assert_eq!(mapper.read_rom(0x4005), 0xA1);
    }

    #[test]
    fn stores_only_the_lower_nibble_and_repeats_ram_across_the_area() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC2, ROM_SIZE_256KB, 0);
        mapper.write_rom(0x0000, 0x0A);

        mapper.write_ram(0xA005, 0x3C);

        assert_eq!(mapper.read_ram(0xA005), 0xFC);
        assert_eq!(mapper.read_ram(0xA205), 0xFC);
        assert_eq!(mapper.read_ram(0xBE05), 0xFC);
    }

    #[test]
    fn reads_ff_while_ram_is_disabled() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC2, ROM_SIZE_256KB, 0);
        mapper.write_rom(0x0000, 0x0A);
        mapper.write_ram(0xA005, 0x03);

        mapper.write_rom(0x0000, 0x00);

        assert_eq!(mapper.read_ram(0xA005), 0xFF);
    }

    #[test]
    fn battery_backed_ram_is_saved_once_per_report_while_enabled() {
        assert_eq!(ram_saves_after_writing(CART_TYPE_MBC2_BATTERY, 0x0A), 1);
        assert_eq!(ram_saves_after_writing(CART_TYPE_MBC2_BATTERY, 0x00), 0);
        assert_eq!(ram_saves_after_writing(CART_TYPE_MBC2, 0x0A), 0);
    }
}
