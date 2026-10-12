//! Reading and writing banked cartridge memory.
//!
//! A cartridge holds more ROM and RAM than the console can address, so it shows
//! one bank (a fixed-size slice) at a time. Bank numbers beyond the end of the
//! memory wrap around, like the unused high address lines on real cartridges.

fn banked_index(memory: &[u8], bank_size: usize, address: u16, bank: u16) -> usize {
    let bank_start = usize::from(bank) * bank_size;
    let offset = usize::from(address) & (bank_size - 1);
    let index = bank_start + offset;
    // Dividing is slow, and banks that exist need no wrapping.
    if index < memory.len() {
        index
    } else {
        index % memory.len()
    }
}

/// Reads the byte at `address` within `bank`, where `bank_size` is a power of
/// two. Returns 0xFF, the value of an open bus, when `memory` is empty.
pub(super) fn banked_read(memory: &[u8], bank_size: usize, address: u16, bank: u16) -> u8 {
    if memory.is_empty() {
        return 0xFF;
    }
    memory[banked_index(memory, bank_size, address, bank)]
}

/// Writes the byte at `address` within `bank`, where `bank_size` is a power of
/// two. Does nothing when `memory` is empty. Returns whether the byte was
/// stored.
pub(super) fn banked_write(
    memory: &mut [u8],
    bank_size: usize,
    address: u16,
    bank: u16,
    value: u8,
) -> bool {
    if memory.is_empty() {
        return false;
    }
    let index = banked_index(memory, bank_size, address, bank);
    memory[index] = value;
    true
}
