/*
    The volume envelope and its zombie mode glitch are ported from SameBoy's apu.c
    (https://github.com/LIJI32/SameBoy, Expat License, see the notice in apu.rs).
*/

//! Volume envelopes and the NRx2 "zombie mode" write glitch.
//!
//! Channels 1, 2 and 4 each have a volume envelope set by their NRx2 register:
//! a starting volume (bits 4-7), a direction (bit 3, set means louder) and a
//! period (bits 0-2) that says how many envelope steps pass between volume
//! changes. A period of 0 stops the envelope.
//!
//! "Zombie mode" is what happens when a game writes NRx2 while the channel is
//! playing: instead of being ignored, the write nudges or inverts the current
//! volume. Some games rely on this to change the volume without restarting the
//! sound.

use crate::apu::{Apu, Channel, MAX_VOLUME, NR22, NR42, Square};
use crate::model::Model;
use nubo_serializable_derive::Serializable;

/// NRx2 bits 0-2: the envelope steps between volume changes. 0 stops the
/// envelope.
pub(super) const fn envelope_period(nrx2: u8) -> u8 {
    nrx2 & 7
}

/// NRx2 bit 3: the envelope makes the channel louder rather than quieter.
pub(super) const fn envelope_increases(nrx2: u8) -> bool {
    nrx2 & 8 != 0
}

/// NRx2 bits 4-7: the volume a trigger starts the channel at.
pub(super) const fn initial_volume(nrx2: u8) -> u8 {
    nrx2 >> 4
}

/// Whether an NRx2 value has the channel's DAC (digital-to-analog converter)
/// on: a starting volume above 0 or a rising envelope, so bits 3-7 aren't all
/// 0. A channel with its DAC off can't play.
pub(super) const fn dac_enabled(nrx2: u8) -> bool {
    nrx2 & 0xF8 != 0
}

/// A channel's volume envelope (channels 1, 2 and 4): its volume and how the
/// volume changes over time.
///
/// Every eighth frame sequencer step counts `countdown` down. Once it has run
/// out, the next rising edge of the DIV-APU bit reloads it with the period and
/// schedules a tick, and the frame sequencer step after that runs the tick,
/// which moves the volume one step. The tick that takes the volume to its end
/// (0, or 15 when rising) locks the envelope there until the next trigger.
/// SameBoy keeps the last three fields in a `GB_envelope_clock_t`, where
/// `tick_pending` is `clock`.
#[derive(Debug, Default, Clone, Copy, Serializable)]
pub(super) struct Envelope {
    /// The channel's 4-bit volume.
    pub(super) volume: u8,
    /// Envelope steps left before the next tick is scheduled.
    pub(super) countdown: u8,
    /// A tick is scheduled for the next frame sequencer envelope step.
    tick_pending: bool,
    /// The envelope reached its end and stays there until the next trigger.
    locked: bool,
    /// The scheduled tick is the last one: the envelope locks when it is
    /// cleared.
    should_lock: bool,
}

impl Envelope {
    pub(super) fn tick_pending(self) -> bool {
        self.tick_pending
    }

    /// Whether the volume and the countdown are ones the envelope can have:
    /// a trigger sets the countdown to at most 7 plus a missed step.
    pub(super) fn is_valid(self) -> bool {
        self.volume <= MAX_VOLUME && self.countdown <= 8
    }

    /// A trigger unlocks the envelope and drops a scheduled tick, without the
    /// lock clearing it would otherwise bring (as SameBoy).
    pub(super) fn reset_on_trigger(&mut self) {
        self.locked = false;
        self.tick_pending = false;
    }

    /// Restarts the envelope at NRx2's starting volume and period, for a
    /// trigger. A trigger shortly before the envelope step that counts down
    /// doesn't let that step count (`step_missed`).
    pub(super) fn restart(&mut self, nrx2: u8, step_missed: bool) {
        self.volume = initial_volume(nrx2);
        self.countdown = envelope_period(nrx2) + u8::from(step_missed);
    }

    /// Counts down on a frame sequencer envelope step, unless a tick is
    /// scheduled. The countdown is 3 bits wide, so it wraps from 0 to 7.
    pub(super) fn count_down(&mut self) {
        if !self.tick_pending {
            self.countdown = self.countdown.wrapping_sub(1) & 7;
        }
    }

    /// Once the countdown has run out, reloads it with NRx2's period and
    /// schedules a tick, or clears the scheduled one when the period is 0.
    /// Runs on rising edges of the DIV-APU bit while the channel plays.
    pub(super) fn prepare_tick(&mut self, nrx2: u8) {
        if self.countdown != 0 {
            return;
        }
        self.countdown = envelope_period(nrx2);
        if self.countdown == 0 {
            self.clear_tick();
        } else if !self.tick_pending {
            let increasing = envelope_increases(nrx2);
            self.tick_pending = true;
            self.should_lock =
                (self.volume == 0xF && increasing) || (self.volume == 0x0 && !increasing);
        }
    }

    /// Clears a scheduled tick; if it was the last one, the envelope locks.
    fn clear_tick(&mut self) {
        if self.tick_pending {
            self.tick_pending = false;
            self.locked |= self.should_lock;
        }
    }

    /// Clears the scheduled tick as the frame sequencer runs it, and returns
    /// whether it moves the volume: not when the envelope is locked or NRx2's
    /// period is 0. `step_volume` then moves it.
    fn take_tick(&mut self, nrx2: u8) -> bool {
        self.clear_tick();
        !self.locked && envelope_period(nrx2) != 0
    }

    /// Moves the volume one step in the direction NRx2 selects. The volume is
    /// a 4-bit counter, so a zombie mode write that leaves it at the end of
    /// its range makes the step wrap around (15 up to 0, or 0 down to 15).
    fn step_volume(&mut self, nrx2: u8) {
        let stepped = if envelope_increases(nrx2) {
            self.volume.wrapping_add(1)
        } else {
            self.volume.wrapping_sub(1)
        };
        self.volume = stepped & 0x0F;
    }

    /// Applies an NRx2 write to a playing channel. On CGB-C and older the
    /// register briefly holds 0xFF during the write, so the glitch applies
    /// twice; pass `passes_through_ff` for those models.
    pub(super) fn zombie_mode_write(&mut self, passes_through_ff: bool, value: u8, old_value: u8) {
        if passes_through_ff {
            self.apply_zombie_mode_write(0xFF, old_value);
            self.apply_zombie_mode_write(value, 0xFF);
        } else {
            self.apply_zombie_mode_write(value, old_value);
        }
    }

    /// Applies the effect of changing NRx2 from `old_value` to `value` while
    /// the channel plays.
    fn apply_zombie_mode_write(&mut self, value: u8, old_value: u8) {
        if self.tick_pending {
            self.countdown = envelope_period(value);
        }
        let mut should_tick =
            envelope_period(value) != 0 && envelope_period(old_value) == 0 && !self.locked;
        let should_invert = envelope_increases(value) != envelope_increases(old_value);

        if value & 0xF == 8 && old_value & 0xF == 8 && !self.locked {
            should_tick = true;
        }

        if should_invert {
            if envelope_increases(value) {
                if envelope_period(old_value) == 0 && !self.locked {
                    self.volume ^= 0xF;
                } else {
                    self.volume = 0xE_u8.wrapping_sub(self.volume) & 0xF;
                }
                should_tick = false;
            } else {
                self.volume = 0x10_u8.wrapping_sub(self.volume) & 0xF;
            }
        }

        if should_tick {
            self.step_volume(value);
        } else if envelope_period(value) == 0 {
            self.clear_tick();
        }
    }
}

impl Apu {
    /// Runs a square channel's scheduled envelope tick.
    pub(super) fn tick_square_envelope(&mut self, square: Square) {
        let nrx2 = self.registers[square.nrx2()];
        if !self.state.square_mut(square).envelope.take_tick(nrx2) {
            return;
        }

        if self.cgb_double_speed {
            self.mask_pcm_for_square_envelope_tick(square);
        }

        self.state.square_mut(square).envelope.step_volume(nrx2);

        if self.state.is_active(square.channel()) {
            self.update_square_sample(square);
        }
    }

    /// Models the PCM12 read glitch caused by an envelope tick in double speed.
    fn mask_pcm_for_square_envelope_tick(&mut self, square: Square) {
        if square == Square::Channel1 {
            self.pcm_mask[0] &= self.state.square1.envelope.volume | 0xF1;
            return;
        }
        let volume = self.state.square2.envelope.volume;
        let mask =
            if self.model == Model::Cgb0 && volume == 1 && envelope_increases(self.registers[NR22])
            {
                0x1F
            } else {
                0x3F
            };
        self.pcm_mask[0] &= (volume << 4) | mask;
    }

    /// Runs the noise channel's scheduled envelope tick.
    pub(super) fn tick_noise_envelope(&mut self) {
        let nr42 = self.registers[NR42];
        if !self.state.noise.envelope.take_tick(nr42) {
            return;
        }

        if self.cgb_double_speed {
            self.pcm_mask[1] &= (self.state.noise.envelope.volume << 4) | 0x1F;
        }

        self.state.noise.envelope.step_volume(nr42);

        if self.state.is_active(Channel::Noise) {
            // The LFSR's lowest bit, as SameBoy, where an NR42 write uses
            // `current_lfsr_sample` instead (see `write_nr42`).
            let sample = if self.state.noise.lfsr & 1 != 0 {
                self.state.noise.envelope.volume
            } else {
                0
            };
            self.update_sample(Channel::Noise, sample);
        }
    }
}
