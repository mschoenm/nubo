pub(super) fn banked_read(rom: &Vec<u8>, bank_size: u32, address: u16, bank: u16) -> u8 {
    let base_location = bank as u32 * bank_size;
    let calculated_address = base_location + ((address as u32 & (bank_size - 1)) as u32);
    rom[calculated_address as usize]
}

pub(super) fn banked_write(rom: &mut Vec<u8>, bank_size: u32, address: u16, bank: u16, value: u8) {
    let base_location = bank as u32 * bank_size;
    let calculated_address = base_location + ((address as u32 & (bank_size - 1)) as u32);
    rom[calculated_address as usize] = value;
}