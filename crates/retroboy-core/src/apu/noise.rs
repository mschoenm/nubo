use crate::apu::envelope::Envelope;
use crate::apu::length::{DEFAULT_MAX_LENGTH, Length};
use crate::serializable::Serializable;
use crate::utils::is_bit_set;
use crate::apu::utils::length_enabled;
use serializable_derive::Serializable;
use getset::{CopyGetters, Getters, MutGetters, Setters};

#[derive(Debug, Serializable, CopyGetters, Setters, Getters, MutGetters)]
pub struct NoiseChannel {
    #[getset(get_copy = "pub(crate)", set = "pub(crate)")]
    enabled: bool,
    #[getset(get_copy = "pub(crate)", set = "pub(crate)")]
    dac_enabled: bool,
    #[getset(get = "pub(crate)", get_mut = "pub(crate)")]
    length: Length,
    #[getset(get = "pub(crate)", get_mut = "pub(crate)")]
    envelope: Envelope,
    #[getset(get_copy = "pub(crate)", set = "pub(crate)")]
    polynomial: u8,
    #[getset(get_copy = "pub(crate)", set = "pub(crate)")]
    lfsr: u16,
    #[getset(get_copy = "pub(crate)", set = "pub(crate)")]
    control: u8,
    #[getset(get_copy = "pub(crate)", set = "pub(crate)")]
    period_divider: u16,
    instruction_cycles: u16
}

const WIDTH_MODE_INDEX: u8 = 3;
const CONTROL_TRIGGER_INDEX: u8 = 7;

impl NoiseChannel {
    pub(super) fn new() -> Self {
        NoiseChannel {
            enabled: false,
            dac_enabled: false,
            length: Length::new(DEFAULT_MAX_LENGTH),
            envelope: Envelope::new(),
            polynomial: 0,
            lfsr: 0,
            control: 0,
            period_divider: 0,
            instruction_cycles: 0
        }
    }

    pub(super) fn reset(original_channel: &NoiseChannel, cgb_mode: bool) -> NoiseChannel {
        let mut new_channel = NoiseChannel::new();

        if !cgb_mode {
           new_channel.length = Length::reset_initial_settings(&original_channel.length); 
        }
        
        new_channel
    }

    fn calculate_period_divider(&self) -> u16 {
        let shift_amount = (self.polynomial & 0b11110000) >> 4;
        let divisor_code = self.polynomial & 0b111;
        let divisor = if divisor_code == 0 {
            8
        }
        else {
            (divisor_code as u16) << 4
        };
        divisor << shift_amount
    }

    fn calculate_next_lfsr(&self) -> u16 {
        let narrow_width = is_bit_set(self.polynomial, WIDTH_MODE_INDEX);

        let first_lfsr_bit = self.lfsr & 0b1;
        let second_lfsr_bit = (self.lfsr & 0b10) >> 1;
        let xor_result = (!(first_lfsr_bit ^ second_lfsr_bit)) & 0b1;

        let mut next_lfsr = self.lfsr | (xor_result << 15);

        if narrow_width {
            next_lfsr |= xor_result << 7;
        }

        next_lfsr >> 1
    }

    pub(super) fn step(&mut self, last_instruction_clock_cycles: u8) {
        self.instruction_cycles += last_instruction_clock_cycles as u16;
        if self.instruction_cycles >= self.period_divider {
            self.instruction_cycles = 0;
            self.period_divider = self.calculate_period_divider();
            self.lfsr = self.calculate_next_lfsr();
        }
    }

    pub(super) fn step_envelope(&mut self) {
        if self.enabled {
            self.envelope.step();
        }
    }

    pub(super) fn should_clock_length_on_enable(&self, original_control_value: u8) -> bool {
        let new_control_value = self.control;
        !length_enabled(original_control_value) && length_enabled(new_control_value)
    }

    pub(super) fn should_clock_length_on_trigger(&self) -> bool {
        self.length.at_max_length() && length_enabled(self.control)
    }

    pub(super) fn step_length(&mut self) {
        if length_enabled(self.control) {
            self.length.step();
            if self.length.timer_expired() {
                self.set_enabled(false);
            }
        }
    }

    pub(super) fn digital_output(&self) -> f32 {
        if self.enabled {
            let amplitude = (self.lfsr & 0x01) as u8;
            let current_volume = self.envelope.current_volume();
            (amplitude * current_volume) as f32
        }
        else {
            7.5
        }
    }

    pub(super) fn trigger(&mut self) {
        if self.dac_enabled {
            self.enabled = true;
        }
        self.period_divider = self.calculate_period_divider();
        self.lfsr = 0;
        self.length.reload_timer();
        self.envelope.reset_settings();
    }

    pub(super) fn should_trigger(&self) -> bool {
        is_bit_set(self.control, CONTROL_TRIGGER_INDEX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enable_noise_channel(channel: &mut NoiseChannel) {
        channel.enabled = true;
        channel.dac_enabled = true;
    }

    #[test]
    fn should_calculate_dac_output_when_amplitude_is_zero() {
        let mut channel = NoiseChannel::new();
        enable_noise_channel(&mut channel);

        channel.lfsr = 0xFFFE;
        channel.envelope.set_current_volume(0xA);

        assert_eq!(channel.digital_output(), 0.0);
    }

    #[test]
    fn should_calculate_dac_output_when_amplitude_is_one() {
        let mut channel = NoiseChannel::new();
        enable_noise_channel(&mut channel);

        channel.lfsr = 0xFFFF;
        channel.envelope.set_current_volume(0xA);

        assert_eq!(channel.digital_output(), 10.0);
    }

    #[test]
    fn should_produce_no_audio_output_if_channel_is_disabled() {
        let mut channel = NoiseChannel::new();

        channel.lfsr = 0;
        channel.envelope.set_current_volume(0xA);

        assert_eq!(channel.digital_output(), 7.5);
    }
}