//! Addresses in the cartridge header, cartridge type codes and bank sizes.
//!
//! Every Game Boy ROM starts with a header at 0x0100-0x014F that describes the
//! cartridge. The values here follow Pan Docs ("The Cartridge Header").

pub(crate) const SGB_SUPPORT_ADDRESS: usize = 0x146;
pub(crate) const CARTRIDGE_TYPE_ADDRESS: usize = 0x147;
pub(crate) const ROM_SIZE_ADDRESS: usize = 0x148;
pub(crate) const RAM_SIZE_ADDRESS: usize = 0x149;
pub(crate) const HEADER_END_ADDRESS: usize = 0x14F;

/// The size of the two ROM banks the console can see at once (32 KiB).
/// Smaller ROMs are padded to this size.
pub(crate) const MINIMUM_ROM_SIZE: usize = 0x8000;

/// The size of one switchable ROM bank, as seen at 0x4000-0x7FFF.
pub(crate) const ROM_BANK_SIZE: usize = 0x4000;
/// The size of one cartridge RAM bank, as seen at 0xA000-0xBFFF.
pub(crate) const RAM_BANK_SIZE: usize = 0x2000;

pub(crate) const CARTRIDGE_TYPE_ROM_ONLY: u8 = 0x0;
pub(crate) const CARTRIDGE_TYPE_MBC1: u8 = 0x1;
pub(crate) const CARTRIDGE_TYPE_MBC1_RAM: u8 = 0x2;
pub(crate) const CARTRIDGE_TYPE_MBC1_RAM_BATTERY: u8 = 0x3;
pub(crate) const CARTRIDGE_TYPE_MBC2: u8 = 0x5;
pub(crate) const CARTRIDGE_TYPE_MBC2_BATTERY: u8 = 0x6;
pub(crate) const CARTRIDGE_TYPE_ROM_RAM: u8 = 0x8;
pub(crate) const CARTRIDGE_TYPE_ROM_RAM_BATTERY: u8 = 0x9;
pub(crate) const CARTRIDGE_TYPE_MBC3_TIMER_BATTERY: u8 = 0xF;
pub(crate) const CARTRIDGE_TYPE_MBC3_TIMER_RAM_BATTERY: u8 = 0x10;
pub(crate) const CARTRIDGE_TYPE_MBC3: u8 = 0x11;
pub(crate) const CARTRIDGE_TYPE_MBC3_RAM: u8 = 0x12;
pub(crate) const CARTRIDGE_TYPE_MBC3_RAM_BATTERY: u8 = 0x13;
pub(crate) const CARTRIDGE_TYPE_MBC5: u8 = 0x19;
pub(crate) const CARTRIDGE_TYPE_MBC5_RAM: u8 = 0x1A;
pub(crate) const CARTRIDGE_TYPE_MBC5_RAM_BATTERY: u8 = 0x1B;
pub(crate) const CARTRIDGE_TYPE_MBC5_RUMBLE: u8 = 0x1C;
pub(crate) const CARTRIDGE_TYPE_MBC5_RUMBLE_RAM: u8 = 0x1D;
pub(crate) const CARTRIDGE_TYPE_MBC5_RUMBLE_RAM_BATTERY: u8 = 0x1E;
pub(crate) const CARTRIDGE_TYPE_HUC1_RAM_BATTERY: u8 = 0xFF;

pub(crate) const TITLE_START_ADDRESS: usize = 0x134;
pub(crate) const TITLE_END_ADDRESS: usize = 0x143;

/// The position of the CGB flag (0x143) within the title. Games made for the
/// Game Boy Color store 0x80 or 0xC0 there, so it is not part of their title.
pub(crate) const CGB_COMPATIBILITY_INDEX: usize = 15;

#[cfg(test)]
pub(crate) const ROM_SIZE_64KB: u8 = 0x1;
#[cfg(test)]
pub(crate) const ROM_SIZE_128KB: u8 = 0x2;
#[cfg(test)]
pub(crate) const ROM_SIZE_256KB: u8 = 0x3;
#[cfg(test)]
pub(crate) const ROM_SIZE_1MB: u8 = 0x5;
#[cfg(test)]
pub(crate) const ROM_SIZE_2MB: u8 = 0x6;
#[cfg(test)]
pub(crate) const ROM_SIZE_4MB: u8 = 0x7;
#[cfg(test)]
pub(crate) const ROM_SIZE_8MB: u8 = 0x8;

#[cfg(test)]
pub(crate) const RAM_SIZE_2KB: u8 = 0x1;
#[cfg(test)]
pub(crate) const RAM_SIZE_8KB: u8 = 0x2;
#[cfg(test)]
pub(crate) const RAM_SIZE_32KB: u8 = 0x3;
#[cfg(test)]
pub(crate) const RAM_SIZE_128KB: u8 = 0x4;
#[cfg(test)]
pub(crate) const RAM_SIZE_64KB: u8 = 0x5;
