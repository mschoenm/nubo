use crate::utils::is_bit_set;

pub(super) fn bounded_wrapping_add(original_value: u8, max_value: u8) -> u8 {
    let mut new_value = original_value + 1;
    if new_value > max_value {
        new_value = 0;
    }
    new_value
}

pub(super) fn as_dac_output(dac_input: f32) -> f32 {
    (dac_input / 7.5) - 1.0
}

const LENGTH_ENABLED_INDEX: u8 = 6;

pub(super) fn length_enabled(register_value_with_length: u8) -> bool {
    is_bit_set(register_value_with_length, LENGTH_ENABLED_INDEX)
}
