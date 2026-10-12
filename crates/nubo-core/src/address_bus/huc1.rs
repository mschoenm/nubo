//! Hudson Soft's HuC1 memory bank controller. Besides ROM and RAM banking it
//! has an infrared (IR) port, used by some games to talk to other consoles or
//! toys.

use crate::address_bus::bank_utils::{banked_read, banked_write};
use crate::address_bus::constants::RAM_BANK_SIZE;
use serializable_derive::Serializable;

/// What 0xA000-0xBFFF shows, chosen by writing to 0x0000-0x1FFF.
#[derive(Debug, PartialEq, Serializable)]
pub(super) enum Huc1Mode {
    /// The cartridge RAM.
    Ram,
    /// The infrared port.
    Infrared,
}

/// The registers of a HuC1. Its only cartridge type, HuC1+RAM+BATTERY,
/// always has RAM; a header that announces none gives it an empty RAM, which
/// reads 0xFF and ignores writes.
#[derive(Debug, Serializable)]
pub(super) struct Huc1CartridgeMapper {
    mode: Huc1Mode,
    /// Whether the game has switched the infrared light on.
    is_ir_light_on: bool,
    rom_bank_number: u8,
    ram_bank_number: u8,
}

impl Huc1CartridgeMapper {
    /// The controller at power-on.
    pub(super) fn new() -> Self {
        Huc1CartridgeMapper {
            mode: Huc1Mode::Ram,
            is_ir_light_on: false,
            rom_bank_number: 1,
            ram_bank_number: 0,
        }
    }

    /// The ROM banks visible at 0x0000-0x3FFF and 0x4000-0x7FFF.
    pub(super) fn rom_banks(&self) -> [u16; 2] {
        [0, u16::from(self.rom_bank_number)]
    }

    /// The RAM bank visible at 0xA000-0xBFFF.
    pub(super) fn ram_bank(&self) -> u8 {
        self.ram_bank_number
    }

    /// Sets a register by writing to the ROM area.
    pub(super) fn write_rom(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x1FFF => {
                self.mode = if value == 0xE {
                    Huc1Mode::Infrared
                } else {
                    Huc1Mode::Ram
                };
            }
            0x2000..=0x3FFF => {
                self.rom_bank_number = value & 0x3F;
            }
            0x4000..=0x5FFF => {
                self.ram_bank_number = value & 0x03;
            }
            0x6000..=0x7FFF => (), // No behavior observed in this address range for HuC1 cartridges
            _ => unreachable!("the address bus only routes ROM addresses here, got {address:#X}"),
        }
    }

    /// Reads `ram` at `address` (relative to 0xA000), or the infrared port,
    /// where 0xC0 means no light is received.
    pub(super) fn read_ram(&self, ram: &[u8], address: u16) -> u8 {
        match self.mode {
            Huc1Mode::Ram => {
                banked_read(ram, RAM_BANK_SIZE, address, u16::from(self.ram_bank_number))
            }
            Huc1Mode::Infrared => 0xC0,
        }
    }

    /// Writes `ram` at `address` (relative to 0xA000), or switches the
    /// infrared light. Returns whether a byte of RAM was stored.
    pub(super) fn write_ram(&mut self, ram: &mut [u8], address: u16, value: u8) -> bool {
        match self.mode {
            Huc1Mode::Ram => banked_write(
                ram,
                RAM_BANK_SIZE,
                address,
                u16::from(self.ram_bank_number),
                value,
            ),
            Huc1Mode::Infrared => {
                self.is_ir_light_on = value == 0x1;
                false
            }
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
    fn enable_ir_mode() {
        let mut mapper =
            build_cartridge_mapper(CART_TYPE_HUC1_RAM_BATTERY, ROM_SIZE_64KB, RAM_SIZE_2KB);

        mapper.write_rom(0x0000, 0xE);

        assert_eq!(mapper.read_ram(0x000), 0xC0);
    }

    #[test]
    fn enable_external_ram() {
        let mut mapper =
            build_cartridge_mapper(CART_TYPE_HUC1_RAM_BATTERY, ROM_SIZE_64KB, RAM_SIZE_2KB);

        mapper.write_rom(0x0000, 0xA);

        mapper.write_ram(0x0000, 0xDD);
        assert_eq!(mapper.read_ram(0x000), 0xDD);
    }

    #[test]
    fn set_rom_bank_number() {
        let mut rom = build_rom(CART_TYPE_HUC1_RAM_BATTERY, ROM_SIZE_128KB, RAM_SIZE_2KB);
        rom[0xC005] = 0xA1;
        let mut mapper = load_cartridge(rom, empty_cartridge_effects()).unwrap();

        mapper.write_rom(0x2000, 0x3);

        let byte = mapper.read_rom(0x4005);
        assert_eq!(byte, 0xA1);
    }

    #[test]
    fn sets_ram_bank_number() {
        let mut mapper =
            build_cartridge_mapper(CART_TYPE_HUC1_RAM_BATTERY, ROM_SIZE_64KB, RAM_SIZE_32KB);

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

    #[test]
    fn ignores_ram_writes_when_the_cartridge_has_no_ram() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_HUC1_RAM_BATTERY, ROM_SIZE_64KB, 0);
        mapper.write_rom(0x0000, 0xA);

        mapper.write_ram(0x0005, 0xCC);

        assert_eq!(mapper.read_ram(0x0005), 0xFF);
    }

    #[test]
    fn ram_is_saved_once_per_report_but_not_in_infrared_mode() {
        assert_eq!(ram_saves_after_writing(CART_TYPE_HUC1_RAM_BATTERY, 0x0A), 1);
        assert_eq!(ram_saves_after_writing(CART_TYPE_HUC1_RAM_BATTERY, 0x0E), 0);
    }
}
