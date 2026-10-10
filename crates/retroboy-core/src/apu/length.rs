use crate::serializable::Serializable;
use getset::{CopyGetters, Setters};
use serializable_derive::Serializable;

#[derive(Debug, Serializable, CopyGetters, Setters)]
#[getset(get_copy = "pub(crate)", set = "pub(crate)")]
pub struct Length {
    initial_settings: u8,
    timer: u16,
    #[getset(skip)]
    max_length: u16
}

pub const WAVE_MAX_LENGTH: u16 = 256;
pub const DEFAULT_MAX_LENGTH: u16 = 64;

impl Length {
    pub(super) fn new(max_length: u16) -> Self {
        Length {
            initial_settings: 0,
            timer: 0,
            max_length
        }
    }

    pub(super) fn reset_initial_settings(original_length: &Length) -> Length {
        Length {
            initial_settings: 0,
            timer: original_length.timer,
            max_length: original_length.max_length
        }
    }

    pub(super) fn step(&mut self) {
        if self.timer > 0 {
            self.timer -= 1;
        }
    }

    pub(super) fn timer_expired(&self) -> bool {
        self.timer == 0
    }

    pub(super) fn initialize_timer(&mut self) {
        let initial_length: u16 = if self.max_length == WAVE_MAX_LENGTH {
            self.initial_settings as u16
        }
        else {
            (self.initial_settings & 0b00111111) as u16
        };
        self.timer = self.max_length - initial_length;
    }

    pub(super) fn reload_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.max_length;
        }
    }

    pub(super) fn at_max_length(&self) -> bool {
        self.timer == self.max_length
    }
}
