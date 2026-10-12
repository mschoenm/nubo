/*
    The wave channel's timing and wave RAM quirks are ported from SameBoy's apu.c
    (https://github.com/LIJI32/SameBoy, Expat License, see the notice in apu.rs).
*/

//! The wave channel (channel 3).
//!
//! The channel plays 32 4-bit samples from wave RAM (0xFF30-0xFF3F, two samples
//! per byte, high nibble first), moving to the next sample every
//! `2048 - period` ticks. NR30 turns its DAC on or off, NR32 sets the volume as
//! a right shift of the samples, and NR33/NR34 hold the period. While the
//! channel plays, CPU accesses to wave RAM go to the byte the channel is
//! reading; on DMG they only work at the moment the channel reads it.

use crate::apu::{Apu, Channel, NR30, WAVE_RAM_START, inverted_period, triggers};
use crate::model::Model;

/// The right shift applied to samples for each NR32 output level: mute, 100%,
/// 50% and 25%.
const VOLUME_SHIFTS: [u8; 4] = [4, 0, 1, 2];
/// Extra ticks before the first sample after a trigger.
const TRIGGER_DELAY: u16 = 3;
/// The `sample_countdown` at which an AGB rereads the first wave RAM byte when
/// the length counter expires.
const AGB_FIRST_BYTE_RELATCH_COUNTDOWN: u16 = 9;
/// A 128-bit number with all 32 of its 4-bit nibbles set to 1, which turns a
/// nibble into 32 copies of it when multiplied by it.
const EVERY_NIBBLE: u128 = u128::MAX / 0xF;

impl Apu {
    /// Reads a byte of wave RAM; `offset` wraps around at 16 bytes.
    fn wave_ram(&self, offset: usize) -> u8 {
        self.registers[WAVE_RAM_START + (offset & 0xF)]
    }

    /// Updates the channel's output from the current sample and volume shift.
    pub(super) fn update_wave_sample(&mut self) {
        let wave = &self.state.wave;
        let nibble = if wave.current_sample_index & 1 != 0 {
            wave.current_sample_byte & 0xF
        } else {
            wave.current_sample_byte >> 4
        };
        let shift = wave.volume_shift;
        self.update_sample(Channel::Wave, nibble >> shift);
    }

    /// The number of 2 MHz ticks up to and including the sample step that
    /// next changes the playing wave channel's output, as long as no register
    /// write or frame sequencer step comes first. `None` when no step does.
    pub(super) fn ticks_until_wave_output_change(&self) -> Option<u32> {
        if !self.state.is_active(Channel::Wave) || !self.is_dac_enabled(Channel::Wave) {
            return None;
        }
        let wave = &self.state.wave;
        let shift = u32::from(wave.volume_shift);
        // The sample a nibble has to hold to play as the current output: one
        // whose bits from `shift` up match it.
        let sample = u128::from(self.state.samples[Channel::Wave.index()]) << shift;
        let steps = if sample > 0xF {
            1
        } else {
            // Wave RAM with its first sample in the top 4 bits, turned so the
            // next sample comes first.
            let wave_ram: [u8; 16] = self.registers[WAVE_RAM_START..WAVE_RAM_START + 16]
                .try_into()
                .expect("wave RAM is 16 bytes");
            let upcoming = u128::from_be_bytes(wave_ram)
                .rotate_left(4 * (u32::from(wave.current_sample_index) + 1));
            let heard_bits = EVERY_NIBBLE * ((0xF << shift) & 0xF);
            let changes = (upcoming ^ (EVERY_NIBBLE * sample)) & heard_bits;
            if changes == 0 {
                return None;
            }
            changes.leading_zeros() / 4 + 1
        };
        let ticks_per_step = u32::from(inverted_period(wave.period)) + 1;
        Some(u32::from(wave.sample_countdown) + 1 + (steps - 1) * ticks_per_step)
    }

    /// Advances the wave channel by `ticks` 2 MHz ticks. Except on AGB, the
    /// timer of a stopped channel keeps running while its DAC is on and keeps
    /// latching the wave RAM byte the CPU's bus address points at.
    pub(super) fn run_wave(&mut self, ticks: u16) {
        self.state.wave.wave_form_just_read = false;
        let mut ticks_left = ticks;

        if self.state.is_active(Channel::Wave) {
            while ticks_left > self.state.wave.sample_countdown {
                let wave = &mut self.state.wave;
                ticks_left -= wave.sample_countdown + 1;
                wave.sample_countdown = inverted_period(wave.period);
                wave.current_sample_index = (wave.current_sample_index + 1) & 0x1F;
                let index = wave.current_sample_index as usize >> 1;
                self.state.wave.current_sample_byte = self.wave_ram(index);
                self.update_wave_sample();
                self.state.wave.wave_form_just_read = true;
            }
            if ticks_left > 0 {
                self.state.wave.sample_countdown -= ticks_left;
                self.state.wave.wave_form_just_read = false;
            }
        } else if self.stopped_wave_timer_runs() {
            while ticks_left > self.state.wave.sample_countdown {
                let wave = &mut self.state.wave;
                ticks_left -= wave.sample_countdown + 1;
                wave.sample_countdown = inverted_period(wave.period);
                if ticks_left > 0 {
                    self.state.wave.current_sample_byte = self.wave_ram(self.bus_address as usize);
                } else {
                    self.state.wave.bugged_read_countdown = 1;
                }
            }
            if ticks_left > 0 {
                self.state.wave.sample_countdown -= ticks_left;
            }
            if self.state.wave.sample_countdown == 0 {
                self.state.wave.bugged_read_countdown = 2;
            }
        }
    }

    /// Whether the timer of the stopped wave channel runs: except on AGB, it
    /// keeps running after the channel stops (by its length counter), while
    /// its DAC stays on, and latches whatever wave RAM byte the CPU's bus
    /// address points at.
    pub(super) fn stopped_wave_timer_runs(&self) -> bool {
        !self.state.is_active(Channel::Wave)
            && self.state.wave.dac_enabled
            && self.state.wave.pulsed
            && !self.is_agb()
    }

    /// NR30 bit 7 turns the DAC on or off. Turning it off stops the channel.
    pub(super) fn write_nr30(&mut self, value: u8) {
        self.state.wave.dac_enabled = value & 0x80 != 0;
        if self.state.wave.dac_enabled {
            return;
        }

        self.state.wave.pulsed = false;
        if self.state.is_active(Channel::Wave) {
            // Turning the DAC off just as the timer reads wave RAM latches the
            // byte at the CPU's address: SameBoy uses the CPU's PC (`gb->pc &
            // 0xF`), Nubo the address on the bus, as for the stopped timer's
            // reads. During this write that is 0xFF1A, so both branches latch
            // byte 0xA, which `wave_ram(NR30)` reads too: NR30's register
            // offset (0x0A) has the low nibble of 0xFF1A. No test ROM tells
            // the PC and the bus apart here.
            if self.state.wave.sample_countdown == 0 && !self.is_agb() {
                self.state.wave.current_sample_byte = self.wave_ram(self.bus_address as usize);
            } else if self.state.wave.wave_form_just_read && self.model <= Model::CgbC {
                self.state.wave.current_sample_byte = self.wave_ram(NR30);
            }
        }
        self.silence(Channel::Wave);
    }

    /// On AGB, a length counter expiring at certain moments latches a new wave
    /// RAM byte.
    pub(super) fn latch_agb_wave_sample_on_length_expiry(&mut self) {
        let wave = &self.state.wave;
        if wave.sample_countdown == 0 {
            let offset = ((wave.current_sample_index as usize + 1) & 0xF) >> 1;
            self.state.wave.current_sample_byte = self.wave_ram(offset);
        } else if wave.sample_countdown == AGB_FIRST_BYTE_RELATCH_COUNTDOWN {
            self.state.wave.current_sample_byte = self.wave_ram(0);
        }
    }

    /// NR32 sets the output level.
    pub(super) fn write_nr32(&mut self, value: u8) {
        self.state.wave.volume_shift = VOLUME_SHIFTS[((value >> 5) & 3) as usize];
        if self.state.is_active(Channel::Wave) {
            self.update_wave_sample();
        }
    }

    /// NR33 holds the low 8 bits of the period.
    pub(super) fn write_nr33(&mut self, value: u8) {
        let wave = &mut self.state.wave;
        wave.period = (wave.period & !0xFF) | u16::from(value);
        if wave.bugged_read_countdown == 1 {
            wave.sample_countdown = inverted_period(wave.period);
        }
    }

    /// NR34 holds the top 3 bits of the period, the length enable bit (6) and
    /// the trigger bit (7).
    pub(super) fn write_nr34(&mut self, value: u8) {
        let wave = &mut self.state.wave;
        wave.period = (wave.period & 0xFF) | (u16::from(value & 7) << 8);

        if triggers(value) {
            self.trigger_wave();
        }

        self.write_nrx4_length(Channel::Wave, value);
    }

    /// Restarts the wave channel from the first sample. On DMG, triggering it
    /// at the moment it reads wave RAM corrupts the first bytes of wave RAM.
    fn trigger_wave(&mut self) {
        self.state.wave.pulsed = true;

        if self.is_dmg()
            && self.state.is_active(Channel::Wave)
            && self.state.wave.sample_countdown == 0
        {
            let offset = (((self.state.wave.current_sample_index + 1) >> 1) & 0xF) as usize;
            if offset < 4 && self.model != Model::Mgb {
                self.registers[WAVE_RAM_START] = self.wave_ram(offset);
            } else {
                let block_start = WAVE_RAM_START + (offset & !3);
                self.registers
                    .copy_within(block_start..block_start + 4, WAVE_RAM_START);
            }
        }

        self.state.wave.current_sample_index = 0;
        if self.state.is_active(Channel::Wave) && self.state.wave.sample_countdown == 0 {
            self.state.wave.current_sample_byte = self.wave_ram(0);
        }
        if self.state.wave.dac_enabled {
            self.state.set_active(Channel::Wave, true);
            let sample = (self.state.wave.current_sample_byte >> 4) >> self.state.wave.volume_shift;
            self.update_sample(Channel::Wave, sample);
        }

        let wave = &mut self.state.wave;
        wave.sample_countdown = inverted_period(wave.period) + TRIGGER_DELAY;
        self.restart_length_if_run_out(Channel::Wave);
    }
}
