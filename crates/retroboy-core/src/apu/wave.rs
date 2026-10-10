use crate::apu::period::Period;
use crate::apu::length::{WAVE_MAX_LENGTH, Length};
use crate::apu::utils::{bounded_wrapping_add, length_enabled};
use crate::serializable::Serializable;
use crate::utils::is_bit_set;
use serializable_derive::Serializable;
use getset::{CopyGetters, Getters, MutGetters, Setters};

#[derive(Debug, Serializable, CopyGetters, Setters, Getters, MutGetters)]
pub struct WaveChannel {
    #[getset(get_copy = "pub(crate)", set = "pub(crate)")]
    enabled: bool,
    #[getset(get_copy = "pub(crate)", set = "pub(crate)")]
    dac_enabled: bool,
    #[getset(get = "pub(crate)", get_mut = "pub(crate)")]
    length: Length,
    #[getset(get_copy = "pub(crate)", set = "pub(crate)")]
    volume: u8,
    #[getset(get = "pub(crate)", get_mut = "pub(crate)")]
    period: Period,
    #[getset(get_copy = "pub(crate)", set = "pub(crate)")]
    wave_position: u8,
    wave_pattern_ram: [u8; 0x10],
}

const MAX_WAVE_SAMPLE_STEPS: u8 = 31;
const PERIOD_HIGH_TRIGGER_INDEX: u8 = 7;

impl WaveChannel {
    pub(super) fn new() -> Self {
        WaveChannel {
            enabled: false,
            dac_enabled: false,
            length: Length::new(WAVE_MAX_LENGTH),
            volume: 0,
            period: Period::new(),
            wave_position: 1,
            wave_pattern_ram: [0; 0x10],
        }
    }

    pub(super) fn reset(original_channel: &WaveChannel, cgb_mode: bool) -> WaveChannel {
        let mut new_channel = WaveChannel::new();

        if !cgb_mode {
           new_channel.length = Length::reset_initial_settings(&original_channel.length); 
        }

        new_channel.wave_pattern_ram = original_channel.wave_pattern_ram;
        new_channel.wave_position = original_channel.wave_position;
        
        new_channel
    }

    pub(super) fn step(&mut self, last_instruction_clock_cycles: u8) {
        if self.enabled {
            self.period.step(last_instruction_clock_cycles / 2, || {
                self.wave_position = bounded_wrapping_add(self.wave_position, MAX_WAVE_SAMPLE_STEPS);
            });
        }
    }

    pub(super) fn should_clock_length_on_enable(&self, original_period_high_value: u8) -> bool {
        let new_period_high_value = self.period.high();
        !length_enabled(original_period_high_value) &&
            length_enabled(new_period_high_value)
    }

    pub(super) fn should_clock_length_on_trigger(&self) -> bool {
        let period_high = self.period.high();
        self.length.at_max_length() && length_enabled(period_high)
    }

    pub(super) fn step_length(&mut self) {
        let period_high = self.period.high();
        let length_timer_enabled = length_enabled(period_high);
        if length_timer_enabled {
            self.length.step();
            if self.length.timer_expired() {
                self.set_enabled(false);
            }
        }
    }

    pub(super) fn read_from_wave_ram(&self, localized_address: u8) -> u8 {
        self.wave_pattern_ram[localized_address as usize]
    }

    pub(crate) fn write_to_wave_ram(&mut self, localized_address: u8, new_value: u8) {
        self.wave_pattern_ram[localized_address as usize] = new_value;
    }

    pub(super) fn digital_output(&self) -> f32 {
        if self.enabled {
            let localized_address = self.wave_position / 2;
            let byte_offset = self.wave_position % 2;

            let byte = self.read_from_wave_ram(localized_address);
            let sample = if byte_offset == 0 { (byte & 0xF0) >> 4 } else { byte & 0xF };

            let output_level = (self.volume & 0b01100000) >> 5;
            match output_level {
                0b01 => sample as f32,
                0b10 => (sample >> 1) as f32,
                0b11 => (sample >> 2) as f32,
                _ => 7.5
            }
        }
        else {
            7.5
        }
    }

    fn corrupt_wave_ram_bug(&mut self) {
        // DMG has a bug that will corrupt wave RAM if the channel is re-triggered
        // right before it reads from wave RAM.
        let offset = (((self.wave_position + 1) >> 1) & 0xF) as usize;
        if offset < 4 {
            self.wave_pattern_ram[0] = self.wave_pattern_ram[offset];
        }
        else {
            let copy_base_position = offset & !3;
            for copy_offset in 0..=3 {
                let copy_position = copy_base_position + copy_offset;
                self.wave_pattern_ram[copy_offset] = self.wave_pattern_ram[copy_position];
            }
        } 
    }

    pub(super) fn trigger(&mut self, cgb_mode: bool) {
        let period_divider = self.period.divider();
        if self.enabled && period_divider == 1 && !cgb_mode {
            self.corrupt_wave_ram_bug();
        }

        self.wave_position = 0;

        if self.dac_enabled {
            self.enabled = true;
        }

        self.period.trigger();
        self.period.apply_wave_channel_trigger_delay();
        self.length.reload_timer();
    }

    pub(super) fn should_trigger(&self) -> bool {
        is_bit_set(self.period.high(), PERIOD_HIGH_TRIGGER_INDEX)
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn enable_wave_channel(channel: &mut WaveChannel) {
        channel.enabled = true;
        channel.dac_enabled = true;
    }

    #[test]
    fn should_calculate_dac_output_when_amplitude_is_zero() {
        let mut channel = WaveChannel::new();
        enable_wave_channel(&mut channel);

        channel.wave_pattern_ram[0] = 0xAC;
        channel.wave_pattern_ram[1] = 0xC0;
        channel.wave_pattern_ram[2] = 0x04;
        channel.wave_pattern_ram[3] = 0xDC;

        channel.wave_position = 3;
        channel.volume = 0b00100000;

        assert_eq!(channel.digital_output(), 0.0);
    }

    #[test]
    fn should_calculate_dac_output_when_amplitude_is_non_zero() {
        let mut channel = WaveChannel::new();
        enable_wave_channel(&mut channel);

        channel.wave_pattern_ram[0] = 0xAC;
        channel.wave_pattern_ram[1] = 0xC0;
        channel.wave_pattern_ram[2] = 0x04;
        channel.wave_pattern_ram[3] = 0xDC;

        channel.wave_position = 5;
        channel.volume = 0b00100000;

        assert_eq!(channel.digital_output(), 4.0); 
    }

    #[test]
    fn should_generate_no_sound_if_channel_is_muted() {
        let mut channel = WaveChannel::new();
        enable_wave_channel(&mut channel);

        channel.wave_pattern_ram[0] = 0xAC;
        channel.wave_pattern_ram[1] = 0xC0;
        channel.wave_pattern_ram[2] = 0x04;
        channel.wave_pattern_ram[3] = 0xDC;

        channel.wave_position = 5;
        channel.volume = 0;

        assert_eq!(channel.digital_output(), 7.5); 
    }

    #[test]
    fn should_shift_sample_right_once_if_channel_is_set_to_half_of_volume() {
        let mut channel = WaveChannel::new();
        enable_wave_channel(&mut channel);

        channel.wave_pattern_ram[0] = 0xAC;
        channel.wave_pattern_ram[1] = 0xC0;
        channel.wave_pattern_ram[2] = 0x04;
        channel.wave_pattern_ram[3] = 0xDC;

        channel.wave_position = 5;
        channel.volume = 0b01000000;

        assert_eq!(channel.digital_output(), 2.0); 
    }

    #[test]
    fn should_produce_no_audio_output_if_channel_is_disabled() {
        let mut channel = WaveChannel::new();

        channel.wave_pattern_ram[0] = 0xAC;
        channel.wave_pattern_ram[1] = 0xC0;
        channel.wave_pattern_ram[2] = 0x04;
        channel.wave_pattern_ram[3] = 0xDC;

        channel.wave_position = 5;
        channel.volume = 0b01000000;

        assert_eq!(channel.digital_output(), 7.5); 
    }
}