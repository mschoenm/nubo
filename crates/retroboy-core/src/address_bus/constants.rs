pub(crate) const ENTRY_POINT_ADDRESS: usize = 0x100;
pub(crate) const SGB_SUPPORT_ADDRESS: usize = 0x146;
pub(crate) const CARTRIDGE_TYPE_ADDRESS: usize = 0x147;
pub(crate) const ROM_SIZE_ADDRESS: usize = 0x148;
pub(crate) const RAM_SIZE_ADDRESS: usize = 0x149;

pub(crate) const CART_TYPE_ROM_ONLY: u8 = 0x0;
pub(crate) const CART_TYPE_MBC1: u8 = 0x1;
pub(crate) const CART_TYPE_MBC1_WITH_RAM: u8 = 0x2;
pub(crate) const CART_TYPE_MBC1_WITH_RAM_PLUS_BATTERY: u8 = 0x3;
pub(crate) const CART_TYPE_MBC3_TIMER_BATTERY: u8 = 0xF;
pub(crate) const CART_TYPE_MBC3_TIMER_RAM_BATTERY: u8 = 0x10;
pub(crate) const CART_TYPE_MBC3: u8 = 0x11;
pub(crate) const CART_TYPE_MBC3_RAM: u8 = 0x12;
pub(crate) const CART_TYPE_MBC3_RAM_BATTERY: u8 = 0x13;
pub(crate) const CART_TYPE_MBC5: u8 = 0x19;
pub(crate) const CART_TYPE_MBC5_RAM: u8 = 0x1A;
pub(crate) const CART_TYPE_MBC5_RAM_BATTERY: u8 = 0x1B;
pub(crate) const CART_TYPE_MBC5_RUMBLE: u8 = 0x1C;
pub(crate) const CART_TYPE_MBC5_RUMBLE_RAM: u8 = 0x1D;
pub(crate) const CART_TYPE_MBC5_RUMBLE_RAM_BATTERY: u8 = 0x1E;
pub(crate) const CART_TYPE_HUC1_RAM_BATTERY: u8 = 0xFF;

pub(crate) const TITLE_START_ADDRESS: usize = 0x134;
pub(crate) const TITLE_END_ADDRESS: usize = 0x143;

pub(crate) const CGB_COMPATABILITY_INDEX: usize = 15;

#[cfg(test)]
pub(crate) const ROM_SIZE_64KB: u8 = 0x1;
#[cfg(test)]
pub(crate) const ROM_SIZE_128KB: u8 = 0x2;
#[cfg(test)]
pub(crate) const ROM_SIZE_256KB: u8 = 0x3;
#[cfg(test)]
pub(crate) const ROM_SIZE_2MB: u8 = 0x6;
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