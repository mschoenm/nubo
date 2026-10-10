const T_CYCLE_INCREMENT: u8 = 4;

pub(super) fn is_bit_set(byte: u8, bit_index: u8) -> bool {
    let mask = 1 << bit_index;
    (mask & byte) > 0
}

pub(super) fn get_bit(byte: u8, bit_index: u8) -> u8 {
    let is_set = is_bit_set(byte, bit_index);
    if is_set { 1 } else { 0 }
}

pub(super) fn set_bit(byte: u8, bit_index: u8) -> u8 {
    let mask: u8 = 1 << bit_index;
    byte | mask
}

pub(super) fn reset_bit(byte: u8, bit_index: u8) -> u8 {
    let mask: u8 = !(1 << bit_index);
    byte & mask
}

pub(super) fn as_word(low_byte: u8, high_byte: u8) -> u16 {
    ((high_byte as u16) << 8) | low_byte as u16
}

pub(super) fn as_bytes(word: u16) -> (u8, u8) {
    let low_byte = (word & 0xFF) as u8;
    let high_byte = (word >> 8) as u8;
    (low_byte, high_byte)
}

pub(super) fn get_t_cycle_increment(double_speed_mode: bool) -> u8 {
    if double_speed_mode { T_CYCLE_INCREMENT / 2 } else { T_CYCLE_INCREMENT }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_get_bit() {
       let byte = 0b10000101 as u8;
       assert_eq!(get_bit(byte, 2), 0x1);
    }
    
    #[test]
    fn should_get_bit_scenario_two() {
        let byte = 0b10000001 as u8;
        assert_eq!(get_bit(byte, 2), 0x0);
    }
    
    #[test]
    fn should_combine_bytes_to_word() {
        let low_byte = 0xA1 as u8;
        let high_byte = 0x2B as u8;
        assert_eq!(as_word(low_byte, high_byte), 0x2BA1);
    }
    
    #[test]
    fn should_split_word_into_bytes() {
        let word = 0x2BA1 as u16;
        assert_eq!(as_bytes(word), (0xA1, 0x2B));
    }

    #[test]
    fn should_get_t_cycle_increment_in_normal_speed_mode() {
        let double_speed_mode = false;
        assert_eq!(get_t_cycle_increment(double_speed_mode), 4);
    }

    #[test]
    fn should_get_t_cycle_increment_in_double_speed_mode() {
        let double_speed_mode = true;
        assert_eq!(get_t_cycle_increment(double_speed_mode), 2);
    }
}