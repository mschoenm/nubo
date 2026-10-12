/*
    The square channels' timing and quirks are ported from SameBoy's apu.c
    (https://github.com/LIJI32/SameBoy, Expat License, see the notice in apu.rs).
*/

//! The two square wave channels (channels 1 and 2).
//!
//! Each channel plays a square wave with a duty cycle (the fraction of time the
//! wave is high: 12.5%, 25%, 50% or 75%) set by bits 6-7 of NRx1. The pitch
//! comes from an 11-bit period value split over NRx3 (low 8 bits) and NRx4 (top
//! 3 bits): the channel moves to the next of the 8 steps of its duty waveform
//! every `(2048 - period) * 2` ticks. Setting bit 7 of NRx4 triggers (restarts)
//! the channel. Channel 1 also has a frequency sweep, see `sweep`.

use crate::apu::envelope::dac_enabled;
use crate::apu::{Apu, MAX_PERIOD, Square, inverted_period, triggers};
use crate::model::Model;

/// The 8-step waveform of each duty cycle setting, one row per value of NRx1
/// bits 6-7.
const DUTY_WAVEFORMS: [u8; 32] = [
    0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 0,
];

/// The same waveforms with each one's 8 steps as the bits of a byte, step 0 in
/// bit 0.
const DUTY_PATTERNS: [u8; 4] = {
    let mut patterns = [0; 4];
    let mut index = 0;
    while index < DUTY_WAVEFORMS.len() {
        patterns[index / 8] |= DUTY_WAVEFORMS[index] << (index % 8);
        index += 1;
    }
    patterns
};

/// What `sample_countdown` reloads with, so a square channel steps every
/// `(2048 - period) * 2` ticks: it runs at 1 MHz, half the 2 MHz tick rate.
fn reload_countdown(period: u16) -> u16 {
    inverted_period(period) * 2 + 1
}

impl Apu {
    /// Updates a square channel's output from its duty step and volume.
    pub(super) fn update_square_sample(&mut self, square: Square) {
        let channel = self.state.square(square);
        if channel.sample_suppressed {
            return;
        }

        let volume = if channel.duty_output_high {
            channel.envelope.volume
        } else {
            0
        };
        self.update_sample(square.channel(), volume);
    }

    /// Latches the duty waveform's level at the channel's current position. The level only
    /// changes when the position moves, so a new duty cycle in NRx1 is heard from the next step
    /// on (Gambatte's sound ch1_duty0_to_duty3_pos3 tests).
    pub(super) fn latch_duty_output(&mut self, square: Square) {
        let duty = self.registers[square.nrx1()] >> 6;
        let channel = self.state.square_mut(square);
        channel.duty_output_high =
            DUTY_WAVEFORMS[(channel.current_sample_index + duty * 8) as usize] != 0;
    }

    /// The number of 2 MHz ticks up to and including the duty step that next
    /// changes a playing square channel's output, as long as no register
    /// write or frame sequencer step comes first. `None` when no step does.
    pub(super) fn ticks_until_square_output_change(&self, square: Square) -> Option<u32> {
        if !self.state.is_active(square.channel()) || !self.is_dac_enabled(square.channel()) {
            return None;
        }
        let channel = self.state.square(square);
        let duty = self.registers[square.nrx1()] >> 6;
        // Bit `n` is the waveform's level `n + 1` steps on, which plays as the
        // volume when set and as 0 when clear.
        let upcoming = DUTY_PATTERNS[usize::from(duty)]
            .rotate_right(u32::from(channel.current_sample_index) + 1);
        let output = self.state.samples[square.channel().index()];
        let changing_steps = if output == 0 {
            if channel.envelope.volume == 0 {
                0
            } else {
                upcoming
            }
        } else if output == channel.envelope.volume {
            !upcoming
        } else {
            0xFF
        };
        if changing_steps == 0 {
            return None;
        }
        let steps = changing_steps.trailing_zeros() + 1;
        let ticks_per_step = u32::from(reload_countdown(channel.period)) + 1;
        Some(u32::from(channel.sample_countdown) + 1 + (steps - 1) * ticks_per_step)
    }

    /// Advances a square channel by `ticks` 2 MHz ticks.
    // Always inlined, so `run`'s loop over the two squares unrolls with each
    // one's fields at fixed addresses; as a call it costs 0.15% more
    // instructions overall.
    #[inline(always)]
    pub(super) fn run_square(&mut self, square: Square, ticks: u16) {
        let mut ticks_left = ticks;
        {
            let channel = self.state.square_mut(square);
            if channel.delay > 0 {
                channel.delay = channel
                    .delay
                    .saturating_sub(u8::try_from(ticks_left).unwrap_or(u8::MAX));
            }
        }

        while ticks_left > self.state.square(square).sample_countdown {
            let channel = self.state.square_mut(square);
            ticks_left -= channel.sample_countdown + 1;
            channel.sample_countdown = reload_countdown(channel.period);
            channel.current_sample_index = (channel.current_sample_index + 1) & 7;
            channel.sample_suppressed = false;
            channel.did_tick = true;
            self.latch_duty_output(square);
            if ticks_left == 0 && self.state.samples[square.channel().index()] == 0 {
                self.pcm_mask[0] &= match square {
                    Square::Channel1 => 0xF0,
                    Square::Channel2 => 0x0F,
                };
            }
            self.update_square_sample(square);
        }

        let channel = self.state.square_mut(square);
        channel.just_reloaded = ticks_left == 0;
        if ticks_left > 0 {
            channel.sample_countdown -= ticks_left;
        }
    }

    /// NRx1 sets the duty cycle (bits 6-7) and the length (bits 0-5). Returns
    /// the value the register keeps: while the APU is off, which only DMG
    /// models let NRx1 be written in, it keeps the length but not the duty.
    pub(super) fn write_square_nrx1(&mut self, square: Square, value: u8) -> u8 {
        self.load_length(square.channel(), value);
        if self.state.powered_on {
            value
        } else {
            value & 0x3F
        }
    }

    /// NRx2 sets the volume envelope. Turning the DAC off (bits 3-7 all 0)
    /// stops the channel, and writes while it plays cause zombie mode.
    pub(super) fn write_square_nrx2(&mut self, square: Square, value: u8) {
        if !dac_enabled(value) {
            self.registers[square.nrx2()] = value;
            self.silence(square.channel());
        } else if self.state.is_active(square.channel()) {
            let old_value = self.registers[square.nrx2()];
            let passes_through_ff = self.model <= Model::CgbC;
            self.state.square_mut(square).envelope.zombie_mode_write(
                passes_through_ff,
                value,
                old_value,
            );
            self.update_square_sample(square);
        }
    }

    /// NRx3 holds the low 8 bits of the period.
    pub(super) fn write_square_nrx3(&mut self, square: Square, value: u8) {
        let channel = self.state.square_mut(square);
        channel.period = (channel.period & !0xFF) | u16::from(value);
        if channel.just_reloaded {
            channel.sample_countdown = reload_countdown(channel.period);
        }
    }

    /// NRx4 holds the top 3 bits of the period, the length enable bit (6) and
    /// the trigger bit (7).
    pub(super) fn write_square_nrx4(&mut self, square: Square, value: u8) {
        let nrx2 = self.registers[square.nrx2()];
        let was_active = self.state.is_active(square.channel());
        let late_revision = self.has_late_revision_square_timing();

        // SameBoy: "When the sample length changes right before being updated
        // from ≥$700 to <$700, the countdown should change to the old length,
        // but the current sample should not change. Because our write timing
        // isn't accurate to the T-cycle, we hack around it by stepping the
        // sample index backwards." So this models write timing, not a quirk of
        // the hardware.
        if !triggers(value)
            && was_active
            && self.registers[square.nrx4()] & 7 == 7
            && value & 7 != 7
        {
            let channel = self.state.square_mut(square);
            if (late_revision || channel.sample_countdown & 1 != 0)
                && channel.did_tick
                && channel.sample_countdown >> 1 == inverted_period(channel.period)
            {
                channel.current_sample_index = channel.current_sample_index.wrapping_sub(1) & 7;
                channel.sample_suppressed = false;
                self.latch_duty_output(square);
            }
        }

        let channel = self.state.square_mut(square);
        let old_period = channel.period;
        channel.period = (channel.period & 0xFF) | (u16::from(value & 7) << 8);
        if channel.just_reloaded {
            channel.sample_countdown = reload_countdown(channel.period);
        }

        if triggers(value) {
            self.trigger_square(square, value, nrx2, was_active, old_period);
        }

        self.write_nrx4_length(square.channel(), value);
    }

    /// CGB-D and CGB-E changed some square channel timing.
    fn has_late_revision_square_timing(&self) -> bool {
        matches!(self.model, Model::CgbD | Model::CgbE)
    }

    /// Restarts a square channel: reloads its timer, volume and envelope, and
    /// turns it on when its DAC is on.
    fn trigger_square(
        &mut self,
        square: Square,
        value: u8,
        nrx2: u8,
        was_active: bool,
        old_period: u16,
    ) {
        let late_revision = self.has_late_revision_square_timing();
        let tick_parity = self.state.tick_parity;

        let step_missed = self.envelope_step_missed(nrx2);
        let channel = self.state.square_mut(square);
        channel.envelope.reset_on_trigger();
        channel.did_tick = false;
        let mut force_unsuppressed = false;
        let mut stepped = false;

        // The `/ 2`s below divide a value that can be negative, rounding
        // towards 0 like SameBoy's C; `>> 1` would round -1 down to -1 and set
        // bit 0x400.
        if was_active {
            let mut extra_delay = 0;
            if late_revision {
                if !channel.just_reloaded
                    && value & 4 == 0
                    && ((i32::from(channel.sample_countdown) - 1 - i32::from(channel.delay)) / 2)
                        & 0x400
                        == 0
                {
                    channel.current_sample_index = (channel.current_sample_index + 1) & 7;
                    channel.sample_suppressed = false;
                    stepped = true;
                } else if channel.period == MAX_PERIOD
                    && old_period != MAX_PERIOD
                    && channel.sample_suppressed
                {
                    extra_delay += 2;
                }
            }
            channel.delay = 4 - tick_parity + extra_delay;
            channel.sample_countdown =
                inverted_period(channel.period) * 2 + u16::from(channel.delay);
        } else {
            if late_revision
                && value & 4 == 0
                && ((i32::from(channel.sample_countdown) - i32::from(channel.delay)) / 2) & 0x400
                    == 0
            {
                channel.current_sample_index = (channel.current_sample_index + 1) & 7;
                force_unsuppressed = true;
                stepped = true;
            }
            // In ticks, the first step comes as late in double speed as at normal speed, on
            // every model (Gambatte's sound ch1_duty0_pos6_to_pos7_timing_ds tests).
            channel.delay = 6 - tick_parity;
            channel.sample_countdown =
                inverted_period(channel.period) * 2 + u16::from(channel.delay);
        }

        channel.envelope.restart(nrx2, step_missed);
        if stepped {
            self.latch_duty_output(square);
        }
        if self.state.is_active(square.channel()) {
            self.update_square_sample(square);
        }

        if dac_enabled(nrx2) && !self.state.is_active(square.channel()) {
            self.state.set_active(square.channel(), true);
            self.update_sample(square.channel(), 0);
            self.state.square_mut(square).sample_suppressed = !force_unsuppressed;
        }

        self.restart_length_if_run_out(square.channel());

        if square == Square::Channel1 {
            self.trigger_sweep(was_active);
        }
    }
}
