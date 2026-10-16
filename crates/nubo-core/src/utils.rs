//! Small bit and byte helpers shared by the whole emulator core.

/// How many times the bit `mask` of a 16-bit counter falls from 1 to 0 while the counter counts up
/// `steps` times by `step` from `start`, wrapping around. `step` must be at most `mask`, so no step
/// skips a whole period of the bit.
pub(super) fn falling_edges(start: u16, step: u16, steps: u32, mask: u16) -> u32 {
    // The bit falls each time the counter passes a multiple of twice the bit.
    let period_shift = mask.trailing_zeros() + 1;
    let start = u64::from(start);
    let end = start + u64::from(step) * u64::from(steps);
    u32::try_from((end >> period_shift) - (start >> period_shift)).expect("fewer edges than steps")
}

pub(super) fn is_bit_set(byte: u8, bit_index: u8) -> bool {
    let mask = 1 << bit_index;
    (mask & byte) > 0
}

/// Returns bit `bit_index` of `byte` as 0 or 1.
#[cfg(test)]
pub(super) fn bit_value(byte: u8, bit_index: u8) -> u8 {
    u8::from(is_bit_set(byte, bit_index))
}

pub(super) fn set_bit(byte: u8, bit_index: u8) -> u8 {
    let mask: u8 = 1 << bit_index;
    byte | mask
}

pub(super) fn reset_bit(byte: u8, bit_index: u8) -> u8 {
    let mask: u8 = !(1 << bit_index);
    byte & mask
}

/// Combines two bytes into a 16-bit word. The Game Boy stores words
/// little-endian, so the low byte comes first.
pub(super) fn as_word(low_byte: u8, high_byte: u8) -> u16 {
    (u16::from(high_byte) << 8) | u16::from(low_byte)
}

/// Splits a 16-bit word into its `(low, high)` bytes.
pub(super) fn as_bytes(word: u16) -> (u8, u8) {
    let low_byte = (word & 0xFF) as u8;
    let high_byte = (word >> 8) as u8;
    (low_byte, high_byte)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_get_bit() {
        let byte = 0b1000_0101_u8;
        assert_eq!(bit_value(byte, 2), 0x1);
    }

    #[test]
    fn should_get_bit_scenario_two() {
        let byte = 0b1000_0001_u8;
        assert_eq!(bit_value(byte, 2), 0x0);
    }

    #[test]
    fn should_combine_bytes_to_word() {
        let low_byte = 0xA1_u8;
        let high_byte = 0x2B_u8;
        assert_eq!(as_word(low_byte, high_byte), 0x2BA1);
    }

    #[test]
    fn should_split_word_into_bytes() {
        let word = 0x2BA1_u16;
        assert_eq!(as_bytes(word), (0xA1, 0x2B));
    }
}
