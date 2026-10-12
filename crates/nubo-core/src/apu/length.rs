/*
    The length counters' quirks are ported from SameBoy's apu.c
    (https://github.com/LIJI32/SameBoy, Expat License, see the notice in apu.rs).
*/

//! Length counters, which let each channel turn itself off after a set time.
//!
//! NRx1 loads a channel's length: up to 64 steps, or 256 for the wave
//! channel. While NRx4 bit 6 enables the counter, every second frame sequencer
//! step (256 times a second) counts it down, and the channel turns off when it
//! reaches 0. A trigger restarts a counter that has run out at the full
//! length. Writes to NRx4 can clock the counter once more, depending on the
//! frame sequencer's phase.

use crate::apu::{Apu, Channel, triggers};
use crate::model::Model;
use serializable_derive::Serializable;

/// One channel's length counter.
#[derive(Debug, Default, Clone, Copy, Serializable)]
pub(super) struct LengthCounter {
    /// Length steps left before the channel turns itself off, while
    /// `enabled`. 0 once it has run out.
    pub(super) remaining: u16,
    /// NRx4 bit 6: the frame sequencer counts the length down.
    pub(super) enabled: bool,
}

impl LengthCounter {
    /// Counts down on a frame sequencer length step, while enabled. Returns
    /// whether the counter ran out now, which turns the channel off.
    fn clock(&mut self) -> bool {
        if self.enabled && self.remaining > 0 {
            self.remaining -= 1;
            self.remaining == 0
        } else {
            false
        }
    }

    /// Whether the remaining length is one the counter can have.
    pub(super) fn is_valid(self, full_length: u16) -> bool {
        self.remaining <= full_length
    }

    /// Whether the next frame sequencer length step runs the counter out.
    pub(super) fn runs_out_on_next_clock(self) -> bool {
        self.enabled && self.remaining == 1
    }

    /// A trigger restarts a counter that has run out at `full_length`,
    /// disabled until the NRx4 write that triggered sets bit 6 again (which
    /// lets that write's extra clock count it down at once).
    pub(super) fn restart_if_run_out(&mut self, full_length: u16) {
        if self.remaining == 0 {
            self.remaining = full_length;
            self.enabled = false;
        }
    }
}

impl Channel {
    /// The longest length NRx1 can load, in frame sequencer length steps: 64
    /// (a quarter of a second), or 256 for the wave channel.
    pub(super) const fn full_length(self) -> u16 {
        match self {
            Channel::Wave => 256,
            Channel::Square1 | Channel::Square2 | Channel::Noise => 64,
        }
    }
}

impl Apu {
    pub(super) fn length_counter(&self, channel: Channel) -> &LengthCounter {
        match channel {
            Channel::Square1 => &self.state.square1.length,
            Channel::Square2 => &self.state.square2.length,
            Channel::Wave => &self.state.wave.length,
            Channel::Noise => &self.state.noise.length,
        }
    }

    fn length_counter_mut(&mut self, channel: Channel) -> &mut LengthCounter {
        match channel {
            Channel::Square1 => &mut self.state.square1.length,
            Channel::Square2 => &mut self.state.square2.length,
            Channel::Wave => &mut self.state.wave.length,
            Channel::Noise => &mut self.state.noise.length,
        }
    }

    /// Loads a channel's length from the length bits of an NRx1 write: the
    /// counter runs that many steps short of the full length.
    pub(super) fn load_length(&mut self, channel: Channel, nrx1: u8) {
        let full_length = channel.full_length();
        self.length_counter_mut(channel).remaining =
            full_length - (u16::from(nrx1) & (full_length - 1));
    }

    /// Restarts a channel's length counter for a trigger, if it has run out.
    pub(super) fn restart_length_if_run_out(&mut self, channel: Channel) {
        let full_length = channel.full_length();
        self.length_counter_mut(channel)
            .restart_if_run_out(full_length);
    }

    /// Runs a frame sequencer length step: counts every channel's length
    /// down and turns off the channels whose length runs out.
    pub(super) fn clock_length_counters(&mut self) {
        for channel in Channel::ALL {
            if self.length_counter_mut(channel).clock() {
                if channel == Channel::Wave && self.state.is_active(Channel::Wave) && self.is_agb()
                {
                    self.latch_agb_wave_sample_on_length_expiry();
                }
                self.silence(channel);
            }
        }
    }

    /// Applies an NRx4 write's length enable bit (6), after the trigger it
    /// may hold. A write that enables the counter while the next frame
    /// sequencer step won't clock it clocks it once more at once (always on
    /// CGB-B and older, see `nrx4_clocks_length`). When that runs a
    /// triggered channel's length out, it restarts one step short of the full
    /// length; otherwise the channel turns off. On CGB-B the wave channel only
    /// turns off on the next such write, with its counter still at 0.
    pub(super) fn write_nrx4_length(&mut self, channel: Channel, nrx4: u8) {
        let triggered = triggers(nrx4);
        let full_length = channel.full_length();
        let turns_off_late = channel == Channel::Wave && self.model == Model::CgbB;
        let extra_clock = self.nrx4_clocks_length(nrx4) && self.state.frame_sequencer_step & 1 != 0;

        let counter = self.length_counter_mut(channel);
        let ran_out = if extra_clock && !counter.enabled {
            if counter.remaining > 0 {
                counter.remaining -= 1;
                if counter.remaining == 0 && triggered {
                    counter.remaining = full_length - 1;
                }
                counter.remaining == 0 && !turns_off_late
            } else {
                turns_off_late
            }
        } else {
            false
        };
        counter.enabled = nrx4 & 0x40 != 0;
        if ran_out {
            self.silence(channel);
        }
    }

    /// Whether an NRx4 write can clock the length counter: when it enables the
    /// length counter, or always on CGB-B and older.
    fn nrx4_clocks_length(&self, nrx4: u8) -> bool {
        nrx4 & 0x40 != 0 || (self.cgb_mode && self.model <= Model::CgbB)
    }
}
