use crate::serializable::Serializable;

use crate::utils::is_bit_set;
use getset::{CopyGetters, Setters};
use serializable_derive::Serializable;

#[derive(Debug, Serializable, CopyGetters, Setters)]
#[getset(get_copy = "pub(crate)", set = "pub(crate)")]
pub struct Envelope {
    initial_settings: u8,
    current_volume: u8,
    timer: u8
}

const ENVELOPE_DIRECTION_INDEX: u8 = 3;

impl Envelope {
    pub(super) fn new() -> Self {
        Envelope {
            initial_settings: 0,
            current_volume: 0,
            timer: 0
        }
    }

    pub(super) fn step(&mut self) {
        let initial_timer = self.initial_settings & 0b00000111;
        let is_upwards = is_bit_set(self.initial_settings, ENVELOPE_DIRECTION_INDEX);

        if initial_timer != 0 {
            if self.timer > 0 {
                self.timer -= 1
            }
        
            if self.timer == 0 {
                self.timer = initial_timer;
        
                if (self.current_volume < 0xF && is_upwards) || (self.current_volume > 0x0 && !is_upwards) {
                    if is_upwards {
                        self.current_volume += 1;
                    }
                    else {
                        self.current_volume -= 1;
                    }
                }
            }
        }
    }

    pub(super) fn reset_settings(&mut self) {
        let initial_timer = self.initial_settings & 0b00000111;
        let initial_volume = (self.initial_settings & 0b11110000) >> 4;
        self.timer = initial_timer;
        self.current_volume = initial_volume;
    }

    pub(super) fn zombie_mode_step(&mut self, new_settings: u8) {
        let old_settings = self.initial_settings;
        let old_period = old_settings & 7;
        let new_period = new_settings & 7;
        let old_direction_increase = is_bit_set(old_settings, ENVELOPE_DIRECTION_INDEX);
        let new_direction_increase = is_bit_set(new_settings, ENVELOPE_DIRECTION_INDEX);

        let direction_changed = new_direction_increase != old_direction_increase;
        let mut should_tick = new_period != 0 && old_period == 0;

        if (new_settings & 0xF) == 8 && (old_settings & 0xF) == 8 {
            should_tick = true;
        }

        if direction_changed {
            if new_direction_increase {
                if old_period == 0 {
                    self.current_volume ^= 0xF;
                } else {
                    self.current_volume = 0xE_u8.wrapping_sub(self.current_volume) & 0xF;
                }
                should_tick = false;
            } else {
                self.current_volume = 0x10_u8.wrapping_sub(self.current_volume) & 0xF;
            }
        }

        if should_tick {
            if new_direction_increase {
                self.current_volume = self.current_volume.wrapping_add(1) & 0xF;
            } else {
                self.current_volume = self.current_volume.wrapping_sub(1) & 0xF;
            }
        }
    }
}
