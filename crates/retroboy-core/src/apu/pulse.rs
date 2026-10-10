use crate::apu::envelope::Envelope;
use crate::apu::length::{DEFAULT_MAX_LENGTH, Length};
use crate::apu::period::Period;
use crate::apu::sweep::Sweep;
use crate::apu::utils::{bounded_wrapping_add, length_enabled};
use crate::serializable::Serializable;
use crate::utils::{get_bit, is_bit_set};
use serializable_derive::Serializable;
use getset::{CopyGetters, Getters, MutGetters, Setters};

#[derive(Debug, Serializable, CopyGetters, Setters, Getters, MutGetters)]
pub struct PulseChannel {
    #[getset(get_copy = "pub(super)", set = "pub(super)")]
    enabled: bool,
    #[getset(get_copy = "pub(super)", set = "pub(super)")]
    dac_enabled: bool,
    #[getset(get_copy = "pub(super)", set = "pub(super)")]
    wave_duty_position: u8,
    #[getset(get = "pub(crate)", get_mut = "pub(crate)")]
    sweep: Sweep,
    #[getset(get = "pub(crate)", get_mut = "pub(crate)")]
    length: Length,
    #[getset(get = "pub(crate)", get_mut = "pub(crate)")]
    envelope: Envelope,
    #[getset(get = "pub(crate)", get_mut = "pub(crate)")]
    period: Period,
}

const MAX_WAVEFORM_STEPS: u8 = 7;
const PERIOD_HIGH_TRIGGER_INDEX: u8 = 7;

const WAVEFORMS: [u8; 4] = [
    0b00000001,
    0b00000011,
    0b00001111,
    0b11111100
];

impl PulseChannel {
    pub(super) fn new() -> Self {
        PulseChannel {
            enabled: false,
            dac_enabled: false,
            wave_duty_position: 0,
            sweep: Sweep::new(),
            length: Length::new(DEFAULT_MAX_LENGTH),
            envelope: Envelope::new(),
            period: Period::new(),
        }
    }

    pub(super) fn reset(original_channel: &PulseChannel, cgb_mode: bool) -> PulseChannel {
        let mut new_channel = PulseChannel::new();

        if !cgb_mode {
           new_channel.length = Length::reset_initial_settings(&original_channel.length); 
        }
        
        new_channel
    }

    pub(super) fn step(&mut self, last_instruction_clock_cycles: u8) {
        if self.enabled {
            self.period.step(last_instruction_clock_cycles / 4, || {
                self.wave_duty_position = bounded_wrapping_add(self.wave_duty_position, MAX_WAVEFORM_STEPS);
            });
        }
    }

    pub(super) fn step_envelope(&mut self) {
        if self.enabled {
            self.envelope.step();
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

    pub(super) fn digital_output(&self) -> f32 {
        if self.enabled {
            let length_settings = self.length.initial_settings();
            let wave_duty = (length_settings & 0b11000000) >> 6;
            let waveform = WAVEFORMS[wave_duty as usize];
            let amplitude = get_bit(waveform, self.wave_duty_position);
            let current_volume = self.envelope.current_volume();
            (amplitude * current_volume) as f32
        }
        else {
            7.5
        }
    }

    pub(super) fn step_sweep(&mut self) {
        if self.enabled {
            self.sweep.step(&mut self.period);
            if self.sweep.should_disable_channel() {
                self.set_enabled(false);
            }
        }
    }

    pub(super) fn trigger(&mut self, with_sweep: bool) {
        if self.dac_enabled {
            self.enabled = true;
        }
        self.period.trigger();
        self.length.reload_timer();
        self.envelope.reset_settings();
        if with_sweep {
            self.sweep.trigger(&self.period);
            if self.sweep.should_disable_channel() {
                self.set_enabled(false);
            }
        }
    }

    pub(super) fn should_trigger(&self) -> bool {
        is_bit_set(self.period.high(), PERIOD_HIGH_TRIGGER_INDEX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enable_pulse_channel(channel: &mut PulseChannel) {
        channel.enabled = true;
        channel.dac_enabled = true;
    }

    fn initialize_amplitude_variables(channel: &mut PulseChannel,
        wave_duty: u8,
        wave_duty_position: u8,
        current_volume: u8) {
        channel.length.set_initial_settings(wave_duty << 6);
        channel.wave_duty_position = wave_duty_position;
        channel.envelope.set_current_volume(current_volume);
    }

    #[test]
    fn should_calculate_dac_output_when_amplitude_is_zero() {
        let mut channel = PulseChannel::new();
        enable_pulse_channel(&mut channel);

        let wave_duty = 1;
        let wave_duty_position = 2;
        let current_volume = 5;
        initialize_amplitude_variables(&mut channel, wave_duty, wave_duty_position, current_volume);

        assert_eq!(channel.digital_output(), 0.0);
    }

    #[test]
    fn should_calculate_dac_output_when_amplitude_is_one() {
        let mut channel = PulseChannel::new();
        enable_pulse_channel(&mut channel);

        let wave_duty = 1;
        let wave_duty_position = 1;
        let current_volume = 5;
        initialize_amplitude_variables(&mut channel, wave_duty, wave_duty_position, current_volume);

        assert_eq!(channel.digital_output(), 5.0);
    }

    #[test]
    fn should_calculate_dac_output_when_volume_is_at_ten() {
        let mut channel = PulseChannel::new();
        enable_pulse_channel(&mut channel);

        let wave_duty = 2;
        let wave_duty_position = 2;
        let current_volume = 10;
        initialize_amplitude_variables(&mut channel, wave_duty, wave_duty_position, current_volume);

        assert_eq!(channel.digital_output(), 10.0);
    }

    #[test]
    fn should_produce_no_audio_output_if_channel_is_disabled() {
        let mut channel = PulseChannel::new();

        let wave_duty = 2;
        let wave_duty_position = 2;
        let current_volume = 10;
        initialize_amplitude_variables(&mut channel, wave_duty, wave_duty_position, current_volume);

        assert_eq!(channel.digital_output(), 7.5);
    }
}