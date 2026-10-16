/*
    The noise channel's timing and the NR43 write glitch tables are ported from SameBoy's apu.c
    (https://github.com/LIJI32/SameBoy, Expat License, see the notice in apu.rs).
*/

//! The noise channel (channel 4).
//!
//! A 14-bit counter, clocked at a rate set by NR43's divider (bits 0-2), steps
//! a linear-feedback shift register (LFSR) whenever the counter bit chosen by
//! NR43's upper nibble goes from 0 to 1. The LFSR's lowest bit is the output,
//! which sounds like noise. In narrow mode (NR43 bit 3) the LFSR repeats much
//! sooner and sounds more tonal. Writing NR43 while the counter runs can step
//! or corrupt the LFSR in ways that differ per hardware revision.

use crate::apu::envelope::dac_enabled;
use crate::apu::{Apu, Channel, NR42, NR43, NoiseCounting, triggers};
use crate::model::Model;

/// Extra ticks for the counter countdown when NR43 is written right after a
/// reload, indexed by the tick alignment: on CGB-D and newer (late) and on
/// CGB-C and older (early).
const LATE_NR43_RELOAD_OFFSETS: [u8; 4] = [2, 1, 0, 3];
const EARLY_NR43_RELOAD_OFFSETS: [u8; 4] = [2, 1, 4, 3];
/// How long a DMG delays a noise trigger written out of phase with the counter.
const DMG_DELAYED_START_TICKS: u8 = 6;
/// The noise counter is 14 bits wide, so NR43 shifts from this one up select a
/// bit it never sets in counting (only an NR42 write can carry it into bit 14,
/// see `write_nr42`, and the next count clears it again).
const NOISE_COUNTER_BITS: u32 = 14;
/// The LFSR shifts right on every step, and the bit each step shifts in only
/// reaches bit 0 on the 15th step after it, or the 7th in narrow mode, where it
/// is shifted in at bit 6 too. Up to this many steps, the LFSR's bits all come
/// from the LFSR as it is now.
const WIDE_LOOKAHEAD_STEPS: u32 = 14;
const NARROW_LOOKAHEAD_STEPS: u32 = 6;

/// The LFSR glitch a CGB-D shows when an NR43 write keeps the selected counter
/// bit at 1 but briefly selects a 0 bit on the way, indexed by the old and new
/// NR43 bits 4-6. This table is for writes where NR43's old bit 7 is clear, the
/// next one for writes where it is set.
///
/// Each entry packs two answers (see `nr43_cgb_d_rising_glitch`):
///
/// - Bits 0-3: the kind of glitch for such a write. 0 is none, 5 sets the
///   LFSR's top bits, and the other kinds step the LFSR with a corruption of
///   their own.
/// - Bits 4 and 5 (0x10 and 0x20): whether a CGB-D also glitches (as kind 5)
///   when the selected bit is 1 before, during and after a write that changes
///   NR43 bits 4-6 (`force_glitch`): bit 5 for writes that keep NR43 bit 7,
///   bit 4 for writes that clear it. Writes that set it glitch when the kind
///   is 1 or 4.
const CGB_D_LOW_TO_HIGH_GLITCHES: [u8; 64] = [
    0x00, 0x01, 0x01, 0x21, 0x02, 0x21, 0x00, 0x00, 0x03, 0x00, 0x21, 0x01, 0x04, 0x04, 0x00, 0x00,
    0x05, 0x01, 0x00, 0x01, 0x04, 0x21, 0x00, 0x00, 0x03, 0x05, 0x05, 0x00, 0x01, 0x01, 0x00, 0x00,
    0x05, 0x01, 0x01, 0x21, 0x00, 0x01, 0x00, 0x00, 0x05, 0x05, 0x21, 0x01, 0x05, 0x00, 0x00, 0x00,
    0x05, 0x01, 0x05, 0x01, 0x05, 0x01, 0x00, 0x00, 0x03, 0x05, 0x05, 0x05, 0x05, 0x05, 0x00, 0x00,
];

const CGB_D_HIGH_TO_LOW_GLITCHES: [u8; 64] = [
    0x00, 0x27, 0x26, 0x37, 0x21, 0x38, 0x01, 0x01, 0x01, 0x00, 0x38, 0x21, 0x21, 0x21, 0x01, 0x01,
    0x01, 0x27, 0x00, 0x28, 0x21, 0x38, 0x01, 0x01, 0x01, 0x02, 0x01, 0x00, 0x31, 0x21, 0x01, 0x01,
    0x06, 0x28, 0x28, 0x38, 0x00, 0x27, 0x01, 0x01, 0x01, 0x03, 0x38, 0x21, 0x01, 0x00, 0x01, 0x01,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// The LFSR glitch CGB-E and newer show when an NR43 write keeps the selected
/// counter bit at 0 but briefly selects a 1 bit on the way. Indexed in octal by
/// the old NR43 bits 4-6, then the new ones.
const CGB_CATEGORY_2_GLITCHES: [u8; 64] = {
    let mut map = [0u8; 64];
    map[0o02] = 4;
    map[0o03] = 2;
    map[0o04] = 2;
    map[0o05] = 2;
    map[0o12] = 2;
    map[0o13] = 4;
    map[0o14] = 2;
    map[0o15] = 2;
    map[0o20] = 1;
    map[0o21] = 2;
    map[0o23] = 1;
    map[0o24] = 5;
    map[0o25] = 3;
    map[0o34] = 2;
    map[0o35] = 2;
    map[0o41] = 2;
    map[0o42] = 2;
    map[0o43] = 2;
    map[0o50] = 6;
    map[0o52] = 2;
    map[0o53] = 2;
    map
};

/// NR43 bits 4-7: the noise counter bit whose rising edge steps the LFSR.
const fn counter_shift(nr43: u8) -> u8 {
    nr43 >> 4
}

/// The LFSR after one step: shifted right once, with the inverted XOR of its
/// two lowest bits fed back into the bits of `high_bit_mask`.
fn next_lfsr(lfsr: u16, high_bit_mask: u16) -> u16 {
    let shifted = lfsr >> 1;
    if (lfsr ^ shifted ^ 1) & 1 != 0 {
        shifted | high_bit_mask
    } else {
        shifted & !high_bit_mask
    }
}

/// The LFSR after `steps` steps, worked out a few steps at a time.
fn lfsr_after_steps(mut lfsr: u16, narrow: bool, mut steps: u32) -> u16 {
    let lookahead = if narrow {
        NARROW_LOOKAHEAD_STEPS
    } else {
        WIDE_LOOKAHEAD_STEPS
    };
    while steps > 0 {
        let chunk = steps.min(lookahead);
        lfsr = lfsr_after_lookahead_steps(lfsr, narrow, chunk);
        steps -= chunk;
    }
    lfsr
}

/// The LFSR after `steps` steps, at most 14 or (in narrow mode) 6. Within
/// that many steps, every bit a step shifts in comes from two bits of the LFSR
/// as it is now, so they can all be worked out at once.
fn lfsr_after_lookahead_steps(lfsr: u16, narrow: bool, steps: u32) -> u16 {
    // A 16th bit never survives the first step.
    let lfsr = u32::from(lfsr) & 0x7FFF;
    // Bit `n` is the bit that step `n + 1` shifts in.
    let shifted_in = !(lfsr ^ (lfsr >> 1)) & ((1 << steps) - 1);
    let wide = (lfsr >> steps) | (shifted_in << (15 - steps));
    let stepped = if narrow {
        (wide & !0x7F) | ((lfsr & 0x7F) >> steps) | (shifted_in << (7 - steps))
    } else {
        wide
    };
    u16::try_from(stepped).expect("the LFSR is 15 bits wide")
}

/// How many of the next `increments` increments of the noise counter, which
/// now holds `counter`, make its bit `bit` go from 0 to 1 (each one steps the
/// LFSR), and whether the last increment is one of them. `None` when none is.
fn rising_edges(counter: u32, bit: u32, increments: u32) -> Option<(u32, bool)> {
    if bit >= NOISE_COUNTER_BITS {
        return None;
    }
    let first = increments_until_rising_edge(counter, bit);
    if first > increments {
        return None;
    }
    let after_first = increments - first;
    let period = 2 << bit;
    Some((after_first / period + 1, after_first.is_multiple_of(period)))
}

/// The number of increments of the noise counter, which now holds `counter`,
/// up to and including the next one that makes its bit `bit` (below 14) go
/// from 0 to 1. That happens when the bits up to it go from `bit_value - 1` to
/// `bit_value`, once every `2 * bit_value` increments.
fn increments_until_rising_edge(counter: u32, bit: u32) -> u32 {
    let bit_value = 1 << bit;
    let period = bit_value * 2;
    (bit_value + period - counter % period - 1) % period + 1
}

/// The 2 MHz ticks between counter increments for NR43's divider code (bits
/// 0-2): 4 per step of the code, with a code of 0 counting as half a step.
fn ticks_per_counter_increment(nr43: u8) -> u8 {
    match (nr43 & 0x07) << 2 {
        0 => 2,
        ticks => ticks,
    }
}

impl Apu {
    /// The LFSR bits that receive the feedback bit: bit 14, and bit 6 too in
    /// narrow mode.
    fn lfsr_high_bit_mask(&self) -> u16 {
        if self.state.noise.narrow {
            0x4040
        } else {
            0x4000
        }
    }

    /// Updates the channel's output from the LFSR's lowest bit.
    fn update_noise_sample(&mut self) {
        let noise = &mut self.state.noise;
        noise.current_lfsr_sample = noise.lfsr & 1 != 0;
        if self.state.is_active(Channel::Noise) {
            let sample = if self.state.noise.current_lfsr_sample {
                self.state.noise.envelope.volume
            } else {
                0
            };
            self.update_sample(Channel::Noise, sample);
        }
    }

    /// Shifts the LFSR right once, feeding back the inverted XOR of its two
    /// lowest bits.
    pub(super) fn step_lfsr(&mut self) {
        let high_bit_mask = self.lfsr_high_bit_mask();
        let noise = &mut self.state.noise;
        self.state.lfsr_bit_7_before_step = noise.lfsr & 0x80 != 0;
        noise.lfsr = next_lfsr(noise.lfsr, high_bit_mask);
        self.update_noise_sample();
        self.state.lfsr_stepped_in_narrow = self.state.noise.narrow;
    }

    /// The number of 2 MHz ticks up to and including the LFSR step that next
    /// changes the playing noise channel's output, or an earlier LFSR step,
    /// as long as no register write or frame sequencer step comes first.
    /// `None` when no LFSR step can change it.
    pub(super) fn ticks_until_noise_output_change(&self) -> Option<u32> {
        if self.state.noise_counting == NoiseCounting::Stopped
            || !self.state.is_active(Channel::Noise)
            || !self.is_dac_enabled(Channel::Noise)
        {
            return None;
        }
        let nr43 = self.registers[NR43];
        let shift = u32::from(counter_shift(nr43));
        if shift >= NOISE_COUNTER_BITS {
            return None;
        }

        // The LFSR steps when the counter bit NR43 selects goes from 0 to 1.
        let increments_until_step =
            increments_until_rising_edge(u32::from(self.state.noise.counter), shift);
        let noise = &self.state.noise;
        let output = self.state.samples[Channel::Noise.index()];
        if noise.envelope.volume == 0 && output == 0 {
            return None;
        }
        let lookahead = if noise.narrow {
            NARROW_LOOKAHEAD_STEPS
        } else {
            WIDE_LOOKAHEAD_STEPS
        };
        // Bit `n` is the LFSR's lowest bit after step `n + 1`, and the output
        // is the volume while that bit is set.
        let upcoming_bits = u32::from(noise.lfsr >> 1) & ((1 << lookahead) - 1);
        let changing_steps = if output == 0 {
            upcoming_bits
        } else if output == noise.envelope.volume {
            !upcoming_bits
        } else {
            1
        };
        let lfsr_steps = (changing_steps.trailing_zeros() + 1).min(lookahead);
        let increments = increments_until_step + (lfsr_steps - 1) * (2 << shift);

        let ticks_per_increment = u32::from(ticks_per_counter_increment(nr43));
        let ticks_until_increment = match noise.counter_countdown {
            0 => ticks_per_increment,
            countdown => u32::from(countdown),
        };
        Some(ticks_until_increment + (increments - 1) * ticks_per_increment)
    }

    /// Advances the noise counter by `ticks` 2 MHz ticks. Rather than going
    /// through the counter's increments one by one, this works out how many
    /// fall within the ticks and how many of them step the LFSR.
    pub(super) fn run_noise(&mut self, ticks: u16) {
        if self.state.noise_counting == NoiseCounting::Stopped {
            return;
        }

        let nr43 = self.registers[NR43];
        let ticks_per_increment = ticks_per_counter_increment(nr43);
        let noise = &mut self.state.noise;
        if noise.counter_countdown == 0 {
            noise.counter_countdown = ticks_per_increment;
        }
        let first_increment = u16::from(noise.counter_countdown);
        if ticks < first_increment {
            noise.counter_countdown -= u8::try_from(ticks).expect("less than the countdown");
            // `run` never runs 0 ticks, so the countdown wasn't reloaded.
            noise.countdown_reloaded = false;
            return;
        }

        let after_first_increment = ticks - first_increment;
        let increments = u32::from(after_first_increment / u16::from(ticks_per_increment)) + 1;
        let ticks_left = after_first_increment % u16::from(ticks_per_increment);
        let old_counter = u32::from(noise.counter);
        noise.counter = u16::try_from((old_counter + increments) & 0x3FFF)
            .expect("the counter is 14 bits wide");
        noise.did_step_counter = true;
        noise.counter_countdown = ticks_per_increment
            - u8::try_from(ticks_left).expect("less than the ticks per increment");
        noise.countdown_reloaded = ticks_left == 0;

        if !self.state.is_active(Channel::Noise) {
            return;
        }
        let Some((lfsr_steps, steps_on_last_increment)) =
            rising_edges(old_counter, u32::from(counter_shift(nr43)), increments)
        else {
            return;
        };
        // Only the last two steps can leave anything behind but the LFSR itself.
        if lfsr_steps > 2 {
            let noise = &mut self.state.noise;
            noise.lfsr = lfsr_after_steps(noise.lfsr, noise.narrow, lfsr_steps - 2);
        }
        if lfsr_steps > 1 {
            self.step_lfsr();
        }
        if steps_on_last_increment
            && ticks_left == 0
            && self.state.samples[Channel::Noise.index()] == 0
            && !self.cgb_double_speed
        {
            self.pcm_mask[1] &= 0x0F;
        }
        self.step_lfsr();
    }

    /// Sets up the counter and the LFSR for a trigger. The exact countdown
    /// depends on the tick alignment, the divider, the hardware revision and
    /// whether the channel was already running.
    fn prepare_noise_start(&mut self) {
        let nr42 = self.registers[NR42];
        let nr43 = self.registers[NR43];
        let active = self.state.is_active(Channel::Noise);
        let early_revision = self.model <= Model::CgbC;
        let early_revision_double_speed = early_revision && self.cgb_double_speed;

        let was_background_counting = self.state.noise_counting != NoiseCounting::Stopped;
        let runs_with_dac = dac_enabled(nr42);
        self.state.noise_counting = if runs_with_dac {
            NoiseCounting::Running
        } else {
            NoiseCounting::Background
        };
        let was_started_with_dac_disabled = self.state.noise_started_with_dac_disabled;
        self.state.noise_started_with_dac_disabled = !runs_with_dac;
        // NR43's divider code (bits 0-2), which one quirk below turns into 8.
        let mut divider_code = nr43 & 0x07;
        let mut instant_step = false;
        let mut div_1_glitch = false;

        let noise = &mut self.state.noise;
        if divider_code > 1
            && (noise.counter_countdown == 1
                || (noise.counter_countdown == 2 && active && early_revision_double_speed))
        {
            noise.counter = (noise.counter + 1) & 0x3FFF;
        } else if noise.counter_countdown == 2 && noise.alignment & 3 == 0 && active {
            if divider_code == 0 {
                divider_code = 8;
            } else if divider_code == 1 {
                if !noise.did_step_counter {
                    div_1_glitch = true;
                }
                let mask = 1u32 << counter_shift(nr43);
                let old_bit = u32::from(noise.counter) & mask != 0;
                noise.counter = (noise.counter + 1) & 0x3FFF;
                let new_bit = u32::from(noise.counter) & mask != 0;
                instant_step = new_bit && !old_bit;
            }
        }

        let mut countdown: u8 = if divider_code == 0 {
            6
        } else {
            divider_code * 4 + 6
        };
        if noise.alignment & 1 != 0 {
            if divider_code == 0 {
                if early_revision || !was_background_counting {
                    countdown = countdown.wrapping_add(1);
                } else {
                    countdown = countdown.wrapping_sub(1);
                }
            } else if noise.alignment & 2 != 0 {
                if divider_code == 1 && !active {
                    countdown = countdown.wrapping_add(1);
                } else {
                    countdown = countdown.wrapping_sub(3);
                }
            } else {
                countdown = countdown.wrapping_sub(1);
                if divider_code == 1 && active {
                    countdown = countdown.wrapping_sub(4);
                }
            }
        } else if divider_code != 0 {
            if noise.alignment & 2 != 0 {
                if early_revision_double_speed && divider_code == 1 {
                    countdown = countdown.wrapping_add(2);
                } else {
                    countdown = countdown.wrapping_sub(2);
                }
            } else if divider_code > 1 {
                if !early_revision_double_speed {
                    countdown = countdown.wrapping_sub(4);
                }
            } else if active && nr43 & 0xF0 == 0 {
                countdown = countdown.wrapping_sub(4);
            }
        } else if early_revision_double_speed {
            countdown = countdown.wrapping_add(2);
        }

        if divider_code > 1 {
            if !runs_with_dac && noise.alignment & 3 == 0 {
                countdown = countdown.wrapping_add(4);
            }
        } else if was_background_counting && !active && noise.alignment & 3 == 0 {
            if divider_code == 0 {
                if was_started_with_dac_disabled {
                    countdown = countdown.wrapping_add(28);
                }
            } else {
                countdown = countdown.wrapping_sub(4);
            }
        }

        if divider_code == 0 && early_revision_double_speed && was_background_counting && !active {
            countdown = countdown.wrapping_sub(1);
        }
        if div_1_glitch {
            countdown = countdown.wrapping_sub(4);
        }
        noise.counter_countdown = countdown;

        noise.lfsr = if divider_code == 0 && active && noise.alignment & 3 == 3 {
            0x0055
        } else {
            0
        };
        if instant_step {
            self.step_lfsr();
        }
    }

    /// Stores a new NR43 value and applies the LFSR glitches caused by the
    /// selected counter bit changing.
    fn apply_nr43_write(&mut self, new: u8) {
        let old_narrow = self.state.noise.narrow;
        self.state.noise.narrow = new & 8 != 0;
        let old = self.registers[NR43];
        self.registers[NR43] = new;

        if old & 0xF0 == new & 0xF0 {
            return;
        }

        let early_revision = self.model <= Model::CgbC;
        let mut effective_counter = u32::from(self.state.noise.counter);
        if early_revision && self.state.noise.countdown_reloaded {
            effective_counter |= effective_counter.wrapping_sub(1) & 0x3FFF;
        }
        let counter_bit = |value: u8| (effective_counter >> counter_shift(value)) & 1 != 0;
        let old_bit = counter_bit(old);
        let mut glitch_bit = counter_bit((old & 0x7F) | (new & 0x80));
        let new_bit = counter_bit(new);
        let mut force_glitch = false;

        if self.model == Model::CgbD && new_bit && glitch_bit && old_bit && (old ^ new) & 0x70 != 0
        {
            force_glitch = true;
        }

        if self.is_agb() {
            let (first, second) = if new >= 0x80 && old >= 0x80 {
                ((old & 0xCF) | (new & 0x30), (old & 0x8F) | (new & 0x70))
            } else {
                ((old & 0xDF) | (new & 0x20), (old & 0xCF) | (new & 0x30))
            };
            glitch_bit = counter_bit(first);
            if glitch_bit != counter_bit(second) {
                if new_bit == old_bit {
                    glitch_bit = !new_bit;
                } else if !glitch_bit && old_bit {
                    force_glitch = true;
                }
            }
        }

        if (old_bit == new_bit && new_bit != glitch_bit) || force_glitch {
            if new_bit {
                if self.model >= Model::CgbE {
                    self.nr43_rising_glitch(old, new, old_narrow);
                } else if self.model == Model::CgbD {
                    self.nr43_cgb_d_rising_glitch(old, new, force_glitch);
                }
            } else if self.model >= Model::CgbE {
                self.nr43_falling_glitch(old, new);
            } else {
                self.step_lfsr();
            }
        } else if !old_bit && new_bit {
            if early_revision {
                let previous_narrow = self.state.noise.narrow;
                self.state.noise.narrow = true;
                self.step_lfsr();
                self.state.noise.narrow = previous_narrow;
                if new & 0xF0 <= 0x20 && glitch_bit && effective_counter & 8 == 0 {
                    self.step_lfsr();
                    let mask = self.lfsr_high_bit_mask();
                    let lower_mask = if self.state.noise.narrow {
                        0x2020
                    } else {
                        0x2000
                    };
                    let noise = &mut self.state.noise;
                    noise.lfsr &= !mask;
                    noise.lfsr |= (noise.lfsr & lower_mask) << 1;
                }
            } else {
                self.step_lfsr();
            }
        } else if early_revision
            && new & 0xF0 <= 0x20
            && !glitch_bit
            && !new_bit
            && !old_bit
            && effective_counter & 8 != 0
        {
            self.step_lfsr();
        }
    }

    /// The CGB-D's LFSR glitch when the selected counter bit stays 1 across an
    /// NR43 write.
    fn nr43_cgb_d_rising_glitch(&mut self, old: u8, new: u8, force_glitch: bool) {
        let map = if old & 0x80 != 0 {
            &CGB_D_HIGH_TO_LOW_GLITCHES
        } else {
            &CGB_D_LOW_TO_HIGH_GLITCHES
        };
        let mut glitch = map[(((old & 0x70) >> 1) | ((new & 0x70) >> 4)) as usize];
        if force_glitch {
            glitch = if (new ^ old) & 0x80 == 0 {
                if glitch & 0x20 != 0 { 5 } else { 0 }
            } else if new & 0x80 == 0 {
                if glitch & 0x10 != 0 { 5 } else { 0 }
            } else if glitch & 0xF == 1 || glitch & 0xF == 4 {
                5
            } else {
                0
            };
        } else {
            glitch &= 0xF;
        }

        let old_lfsr = self.state.noise.lfsr;
        let mask = self.lfsr_high_bit_mask();
        match glitch {
            1 | 2 | 4 | 5 | 6 | 8 => {
                let and_bit_1_with_bit_0 = match glitch {
                    2 => true,
                    4 => old_lfsr & 0x60 != 0x40,
                    6 => old_lfsr & 0x40 != 0x40,
                    _ => false,
                };
                if and_bit_1_with_bit_0 && old_lfsr & 1 == 0 {
                    self.state.noise.lfsr &= !2;
                }
                if glitch != 5 {
                    self.step_lfsr();
                }
                if glitch != 8 || old_lfsr & 3 != 2 {
                    self.state.noise.lfsr |= mask;
                } else {
                    self.state.noise.lfsr |= old_lfsr & mask;
                }
            }
            7 => {
                self.step_lfsr();
                self.state.noise.lfsr |= old_lfsr & mask;
            }
            3 => {
                self.step_lfsr();
                let noise = &mut self.state.noise;
                noise.lfsr &= old_lfsr;
                noise.lfsr |= old_lfsr & 1;
                noise.lfsr |= mask;
                self.update_noise_sample();
            }
            _ => {}
        }
    }

    /// The LFSR glitch of CGB-E and newer when the selected counter bit stays 1
    /// across an NR43 write.
    fn nr43_rising_glitch(&mut self, old: u8, new: u8, old_narrow: bool) {
        if new & 0x80 == 0 {
            self.step_lfsr();
            return;
        }

        // The low 3 bits of the counter bit NR43 selected before and after.
        let old_shift = counter_shift(old) & 7;
        let new_shift = counter_shift(new) & 7;
        if (old_shift ^ 7) + new_shift <= 7 && (old_shift ^ 7) & new_shift == 0 {
            return;
        }

        let noise = &mut self.state.noise;
        noise.lfsr &= !0x80;
        noise.lfsr |= (noise.lfsr >> 1) & 0x80;

        if (old_shift == 0 || old_shift == 4) && new_shift == 3 {
            noise.lfsr &= (noise.lfsr >> 1) | 0x545;
            self.update_noise_sample();
        } else if old_shift == 2 && new_shift == 3 {
            let mut mask = 0x555;
            if noise.lfsr & 0xC == 0xC {
                mask |= 8;
            }
            if noise.lfsr & 0xC00 == 0xC00 {
                mask |= 0x800;
            }
            noise.lfsr &= (noise.lfsr >> 1) | mask;
            self.update_noise_sample();
        }

        if !self.state.noise.narrow && old_narrow && self.state.lfsr_stepped_in_narrow {
            if self.state.lfsr_bit_7_before_step {
                self.state.noise.lfsr |= 0x40;
            } else {
                self.state.noise.lfsr &= !0x40;
            }
        }
        let mask = self.lfsr_high_bit_mask();
        self.state.noise.lfsr |= mask;
        self.state.lfsr_stepped_in_narrow = self.state.noise.narrow;
    }

    /// The LFSR glitch of CGB-E and newer when the selected counter bit stays 0
    /// across an NR43 write.
    fn nr43_falling_glitch(&mut self, old: u8, new: u8) {
        let glitch = if new & 0x80 != 0 {
            CGB_CATEGORY_2_GLITCHES[(((old & 0x70) >> 1) | ((new & 0x70) >> 4)) as usize]
        } else {
            0
        };
        let narrow = self.state.noise.narrow;

        match glitch {
            1 | 6 => {
                self.step_lfsr();
                let noise = &mut self.state.noise;
                if glitch == 6 {
                    if (narrow && noise.lfsr & 0x71 == 0x20) || noise.lfsr & 0x71 == 0x61 {
                        noise.lfsr &= !0x20;
                    }
                    if noise.lfsr & 0x7001 == 0x2000 || noise.lfsr & 0x7001 == 0x6001 {
                        noise.lfsr &= !0x2000;
                    }
                }
                if noise.lfsr & 0x3 == 2 {
                    noise.lfsr &= !2;
                }
            }
            2 => {
                let previous = self.state.noise.lfsr;
                self.step_lfsr();
                self.state.noise.lfsr &= previous | 1;
            }
            3 | 5 => {
                let noise = &mut self.state.noise;
                if glitch == 5 {
                    if noise.lfsr & 0x3 == 2 {
                        noise.lfsr &= if narrow { !0x4040 } else { !0x4000 };
                    }
                    if noise.lfsr & 0x19 == 8 {
                        noise.lfsr &= !8;
                    }
                }
                noise.lfsr &= !1;
                noise.lfsr |= (noise.lfsr >> 1) & 1;
                self.update_noise_sample();
                self.state.lfsr_stepped_in_narrow = narrow;
            }
            4 => {
                let previous = self.state.noise.lfsr;
                self.step_lfsr();
                self.state.noise.lfsr &= previous | if narrow { !0x2022 } else { !0x2002 };
            }
            _ => self.step_lfsr(),
        }
    }

    /// NR42 sets the volume envelope. Turning the DAC off (bits 3-7 all 0)
    /// stops the channel, and writes while it plays cause zombie mode.
    pub(super) fn write_nr42(&mut self, value: u8) {
        if !dac_enabled(value) {
            if self.state.is_active(Channel::Noise) && self.registers[NR43] & 7 != 0 {
                if self.state.noise.counter_countdown <= 2 {
                    // Without the 14-bit wrap of the other increments, as
                    // SameBoy: 0x3FFF becomes 0x4000, whose bit 14 never
                    // steps the LFSR (only the NR43 write glitches can see
                    // it), and the next count wraps it to 0x0001.
                    self.state.noise.counter = self.state.noise.counter.wrapping_add(1);
                }
                self.state.noise_counting = NoiseCounting::Stopped;
            } else if self.state.noise_counting == NoiseCounting::Running {
                self.state.noise_counting = NoiseCounting::Background;
            }
            self.registers[NR42] = value;
            self.silence(Channel::Noise);
        } else if self.state.is_active(Channel::Noise) {
            let old_value = self.registers[NR42];
            let passes_through_ff = self.model <= Model::CgbC;
            self.state
                .noise
                .envelope
                .zombie_mode_write(passes_through_ff, value, old_value);
            // `current_lfsr_sample`, as SameBoy, where an envelope tick uses
            // the LFSR's lowest bit instead (`tick_noise_envelope`). They
            // differ after a trigger that seeds the LFSR with 0x0055.
            let sample = if self.state.noise.current_lfsr_sample {
                self.state.noise.envelope.volume
            } else {
                0
            };
            self.update_sample(Channel::Noise, sample);
        }
    }

    /// NR43 sets the divider, narrow mode and the counter bit that clocks the
    /// LFSR. On CGB-C and older the register briefly holds 0xFF during a write.
    pub(super) fn write_nr43(&mut self, value: u8) {
        let alignment = (self.state.noise.alignment & 3) as usize;
        if self.state.noise.countdown_reloaded {
            let ticks_per_increment = ticks_per_counter_increment(value);
            let offsets = if self.model <= Model::CgbC {
                EARLY_NR43_RELOAD_OFFSETS
            } else {
                LATE_NR43_RELOAD_OFFSETS
            };
            self.state.noise.counter_countdown = ticks_per_increment
                + if ticks_per_increment == 2 {
                    0
                } else {
                    offsets[alignment]
                };
        }

        if self.model <= Model::CgbC {
            if self.state.noise.countdown_reloaded {
                let counter = self.state.noise.counter;
                let old_nr43 = self.registers[NR43];
                let old_bit = (counter >> counter_shift(old_nr43)) & 1 != 0;
                let glitch_bit = (counter >> 7) & 1 != 0;
                let new_bit = (counter >> counter_shift(value)) & 1 != 0;
                if !old_bit && new_bit && glitch_bit {
                    let previous_counter = counter.wrapping_sub(1) & 0x3FFF;
                    let old_bit = (previous_counter >> counter_shift(old_nr43)) & 1 != 0;
                    let glitch_bit = (previous_counter >> 7) & 1 != 0;
                    let new_bit = (previous_counter >> counter_shift(value)) & 1 != 0;
                    if old_bit && !new_bit && glitch_bit {
                        self.step_lfsr();
                    }
                }
            }
            self.apply_nr43_write(0xFF);
        }
        self.apply_nr43_write(value);
    }

    /// NR44 holds the length enable bit (6) and the trigger bit (7).
    pub(super) fn write_nr44(&mut self, value: u8) {
        if triggers(value) {
            self.state.noise.envelope.reset_on_trigger();
            if self.is_dmg() && self.state.noise.alignment & 3 != 0 {
                self.state.noise.dmg_delayed_start = DMG_DELAYED_START_TICKS;
            } else {
                self.start_noise();
            }
        }

        self.write_nrx4_length(Channel::Noise, value);
    }

    /// Restarts the noise channel: clears the LFSR and reloads the volume and
    /// envelope.
    fn start_noise(&mut self) {
        let nr42 = self.registers[NR42];
        self.state.noise.lfsr = 0;
        self.prepare_noise_start();

        let step_missed = self.envelope_step_missed(nr42);
        let noise = &mut self.state.noise;
        noise.envelope.restart(nr42, step_missed);
        noise.current_lfsr_sample = false;
        noise.did_step_counter = noise.alignment & 3 == 2;

        if dac_enabled(nr42) {
            self.state.set_active(Channel::Noise, true);
            self.update_sample(Channel::Noise, 0);
        }

        self.restart_length_if_run_out(Channel::Noise);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The LFSR after `steps` steps taken one at a time.
    fn lfsr_after_single_steps(mut lfsr: u16, narrow: bool, steps: u32) -> u16 {
        let high_bit_mask = if narrow { 0x4040 } else { 0x4000 };
        for _ in 0..steps {
            lfsr = next_lfsr(lfsr, high_bit_mask);
        }
        lfsr
    }

    #[test]
    fn steps_the_lfsr_many_times_at_once_like_one_at_a_time() {
        for lfsr in (0..=0xFFFF).step_by(97) {
            for narrow in [false, true] {
                for steps in 0..40 {
                    assert_eq!(
                        lfsr_after_steps(lfsr, narrow, steps),
                        lfsr_after_single_steps(lfsr, narrow, steps),
                        "LFSR {lfsr:04X}, narrow {narrow}, {steps} steps"
                    );
                }
            }
        }
    }

    #[test]
    fn counts_the_counter_increments_that_step_the_lfsr() {
        for bit in 0..16 {
            for counter in (0..=0x4000).step_by(37) {
                for increments in 1..300 {
                    let mut value = counter;
                    let mut edges = 0;
                    let mut last_is_edge = false;
                    for _ in 0..increments {
                        let old_bit = (value >> bit) & 1;
                        value = (value + 1) & 0x3FFF;
                        last_is_edge = old_bit == 0 && (value >> bit) & 1 == 1;
                        edges += u32::from(last_is_edge);
                    }
                    let expected = (edges > 0).then_some((edges, last_is_edge));
                    assert_eq!(rising_edges(counter, bit, increments), expected);
                }
            }
        }
    }
}
