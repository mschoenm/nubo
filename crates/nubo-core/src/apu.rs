/*
    The channel timing, register behavior and hardware quirks in this module and its submodules are
    ported from SameBoy's APU (https://github.com/LIJI32/SameBoy), which is what SameSuite's APU tests
    are verified against, including its differences between hardware revisions. SameBoy is licensed
    under the Expat License:

    Copyright (c) 2015-2026 Lior Halphon

    Permission is hereby granted, free of charge, to any person obtaining a copy
    of this software and associated documentation files (the "Software"), to deal
    in the Software without restriction, including without limitation the rights
    to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
    copies of the Software, and to permit persons to whom the Software is
    furnished to do so, subject to the following conditions:

    The above copyright notice and this permission notice shall be included in all
    copies or substantial portions of the Software.

    THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
    IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
    FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
    AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
    LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
    OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
    SOFTWARE.
*/

//! The APU (audio processing unit): the Game Boy's sound hardware.
//!
//! The APU has four sound channels: two square waves (channel 1 also has a
//! frequency sweep), a wave channel that plays 32 4-bit samples from wave RAM,
//! and a noise channel driven by an LFSR (linear-feedback shift register, a
//! cheap pseudo-random bit generator). The CPU controls them through the NRxx
//! registers at 0xFF10-0xFF26: NRxy is register `y` of channel `x`, so NR12 is
//! channel 1's volume envelope register, and NR50-NR52 are the global volume,
//! panning and power registers. Wave RAM sits at 0xFF30-0xFF3F.
//!
//! Timing works like this:
//!
//! - The channels count 2 MHz ticks: two per M-cycle (an M-cycle is four clock
//!   ticks of the 4 MHz CPU clock), or one per M-cycle in CGB double speed mode,
//!   where M-cycles are twice as fast.
//! - The frame sequencer drives the slower parts: length counters (which
//!   silence a channel after a set time), volume envelopes and channel 1's
//!   sweep. It steps on each falling edge of a bit of the DIV timer (the
//!   "DIV-APU" event, 512 times per second), which the address bus reports
//!   through `Apu::system_counter_changed`.
//!
//! Real hardware has many quirks that games and test ROMs depend on. The most
//! notable ones modelled here are:
//!
//! - "Zombie mode": writing an NRx2 (envelope) register while the channel plays
//!   changes its volume in odd but predictable ways instead of being ignored.
//! - PCM12 and PCM34 (0xFF76 and 0xFF77, CGB only) let the CPU read the current
//!   4-bit output of channels 1+2 and 3+4. On CGB-C and older, reads at certain
//!   moments return glitched values, modelled with a per-M-cycle mask.
//! - Many register writes and channel triggers behave differently depending on
//!   the exact 2 MHz phase and on the hardware revision (`Model`).
//!
//! To turn the channels' digital output into audio, each change of a channel's
//! level is added to a band-limited step (BLEP) buffer, see `blep`, and the
//! result goes through a high-pass filter like the capacitor on real hardware.
//!
//! The address bus steps the APU once per M-cycle, but the channels run lazily,
//! like SameBoy's: the M-cycles add up and run together when the audio buffers
//! fill up or before anything outside the channels' own timing reads or changes
//! them, with exactly the same result as running them one at a time (see
//! `catch_up`).
//!
//! Where a plainer name fits, the state is named differently from SameBoy's.
//! The main differences:
//!
//! | Nubo                                    | SameBoy                         |
//! |-----------------------------------------|---------------------------------|
//! | `frame_sequencer_step`                  | `div_divider`                   |
//! | `tick_parity`                           | `lf_div`                        |
//! | `DivEventSkip` (`Pending`)              | `skip_div_event` (`SKIP`)       |
//! | `active_channels` (one bit per channel) | `is_active[]`                   |
//! | `period` (square and wave)              | `sample_length`                 |
//! | `LengthCounter::remaining`              | `pulse_length`                  |
//! | `Envelope::volume`, `countdown`         | `current_volume`, `volume_countdown` |
//! | `Envelope::tick_pending`                | `envelope_clock.clock`          |
//! | `sample_suppressed`                     | `sample_surpressed`             |
//! | `sweep_pace_counter`                    | `square_sweep_countdown`        |
//! | `sweep_period_addend`                   | `sweep_length_addend`           |
//! | `sweep_shadow_period`                   | `shadow_sweep_sample_length`    |
//! | `WaveChannel::dac_enabled`              | `wave_channel.enable`           |
//! | `volume_shift`                          | `wave_channel.shift`            |
//! | `ticks_per_counter_increment` (noise)   | the noise `divisor`             |
//! | `sample_clock` (scaled by the rate)     | `apu_output.sample_cycles`      |

use crate::address_bus::MemoryMapped;
use crate::apu::blep::{BLEP_PHASES, BLEP_TABLE, BlepBuffer, MAX_PLAUSIBLE_LEVEL};
use crate::apu::envelope::{Envelope, dac_enabled, envelope_period};
use crate::apu::length::LengthCounter;
use crate::clock::{SYSTEM_COUNTER_TICKS_PER_M_CYCLE, dots_per_m_cycle};
use crate::model::Model;
use crate::serializable::{FieldWriter, Serializable, array_encoding, read_fields};
use crate::utils::is_bit_set;
use nubo_serializable_derive::Serializable;
use std::io::{self, Read, Write};

const CHANNEL_COUNT: usize = 4;

/// One of the four sound channels. Its number (`index`) is its bit in NR52
/// and in each half of NR51, and its position in per-channel arrays such as
/// `ApuState::samples`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Channel {
    Square1,
    Square2,
    Wave,
    Noise,
}

impl Channel {
    const ALL: [Channel; CHANNEL_COUNT] = [
        Channel::Square1,
        Channel::Square2,
        Channel::Wave,
        Channel::Noise,
    ];

    const fn index(self) -> usize {
        self as usize
    }
}

/// One of the two square channels, which work alike: channel 1 (which also
/// has the frequency sweep) or channel 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Square {
    Channel1,
    Channel2,
}

impl Square {
    const BOTH: [Square; 2] = [Square::Channel1, Square::Channel2];

    const fn channel(self) -> Channel {
        match self {
            Square::Channel1 => Channel::Square1,
            Square::Channel2 => Channel::Square2,
        }
    }

    /// The channel's NRx1 register (duty cycle and length): NR11 or NR21.
    /// Channel 2's registers are channel 1's five addresses on.
    const fn nrx1(self) -> usize {
        self.register(NR11)
    }

    /// The channel's NRx2 register (volume envelope): NR12 or NR22.
    const fn nrx2(self) -> usize {
        self.register(NR12)
    }

    /// The channel's NRx4 register (period high bits, length enable and
    /// trigger): NR14 or NR24.
    const fn nrx4(self) -> usize {
        self.register(NR14)
    }

    /// Channel 1's register `channel_1_register` for this channel.
    const fn register(self, channel_1_register: usize) -> usize {
        match self {
            Square::Channel1 => channel_1_register,
            Square::Channel2 => channel_1_register + (NR21 - NR11),
        }
    }
}

/// The largest period value: periods are 11 bits, the low 8 in NRx3 and the
/// top 3 in NRx4.
const MAX_PERIOD: u16 = 0x7FF;
/// The largest 4-bit volume or channel output.
const MAX_VOLUME: u8 = 15;

/// `2047 - period` for an 11-bit period value (NRx3 and NRx4), which the
/// hardware gets by inverting its bits. The channels' timers count down from
/// it to 0, so they step every `2048 - period` timer units.
const fn inverted_period(period: u16) -> u16 {
    period ^ MAX_PERIOD
}

/// NRx4 bit 7: the write triggers (restarts) the channel.
const fn triggers(nrx4: u8) -> bool {
    nrx4 & 0x80 != 0
}

/// A trigger this many 2 MHz ticks or less before the frame sequencer step that clocks the
/// envelopes doesn't let that step count towards the envelope period.
const ENVELOPE_STEP_MISSED_TICKS: u32 = 2;

const NR10: usize = 0x00;
const NR11: usize = 0x01;
const NR12: usize = 0x02;
const NR13: usize = 0x03;
const NR14: usize = 0x04;
const NR21: usize = 0x06;
const NR22: usize = 0x07;
const NR23: usize = 0x08;
const NR24: usize = 0x09;
const NR30: usize = 0x0A;
const NR31: usize = 0x0B;
const NR32: usize = 0x0C;
const NR33: usize = 0x0D;
const NR34: usize = 0x0E;
const NR41: usize = 0x10;
const NR42: usize = 0x11;
const NR43: usize = 0x12;
const NR44: usize = 0x13;
const NR50: usize = 0x14;
const NR51: usize = 0x15;
const NR52: usize = 0x16;
const WAVE_RAM_START: usize = 0x20;
const WAVE_RAM_END: usize = 0x2F;
const REGISTER_COUNT: usize = 0x30;

/// Bits that always read as 1 in each register, because they are unused or
/// write-only.
const READ_MASKS: [u8; WAVE_RAM_START] = [
    0x80, 0x3F, 0x00, 0xFF, 0xBF, 0xFF, 0x3F, 0x00, 0xFF, 0xBF, 0x7F, 0xFF, 0x9F, 0xFF, 0xBF, 0xFF,
    0xFF, 0x00, 0x00, 0xBF, 0x00, 0x00, 0x70, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
];

/// T-cycles (ticks of the 4 MHz CPU clock) per second at normal speed. In
/// double speed, M-cycles are twice as fast and count as 2 T-cycles here, so
/// this is the length of an emulated second at both speeds.
const T_CYCLES_PER_SECOND: u32 = 4_194_304;
const DEFAULT_SAMPLE_RATE: u32 = 44100;
/// The sample rates `set_sample_rate` accepts; it clamps others to this range.
/// Every real sound card's rate lies far inside it. Above the maximum, an
/// M-cycle (4 T-cycles) would last longer than an output sample.
const MIN_SAMPLE_RATE: u32 = 1_000;
const MAX_SAMPLE_RATE: u32 = T_CYCLES_PER_SECOND / 4;
/// How much of its charge the high-pass filter's capacitor keeps per T-cycle
/// (SameBoy's value).
const HIGHPASS_CHARGE_KEPT_PER_T_CYCLE: f64 = 0.999_958;
const MAX_AUDIO_BUFFER_SIZE: usize = 512;

/// Whether the frame sequencer skips a DIV-APU event after the APU is powered
/// on. When the DIV-APU bit is already set at power on, the first falling edge
/// is ignored and the step counter starts one step ahead.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serializable)]
enum DivEventSkip {
    /// Every DIV-APU event moves the frame sequencer on a step.
    #[default]
    Inactive,
    /// The skipped event has passed. The next one runs the step the counter
    /// already holds, without moving it on, and the events after it work as
    /// usual.
    Skipped,
    /// The next DIV-APU event does nothing.
    Pending,
}

/// Whether the noise channel's counter runs, and why. SameBoy keeps this as
/// two flags (`noise_counter_active` and `noise_background_counter_active`),
/// of which only these three combinations occur.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serializable)]
enum NoiseCounting {
    /// Before the first trigger, after powering off, and after the DAC is
    /// turned off while the channel plays with an NR43 divider code above 0.
    #[default]
    Stopped,
    /// In the background only: the channel was triggered with its DAC off, or
    /// the DAC was turned off later.
    Background,
    /// The channel was triggered with its DAC on.
    Running,
}

/// The state of one square wave channel (channel 1 or 2).
#[derive(Debug, Default, Clone, Copy, Serializable)]
struct SquareChannel {
    length: LengthCounter,
    envelope: Envelope,
    /// Position (0-7) in the duty cycle waveform.
    current_sample_index: u8,
    /// The level of the duty waveform at `current_sample_index`, latched when the position
    /// moves.
    duty_output_high: bool,
    /// Keeps the output silent right after a trigger until the first duty step.
    sample_suppressed: bool,
    /// 2 MHz ticks left before the next duty step.
    sample_countdown: u16,
    /// The 11-bit period value from NRx3 and NRx4; higher is a higher pitch.
    period: u16,
    /// Extra ticks the channel waits after a trigger.
    delay: u8,
    /// Whether the channel has stepped its duty position since the last trigger.
    did_tick: bool,
    /// Whether `sample_countdown` was reloaded at the very end of the last run,
    /// in which case a period write also updates the countdown.
    just_reloaded: bool,
}

/// The state of the wave channel (channel 3).
#[derive(Debug, Default, Clone, Copy, Serializable)]
struct WaveChannel {
    /// NR30 bit 7: the channel's DAC (digital-to-analog converter) is on.
    dac_enabled: bool,
    length: LengthCounter,
    /// How far the 4-bit samples are shifted right: 0 is full volume, 4 is mute.
    volume_shift: u8,
    /// The 11-bit period value from NR33 and NR34.
    period: u16,
    /// 2 MHz ticks left before the next sample is read.
    sample_countdown: u16,
    /// Position (0-31) of the current 4-bit sample in wave RAM.
    current_sample_index: u8,
    /// The wave RAM byte that holds the current sample.
    current_sample_byte: u8,
    /// Whether the channel read wave RAM during the current M-cycle. On DMG,
    /// the CPU can only access wave RAM at that moment while the channel plays.
    wave_form_just_read: bool,
    /// Whether the channel has been triggered since its DAC was last turned off.
    pulsed: bool,
    /// Ticks left before a delayed wave RAM read, used to model a DMG quirk
    /// where the channel reads whatever byte the CPU is accessing.
    bugged_read_countdown: u8,
}

/// The state of the noise channel (channel 4).
#[derive(Debug, Default, Clone, Copy, Serializable)]
struct NoiseChannel {
    length: LengthCounter,
    envelope: Envelope,
    /// The 15-bit linear-feedback shift register whose lowest bit is the output.
    lfsr: u16,
    /// NR43 bit 3: the LFSR acts as 7 bits wide, giving a more tonal noise.
    narrow: bool,
    /// Ticks left before `counter` increments.
    counter_countdown: u8,
    /// A 14-bit counter; the LFSR steps when the bit NR43 selects goes from 0
    /// to 1. Turning the DAC off can carry it past 0x3FFF (see `write_nr42`).
    counter: u16,
    /// Counts 2 MHz ticks (wrapping around); its low bits give the phase that
    /// many noise quirks depend on.
    alignment: u8,
    /// The LFSR's lowest bit as of its last step (false after a trigger),
    /// which an NR42 write plays at the new volume.
    current_lfsr_sample: bool,
    /// Whether `counter` has counted since the trigger (a trigger at
    /// alignment 2 counts as having counted), which changes a trigger glitch
    /// with NR43's divider code 1.
    did_step_counter: bool,
    /// Whether `counter_countdown` was reloaded at the very end of the last run.
    countdown_reloaded: bool,
    /// Ticks left before a delayed trigger, which happens on DMG when NR44 is
    /// written at certain phases.
    dmg_delayed_start: u8,
}

/// Everything about the APU that is reset when it is powered off through NR52,
/// except the register values themselves.
#[derive(Debug, Default, Clone, Copy, Serializable)]
struct ApuState {
    /// NR52 bit 7: the APU is powered on.
    powered_on: bool,
    /// Counts frame sequencer steps. Every second step clocks the length
    /// counters, every fourth step the sweep, and every eighth step counts down
    /// the envelopes.
    frame_sequencer_step: u8,
    /// Flips on every odd number of 2 MHz ticks, so it tells which half of a
    /// 1 MHz cycle the APU is in (SameBoy calls this `lf_div`).
    tick_parity: u8,
    div_event_skip: DivEventSkip,
    /// An envelope tick delayed to the start of the next M-cycle, which happens
    /// in double speed on CGB-D and CGB-E.
    pending_envelope_tick: bool,
    /// Counts the frame sequencer's sweep steps up to 7, wrapping to 0: it
    /// restarts at `7 - pace` (NR10 bits 4-6), so the sweep calculates on the
    /// `pace`th sweep step after.
    sweep_pace_counter: u8,
    /// 1 MHz cycles left before the running sweep calculation finishes
    /// (0 when none runs). It is NR10's shift: one cycle per shifted bit.
    square_sweep_calculate_countdown: u8,
    /// 1 MHz cycles before `square_sweep_calculate_countdown` starts counting
    /// after a sweep step or trigger reloads it. NR10 writes in this window
    /// disturb the calculation (`nr10_write_glitch`).
    square_sweep_calculate_countdown_reload_timer: u8,
    /// The amount the sweep adds to (or, negated, subtracts from) the period.
    sweep_period_addend: u16,
    /// The copy of channel 1's period the sweep calculates from.
    sweep_shadow_period: u16,
    /// Whether the current sweep calculation runs with a shift of 0.
    unshifted_sweep: bool,
    /// A sweep step started a calculation with a shift of 0, which finishes
    /// as soon as the reload timer runs out.
    square_sweep_instant_calculation_done: bool,
    /// Ticks after a channel 1 trigger during which the sweep keeps its old
    /// values.
    channel_1_restart_hold: u8,
    /// The addend of the last finished sweep calculation, used for the overflow
    /// check when NR10 is written.
    channel_1_completed_addend: u16,
    square1: SquareChannel,
    square2: SquareChannel,
    wave: WaveChannel,
    noise: NoiseChannel,
    noise_counting: NoiseCounting,
    /// Whether the LFSR's last step was in narrow mode, which an NR43 write
    /// that leaves narrow mode on CGB-E and newer depends on.
    lfsr_stepped_in_narrow: bool,
    /// The LFSR's bit 7 before its last step, which that NR43 write puts back
    /// into bit 6.
    lfsr_bit_7_before_step: bool,
    /// The last trigger came with the DAC off, so only the background counter
    /// started, which shifts a later trigger's counter timing.
    noise_started_with_dac_disabled: bool,
    /// One bit per channel: whether it is playing (the low bits of NR52).
    active_channels: u8,
    /// The current 4-bit digital output of each channel.
    samples: [u8; CHANNEL_COUNT],
}

impl ApuState {
    fn square(&self, square: Square) -> &SquareChannel {
        match square {
            Square::Channel1 => &self.square1,
            Square::Channel2 => &self.square2,
        }
    }

    fn square_mut(&mut self, square: Square) -> &mut SquareChannel {
        match square {
            Square::Channel1 => &mut self.square1,
            Square::Channel2 => &mut self.square2,
        }
    }

    fn is_active(&self, channel: Channel) -> bool {
        self.active_channels & (1 << channel.index()) != 0
    }

    fn set_active(&mut self, channel: Channel, active: bool) {
        if active {
            self.active_channels |= 1 << channel.index();
        } else {
            self.active_channels &= !(1 << channel.index());
        }
    }

    /// Rejects values the emulator never produces, so a corrupted save state
    /// can't make the APU index out of bounds, overflow or divide by zero
    /// later.
    fn validate(&self) -> io::Result<()> {
        let squares_valid = [&self.square1, &self.square2].iter().all(|square| {
            square.length.is_valid(Channel::Square1.full_length())
                && square.envelope.is_valid()
                && square.current_sample_index < 8
                && square.period <= MAX_PERIOD
        });
        let wave = &self.wave;
        let wave_valid = wave.length.is_valid(Channel::Wave.full_length())
            && wave.current_sample_index < 32
            && wave.period <= MAX_PERIOD
            && wave.volume_shift <= 4;
        let noise = &self.noise;
        let noise_valid = noise.length.is_valid(Channel::Noise.full_length())
            && noise.envelope.is_valid()
            // 14 bits, which NR42's increments without the wrap (see
            // `write_nr42`) can carry a little past.
            && noise.counter < 0x8000;
        let sweep_valid = self.sweep_pace_counter <= 7
            && self.square_sweep_calculate_countdown <= 7
            && self.square_sweep_calculate_countdown_reload_timer <= 4
            && self.sweep_period_addend <= MAX_PERIOD
            && self.sweep_shadow_period <= MAX_PERIOD
            && self.channel_1_completed_addend <= MAX_PERIOD
            && self.channel_1_restart_hold <= 4;
        let outputs_valid =
            self.active_channels <= 0x0F && self.samples.iter().all(|&sample| sample <= MAX_VOLUME);
        if squares_valid
            && wave_valid
            && noise_valid
            && sweep_valid
            && outputs_valid
            && self.tick_parity <= 1
        {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "APU state out of range",
            ))
        }
    }
}

/// The Game Boy's audio processing unit. It runs the four channels, mixes their
/// output and queues the resulting stereo samples for the frontend.
///
/// Its fields are private to `apu` and its submodules, so code elsewhere can
/// only reach the APU's state through methods that run the pending M-cycles
/// first (see `catch_up`).
///
/// `Clone` and `Debug` are written out in `catch_up`: copies for reads can
/// leave out the sample queues, and `Debug` prints the state with the pending
/// M-cycles run and without the catch-up's scheduling.
pub struct Apu {
    /// The NRxx registers and wave RAM, indexed by address minus 0xFF10.
    registers: [u8; REGISTER_COUNT],
    state: ApuState,
    model: Model,
    cgb_mode: bool,
    cgb_double_speed: bool,
    /// Masks ANDed into PCM12 and PCM34 reads during the current M-cycle,
    /// modelling a read glitch of CGB-C and older.
    pcm_mask: [u8; 2],
    system_counter: u16,
    /// The address on the CPU's memory bus, which some wave channel glitches read.
    bus_address: u16,
    /// How far the output is into the current output sample. Each T-cycle adds
    /// `sample_rate`, and a whole sample is `T_CYCLES_PER_SECOND`, so exactly
    /// `sample_rate` samples come per emulated second, without rounding. As a
    /// fraction of a sample, it means the same at every sample rate.
    sample_clock: u32,
    /// In double speed, DIV events reach the frame sequencer one M-cycle
    /// late. This flips each time the CPU enters double speed, survives DIV
    /// writes, and clears when the APU is powered on (AGE's spsw-ch2-lc-delay
    /// tests and Gambatte's speedchange ch2_nr52 tests, without breaking
    /// SameSuite, which powers the APU on after its speed switch).
    late_div_events: bool,
    /// A frame sequencer step held back by `late_div_events`, and whether it
    /// came from a DIV write.
    pending_div_event: Option<bool>,
    /// Whether `pending_div_event` or `state.pending_envelope_tick` holds an
    /// event back to the next M-cycle, so the start of an M-cycle only has to
    /// check this.
    held_back_events: bool,
    /// The bit of the system counter whose falling edge steps the frame
    /// sequencer, kept up to date with `cgb_double_speed` (see `div_apu_bit`).
    div_apu_bit: u16,
    left_sample_queue: Vec<f32>,
    right_sample_queue: Vec<f32>,
    previous_left_outputs: [f32; CHANNEL_COUNT],
    previous_right_outputs: [f32; CHANNEL_COUNT],
    /// Everything the channel outputs were last worked out from (`output_inputs`), so
    /// `process_channel_deltas` can skip the channels and M-cycles in which none of it changed.
    /// `None` forces the next M-cycle to work them all out.
    previous_output_inputs: Option<u128>,
    /// The 2 MHz ticks until each channel's output can next change
    /// (`NO_OUTPUT_CHANGE` when it can't), counted down as the channels run,
    /// or `None` when it has to be worked out again.
    next_output_changes: [Option<u32>; CHANNEL_COUNT],
    blep_left: BlepBuffer,
    blep_right: BlepBuffer,
    /// The charge of the high-pass filter's capacitor on each side.
    highpass_left_charge: f32,
    highpass_right_charge: f32,
    /// How much of its charge the capacitor keeps per output sample.
    highpass_rate: f32,
    /// Output samples per second, chosen by the frontend to match its sound card.
    sample_rate: u32,
    /// M-cycles the address bus has stepped the APU through that the channels haven't run yet.
    pending_cycles: u32,
    /// What `exact_cycles_until_audio_buffers_full` last worked out, kept until the audio output
    /// moves on or changes. `None` when it has to be worked out again.
    audio_buffers_full_in: Option<u32>,
    /// How many M-cycles may be pending before `step` runs them, worked out by
    /// `schedule_catch_up`.
    catch_up_deadline: u32,
    /// Whether something outside the channels' own timing changed them since
    /// `schedule_catch_up` last ran, in which case `catch_up_deadline` is 1 until
    /// it is worked out again.
    schedule_outdated: bool,
    /// Whether the address bus has stepped the APU in the current M-cycle, so the M-cycle's own
    /// run is done or pending.
    stepped_this_cycle: bool,
}

impl Apu {
    pub(crate) fn new() -> Self {
        let sample_rate = DEFAULT_SAMPLE_RATE;
        Apu {
            registers: [0; REGISTER_COUNT],
            state: ApuState::default(),
            model: Model::Dmg,
            cgb_mode: false,
            cgb_double_speed: false,
            pcm_mask: [0xFF; 2],
            system_counter: 0,
            bus_address: 0,
            sample_clock: 0,
            late_div_events: false,
            pending_div_event: None,
            held_back_events: false,
            div_apu_bit: div_apu_bit(false),
            left_sample_queue: Vec::new(),
            right_sample_queue: Vec::new(),
            previous_left_outputs: [0.0; CHANNEL_COUNT],
            previous_right_outputs: [0.0; CHANNEL_COUNT],
            previous_output_inputs: None,
            next_output_changes: [None; CHANNEL_COUNT],
            blep_left: BlepBuffer::new(),
            blep_right: BlepBuffer::new(),
            highpass_left_charge: 0.0,
            highpass_right_charge: 0.0,
            highpass_rate: highpass_rate(sample_rate),
            sample_rate,
            pending_cycles: 0,
            audio_buffers_full_in: None,
            catch_up_deadline: 1,
            schedule_outdated: true,
            stepped_this_cycle: false,
        }
    }

    /// Sets the hardware revision the APU emulates. Many quirks differ between
    /// revisions, as in SameBoy (which SameSuite's tests check), so the APU
    /// asks `model` wherever they do.
    pub(crate) fn set_model(&mut self, model: Model) {
        self.prepare_for_change();
        self.model = model;
        self.cgb_mode = model.is_cgb();
        self.previous_output_inputs = None;
    }

    /// Moves the duty cycle of the boot ROM's sound (on channel 1) `ticks`
    /// 2 MHz ticks back without changing anything else.
    pub(crate) fn delay_boot_sound_phase(&mut self, ticks: i32) {
        self.prepare_for_change();
        let square = self.state.square_mut(Square::Channel1);
        let step = i32::from(inverted_period(square.period) * 2 + 2);
        let elapsed = i32::from(square.current_sample_index) * step + step
            - 1
            - i32::from(square.sample_countdown).min(step - 1);
        let shifted = (elapsed - ticks).rem_euclid(step * 8);
        square.current_sample_index =
            u8::try_from(shifted / step).expect("the duty cycle has 8 steps");
        square.sample_countdown =
            u16::try_from(step - 1 - shifted % step).expect("less than one duty step");
        self.latch_duty_output(Square::Channel1);
    }

    /// Moves the frame sequencer `steps` steps on without running them.
    pub(crate) fn advance_frame_sequencer(&mut self, steps: u8) {
        self.prepare_for_change();
        self.state.frame_sequencer_step = self.state.frame_sequencer_step.wrapping_add(steps);
    }

    /// Stops the channel 1 sound the boot ROM plays, for when the boot ROM is
    /// skipped.
    pub(crate) fn silence_boot_sound(&mut self) {
        self.prepare_for_change();
        self.silence(Channel::Square1);
    }

    pub(crate) fn set_cgb_double_speed(&mut self, cgb_double_speed: bool) {
        self.prepare_for_change();
        if cgb_double_speed && !self.cgb_double_speed {
            self.late_div_events = !self.late_div_events;
        }
        if !cgb_double_speed && let Some(during_div_write) = self.pending_div_event.take() {
            self.step_frame_sequencer(during_div_write);
        }
        self.cgb_double_speed = cgb_double_speed;
        self.audio_buffers_full_in = None;
        self.div_apu_bit = div_apu_bit(cgb_double_speed);
        self.update_held_back_events();
    }

    /// Tells the APU the address the CPU accesses in the current M-cycle,
    /// which the timer of a stopped wave channel latches wave RAM bytes from
    /// (except on AGB).
    ///
    /// Unlike other changes this doesn't catch the channels up first, though
    /// their runs read it. That is right because the M-cycles whose runs read
    /// it are never batched: `cycles_until_timing_dependent_run` makes each of
    /// them run at its own `step`, and turns off the bulk runs and deferred
    /// M-cycles of the address bus (`can_advance_in_bulk`) meanwhile, so each
    /// run reads the address of the last access before it.
    pub(crate) fn set_bus_address(&mut self, address: u16) {
        self.bus_address = address;
    }

    fn is_dmg(&self) -> bool {
        !self.cgb_mode
    }

    fn is_agb(&self) -> bool {
        self.model > Model::CgbE
    }

    /// Whether a channel's DAC (digital-to-analog converter) is on. A channel
    /// with its DAC off can't play. The AGB has no per-channel DAC switch.
    fn is_dac_enabled(&self, channel: Channel) -> bool {
        if self.is_agb() {
            return true;
        }
        match channel {
            Channel::Square1 => dac_enabled(self.registers[NR12]),
            Channel::Square2 => dac_enabled(self.registers[NR22]),
            Channel::Wave => self.state.wave.dac_enabled,
            Channel::Noise => dac_enabled(self.registers[NR42]),
        }
    }

    /// Sets a channel's digital output, which only changes while its DAC is on.
    fn update_sample(&mut self, channel: Channel, value: u8) {
        let index = channel.index();
        if value == 0 && self.state.samples[index] == 0 {
            return;
        }
        if self.is_dac_enabled(channel) {
            self.state.samples[index] = value;
        }
    }

    /// Turns a channel off, as its length counter running out or its DAC
    /// being turned off does: it stops playing (its NR52 bit clears), and its
    /// output drops to 0 unless its DAC is off, which holds the output.
    fn silence(&mut self, channel: Channel) {
        self.state.set_active(channel, false);
        self.update_sample(channel, 0);
    }

    /// Resets the APU as NR52 turns it on. If the DIV-APU bit is set by the
    /// end of the next M-cycle, the frame sequencer skips its first step
    /// (SameBoy; when the bit is sampled follows Gambatte's
    /// ch2_late_reset_nr52 tests).
    fn reset_for_power_on(&mut self) {
        self.state = ApuState::default();
        self.state.tick_parity = 1;
        self.state.wave.volume_shift = 4;
        let next_cycle_counter = self
            .system_counter
            .wrapping_add(SYSTEM_COUNTER_TICKS_PER_M_CYCLE);
        if next_cycle_counter & self.div_apu_bit != 0 {
            self.state.div_event_skip = DivEventSkip::Pending;
            self.state.frame_sequencer_step = 1;
        }
        self.state.square1.sample_countdown = 0xFFFF;
        self.state.square2.sample_countdown = 0xFFFF;
    }

    /// Whether the next frame sequencer step comes within `ticks` 2 MHz ticks and is one of the
    /// steps selected by `mask` (3 for the sweep's steps, 7 for the envelopes' step).
    fn next_step_is_imminent(&self, mask: u8, ticks: u32) -> bool {
        let edge_period = u32::from(self.div_apu_bit) * 2;
        let units_per_tick = if self.cgb_double_speed { 4 } else { 2 };
        let ticks_until_edge =
            (edge_period - (u32::from(self.system_counter) & (edge_period - 1))) / units_per_tick;
        self.state.frame_sequencer_step.wrapping_add(1) & mask == mask && ticks_until_edge <= ticks
    }

    /// Whether a trigger is too close to the next frame sequencer step to let it count towards the
    /// envelope's first period, when that step clocks the envelopes and the envelope has a period
    /// (measured against Gambatte's envelope unit; Gambatte's sound
    /// ch2_init_reset_env_counter_timing tests).
    fn envelope_step_missed(&self, nrx2: u8) -> bool {
        envelope_period(nrx2) != 0 && self.next_step_is_imminent(7, ENVELOPE_STEP_MISSED_TICKS)
    }

    /// Called whenever the system counter changes, including on DIV writes.
    /// Steps the frame sequencer on a falling edge of the DIV-APU bit and
    /// prepares envelope ticks on a rising edge.
    #[inline]
    pub(crate) fn system_counter_changed(&mut self, old: u16, new: u16, during_div_write: bool) {
        self.system_counter = new;
        if (old ^ new) & self.div_apu_bit != 0 {
            self.div_apu_bit_changed(old, new, during_div_write);
        }
    }

    fn div_apu_bit_changed(&mut self, old: u16, new: u16, during_div_write: bool) {
        let apu_bit = self.div_apu_bit;
        if old & !new & apu_bit != 0 {
            if self.cgb_double_speed && self.late_div_events {
                self.pending_div_event = Some(during_div_write);
                self.held_back_events = true;
            } else {
                let quiet = self.frame_sequencer_step_is_quiet();
                if !quiet {
                    self.prepare_for_change();
                }
                let checked = (cfg!(debug_assertions) && quiet).then(|| self.what_runs_read());
                self.step_frame_sequencer(during_div_write);
                if let Some(before) = checked {
                    debug_assert_eq!(
                        before,
                        self.what_runs_read(),
                        "a frame sequencer step judged quiet changed what the channels' runs read"
                    );
                }
            }
        } else if !old & new & apu_bit != 0 {
            // Preparing the envelope ticks changes nothing the channels' runs
            // read or put out, so the pending M-cycles can run later.
            if self.stepped_this_cycle {
                self.prepare_for_change();
            }
            self.prepare_envelope_ticks();
        }
    }

    /// Whether the frame sequencer step that comes now changes nothing the
    /// channels' runs read or put out, so the pending M-cycles can run later:
    /// no envelope ticks, no length counter runs out and the sweep doesn't
    /// calculate. Like preparing the envelope ticks, the step also clears the
    /// PCM read glitches, which only gives the same result before the address
    /// bus has stepped the APU in the current M-cycle.
    fn frame_sequencer_step_is_quiet(&self) -> bool {
        if self.stepped_this_cycle {
            return false;
        }
        let Some(step) = self.upcoming_frame_sequencer_step() else {
            return true;
        };
        let state = &self.state;
        let envelope_ticks = !self.envelope_ticks_wait_a_cycle()
            && (state.square1.envelope.tick_pending()
                || state.square2.envelope.tick_pending()
                || state.noise.envelope.tick_pending());
        let length_runs_out = clocks_lengths(step)
            && Channel::ALL
                .into_iter()
                .any(|channel| self.length_counter(channel).runs_out_on_next_clock());
        let sweep_calculates =
            clocks_sweep(step) && self.sweep_pace_elapsed((state.sweep_pace_counter + 1) & 7);
        !envelope_ticks && !length_runs_out && !sweep_calculates
    }

    /// What the channels' runs read and put out that a frame sequencer step
    /// can change, for checking that a step `frame_sequencer_step_is_quiet`
    /// let through changed none of it: which channels play, their outputs,
    /// channel 1's period, the sweep calculation's timers and the volumes.
    fn what_runs_read(&self) -> (u8, [u8; CHANNEL_COUNT], u16, u8, u8, [u8; 3]) {
        let state = &self.state;
        (
            state.active_channels,
            state.samples,
            state.square1.period,
            state.square_sweep_calculate_countdown,
            state.square_sweep_calculate_countdown_reload_timer,
            [
                state.square1.envelope.volume,
                state.square2.envelope.volume,
                state.noise.envelope.volume,
            ],
        )
    }

    /// The number the frame sequencer's step counter has after the coming DIV
    /// event, or `None` when that event doesn't step it: while the APU is
    /// off, and for the event skipped after powering on.
    fn upcoming_frame_sequencer_step(&self) -> Option<u8> {
        if !self.state.powered_on {
            return None;
        }
        let step = self.state.frame_sequencer_step;
        match self.state.div_event_skip {
            DivEventSkip::Pending => None,
            DivEventSkip::Skipped => Some(step),
            DivEventSkip::Inactive => Some(step.wrapping_add(1)),
        }
    }

    /// Whether the envelope ticks of a frame sequencer step wait for the start
    /// of the next M-cycle, as in double speed on CGB-D and CGB-E.
    fn envelope_ticks_wait_a_cycle(&self) -> bool {
        self.cgb_double_speed && matches!(self.model, Model::CgbD | Model::CgbE)
    }

    /// Starts a new M-cycle: clears the PCM read glitches of the last one and
    /// runs the frame sequencer events that were held back to it.
    #[inline]
    pub(crate) fn begin_machine_cycle(&mut self) {
        self.pcm_mask = [0xFF; 2];
        self.stepped_this_cycle = false;
        if self.held_back_events {
            self.run_held_back_events();
        }
    }

    fn run_held_back_events(&mut self) {
        self.prepare_for_change();
        if let Some(during_div_write) = self.pending_div_event.take() {
            self.step_frame_sequencer(during_div_write);
        }
        if self.state.pending_envelope_tick {
            self.run_delayed_envelope_ticks();
        }
        self.update_held_back_events();
    }

    fn update_held_back_events(&mut self) {
        self.held_back_events =
            self.pending_div_event.is_some() || self.state.pending_envelope_tick;
    }

    fn run_delayed_envelope_ticks(&mut self) {
        self.state.pending_envelope_tick = false;
        if !self.state.powered_on {
            return;
        }
        self.tick_pending_envelopes();
    }

    fn tick_pending_envelopes(&mut self) {
        for square in Square::BOTH {
            if self.state.square(square).envelope.tick_pending() {
                self.tick_square_envelope(square);
            }
        }
        if self.state.noise.envelope.tick_pending() {
            self.tick_noise_envelope();
        }
    }

    /// Runs one frame sequencer step: envelopes, length counters and the sweep,
    /// each at its own rate.
    fn step_frame_sequencer(&mut self, during_div_write: bool) {
        self.pcm_mask = [0xFF; 2];
        let Some(step) = self.upcoming_frame_sequencer_step() else {
            if self.state.powered_on {
                self.state.div_event_skip = DivEventSkip::Skipped;
            }
            return;
        };
        self.state.div_event_skip = DivEventSkip::Inactive;
        self.state.frame_sequencer_step = step;

        if clocks_envelopes(step) {
            self.state.square1.envelope.count_down();
            self.state.square2.envelope.count_down();
            self.state.noise.envelope.count_down();
        }

        if self.envelope_ticks_wait_a_cycle() {
            self.state.pending_envelope_tick = true;
            self.held_back_events = true;
        } else {
            self.tick_pending_envelopes();
        }

        if clocks_lengths(step) {
            self.clock_length_counters();
        }

        if clocks_sweep(step) {
            self.state.sweep_pace_counter = (self.state.sweep_pace_counter + 1) & 7;
            self.trigger_sweep_calculation(during_div_write);
        }
    }

    /// Reloads the envelope countdowns that ran out and decides whether each
    /// envelope ticks on the next frame sequencer envelope step. Runs on the
    /// rising edge of the DIV-APU bit, half a step before the frame sequencer.
    fn prepare_envelope_ticks(&mut self) {
        self.pcm_mask = [0xFF; 2];
        if !self.state.powered_on {
            return;
        }
        for square in Square::BOTH {
            if self.state.is_active(square.channel()) {
                let nrx2 = self.registers[square.nrx2()];
                self.state.square_mut(square).envelope.prepare_tick(nrx2);
            }
        }
        if self.state.is_active(Channel::Noise) {
            self.state.noise.envelope.prepare_tick(self.registers[NR42]);
        }
    }

    /// Advances every channel by `ticks` 2 MHz ticks.
    fn run(&mut self, mut ticks: u16) {
        if ticks == 0 {
            return;
        }

        if self.state.wave.bugged_read_countdown > 0 {
            for _ in 0..ticks {
                self.state.wave.bugged_read_countdown -= 1;
                if self.state.wave.bugged_read_countdown == 0 {
                    self.state.wave.current_sample_byte =
                        self.registers[WAVE_RAM_START + (self.bus_address as usize & 0xF)];
                    if self.state.is_active(Channel::Wave) {
                        self.update_wave_sample();
                    }
                    break;
                }
            }
        }

        let mut start_noise = false;
        if self.state.noise.dmg_delayed_start > 0 {
            let delay = u16::from(self.state.noise.dmg_delayed_start);
            if delay == ticks {
                self.state.noise.dmg_delayed_start = 0;
                start_noise = true;
            } else if delay > ticks {
                self.state.noise.dmg_delayed_start -=
                    u8::try_from(ticks).expect("less than the delay");
            } else {
                ticks -= delay;
                self.run(delay);
            }
        }

        self.state.tick_parity ^= (ticks & 1) as u8;
        // Only the low bits of the alignment matter, so it counts modulo 256
        // and the ticks can be cut to their low byte.
        self.state.noise.alignment = self.state.noise.alignment.wrapping_add(ticks as u8);

        self.run_sweep(ticks);

        if self.state.channel_1_restart_hold > 0 {
            self.state.channel_1_restart_hold = self
                .state
                .channel_1_restart_hold
                .saturating_sub(u8::try_from(ticks).unwrap_or(u8::MAX));
        }

        for square in Square::BOTH {
            if self.state.is_active(square.channel()) {
                self.run_square(square, ticks);
            }
        }

        self.run_wave(ticks);
        self.run_noise(ticks);

        if start_noise {
            let nr44 = self.registers[NR44] | 0x80;
            self.write_register(NR44, nr44);
        }
    }

    /// The 2 MHz ticks in one M-cycle: two, or one in double speed.
    fn ticks_per_cycle(&self) -> u16 {
        if self.cgb_double_speed { 1 } else { 2 }
    }

    /// Moves the APU on by one M-cycle. The M-cycle only counts as pending
    /// until `catch_up_deadline` of them have built up, then they all run.
    #[inline]
    pub(crate) fn step(&mut self) {
        self.pending_cycles += 1;
        self.stepped_this_cycle = true;
        if self.pending_cycles >= self.catch_up_deadline {
            self.catch_up_when_due();
        }
    }

    /// Runs one M-cycle while the CPU is in STOP mode. The CGB's channels
    /// keep playing while those of monochrome models stand still; DIV doesn't
    /// count, so the frame sequencer stands still on both (SameBoy).
    pub(crate) fn step_while_stopped(&mut self) {
        if self.model.is_cgb() {
            self.step();
        } else {
            self.prepare_for_change();
            self.enqueue_audio_samples();
        }
    }

    /// A channel's current output, or 0 when it isn't playing.
    fn audible_sample(&self, channel: Channel) -> u8 {
        if self.state.is_active(channel) {
            self.state.samples[channel.index()]
        } else {
            0
        }
    }

    /// The PCM12 register (0xFF76): channel 1's output in the low nibble and
    /// channel 2's in the high nibble.
    pub(crate) fn pcm12(&self) -> u8 {
        let apu = self.caught_up();
        (apu.audible_sample(Channel::Square1) | (apu.audible_sample(Channel::Square2) << 4))
            & apu.effective_pcm_mask(0)
    }

    /// The PCM34 register (0xFF77): channel 3's output in the low nibble and
    /// channel 4's in the high nibble.
    pub(crate) fn pcm34(&self) -> u8 {
        let apu = self.caught_up();
        (apu.audible_sample(Channel::Wave) | (apu.audible_sample(Channel::Noise) << 4))
            & apu.effective_pcm_mask(1)
    }

    fn effective_pcm_mask(&self, index: usize) -> u8 {
        if self.model <= Model::CgbC {
            self.pcm_mask[index]
        } else {
            0xFF
        }
    }

    fn read_register(&self, register: usize) -> u8 {
        if register == NR52 {
            let channels = self.state.active_channels & 0x0F;
            let powered_on = if self.state.powered_on { 0x80 } else { 0 };
            return powered_on | 0x70 | channels;
        }

        if (WAVE_RAM_START..=WAVE_RAM_END).contains(&register) {
            if self.state.is_active(Channel::Wave) {
                if (self.is_dmg() && !self.state.wave.wave_form_just_read) || self.is_agb() {
                    return 0xFF;
                }
                return self.registers
                    [WAVE_RAM_START + self.state.wave.current_sample_index as usize / 2];
            }
            return self.registers[register];
        }

        self.registers[register] | READ_MASKS[register]
    }

    fn write_register(&mut self, mut register: usize, mut value: u8) {
        let is_length_register = matches!(register, NR11 | NR21 | NR31 | NR41);
        if !self.state.powered_on
            && register != NR52
            && register < WAVE_RAM_START
            && (self.cgb_mode || !is_length_register)
        {
            return;
        }

        if (WAVE_RAM_START..=WAVE_RAM_END).contains(&register)
            && self.state.is_active(Channel::Wave)
        {
            if (self.is_dmg() && !self.state.wave.wave_form_just_read) || self.is_agb() {
                return;
            }
            register = WAVE_RAM_START + self.state.wave.current_sample_index as usize / 2;
        }

        match register {
            NR50 | NR51 => {}
            NR52 => self.write_nr52(value),
            NR10 => self.write_nr10(value),
            NR11 => value = self.write_square_nrx1(Square::Channel1, value),
            NR21 => value = self.write_square_nrx1(Square::Channel2, value),
            NR12 => self.write_square_nrx2(Square::Channel1, value),
            NR22 => self.write_square_nrx2(Square::Channel2, value),
            NR13 => self.write_square_nrx3(Square::Channel1, value),
            NR23 => self.write_square_nrx3(Square::Channel2, value),
            NR14 => self.write_square_nrx4(Square::Channel1, value),
            NR24 => self.write_square_nrx4(Square::Channel2, value),
            NR30 => self.write_nr30(value),
            NR31 => self.load_length(Channel::Wave, value),
            NR32 => self.write_nr32(value),
            NR33 => self.write_nr33(value),
            NR34 => self.write_nr34(value),
            NR41 => self.load_length(Channel::Noise, value),
            NR42 => self.write_nr42(value),
            NR43 => self.write_nr43(value),
            NR44 => self.write_nr44(value),
            _ => {}
        }

        self.registers[register] = value;
    }

    /// Powers the APU on or off. Powering off clears every register except wave
    /// RAM; on DMG the length counters survive.
    fn write_nr52(&mut self, value: u8) {
        let old_lengths = Channel::ALL.map(|channel| self.length_counter(channel).remaining);

        if value & 0x80 != 0 && !self.state.powered_on {
            self.late_div_events = false;
            self.reset_for_power_on();
            self.state.powered_on = true;
        } else if value & 0x80 == 0 && self.state.powered_on {
            self.state = ApuState::default();
            self.registers[..WAVE_RAM_START].fill(0);
        }

        if self.is_dmg() && value & 0x80 != 0 {
            self.state.square1.length.remaining = old_lengths[0];
            self.state.square2.length.remaining = old_lengths[1];
            self.state.wave.length.remaining = old_lengths[2];
            self.state.noise.length.remaining = old_lengths[3];
        }
        self.update_held_back_events();
    }

    /// Whether both sample queues hold a whole buffer (`MAX_AUDIO_BUFFER_SIZE`
    /// samples). `Emulator::step_until_next_audio_buffer` runs the emulator
    /// until this is true and then hands the samples to the frontend, so the
    /// audio output paces the main emulation loop.
    pub(crate) fn audio_buffers_full(&self) -> bool {
        self.left_sample_queue.len() >= MAX_AUDIO_BUFFER_SIZE
            && self.right_sample_queue.len() >= MAX_AUDIO_BUFFER_SIZE
    }

    /// Empties the audio buffers, after queueing the samples of the M-cycles
    /// that already passed.
    pub(crate) fn clear_audio_buffers(&mut self) {
        self.prepare_for_change();
        self.left_sample_queue.clear();
        self.right_sample_queue.clear();
        self.audio_buffers_full_in = None;
    }

    /// Moves every sample made so far, including those of the M-cycles that
    /// already passed, to the end of `left` and `right`, and empties the
    /// buffers.
    pub(crate) fn take_samples(&mut self, left: &mut Vec<f32>, right: &mut Vec<f32>) {
        self.prepare_for_change();
        left.append(&mut self.left_sample_queue);
        right.append(&mut self.right_sample_queue);
        self.audio_buffers_full_in = None;
    }

    /// The queued left samples. They are only complete once the buffers are
    /// full (see `schedule_catch_up`), which is when the emulator hands them
    /// out.
    pub(crate) fn left_samples(&self) -> &[f32] {
        self.left_sample_queue.as_slice()
    }

    /// The queued right samples, complete once the buffers are full.
    pub(crate) fn right_samples(&self) -> &[f32] {
        self.right_sample_queue.as_slice()
    }

    /// Sets how many output samples per emulated second the APU queues,
    /// clamped to `MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE`. The rate belongs to the
    /// frontend's sound card, not to the console, so save states don't hold it.
    pub(crate) fn set_sample_rate(&mut self, sample_rate: u32) {
        self.prepare_for_change();
        self.sample_rate = sample_rate.clamp(MIN_SAMPLE_RATE, MAX_SAMPLE_RATE);
        self.audio_buffers_full_in = None;
        self.highpass_rate = highpass_rate(self.sample_rate);
    }

    /// How much `sample_clock` advances per M-cycle: `sample_rate` for each of
    /// its T-cycles. It is at most `T_CYCLES_PER_SECOND`, one output sample.
    fn sample_clock_per_cycle(&self) -> u32 {
        u32::from(dots_per_m_cycle(self.cgb_double_speed)) * self.sample_rate
    }

    /// The analog level a channel sends to one side, between -0.25 and 0.25.
    ///
    /// Keep `output_inputs` in sync: it packs every value this and
    /// `agb_channel_output` read, so the M-cycles in which none of them
    /// changed can skip mixing. A value read here but not packed there would
    /// leave the audio stale (debug builds check for that).
    fn channel_output(&self, channel: Channel, connected: bool) -> f32 {
        if self.is_agb() {
            return self.agb_channel_output(channel, connected);
        }
        if connected && self.is_dac_enabled(channel) {
            (f32::from(self.state.samples[channel.index()]) / 7.5 - 1.0) / 4.0
        } else {
            0.0
        }
    }

    /// The AGB mixes the channels digitally, with a different bias per channel.
    fn agb_channel_output(&self, channel: Channel, connected: bool) -> f32 {
        let sample = self.state.samples[channel.index()];
        let (sample, silence) = if channel == Channel::Wave {
            (sample ^ 0xF, 14)
        } else {
            (sample, 0)
        };
        let bias = match channel {
            _ if !self.state.is_active(channel) => 0,
            Channel::Square1 => self.state.square1.envelope.volume,
            Channel::Square2 => self.state.square2.envelope.volume,
            Channel::Noise => self.state.noise.envelope.volume,
            Channel::Wave => 0,
        };
        let level = if connected {
            sample * 2 + bias
        } else {
            silence
        };
        (f32::from(level) / 15.0 - 1.0) / 4.0
    }

    /// Packs every value `channel_output` reads into one number, 32 bits per channel: its sample,
    /// its two NR51 bits, whether its DAC is on and (for the AGB) whether it plays and its volume.
    fn output_inputs(&self) -> u128 {
        let panning = self.registers[NR51];
        let volumes = [
            self.state.square1.envelope.volume,
            self.state.square2.envelope.volume,
            0,
            self.state.noise.envelope.volume,
        ];
        Channel::ALL.into_iter().fold(0, |inputs, channel| {
            let index = channel.index();
            let channel_inputs = u32::from(self.state.samples[index])
                | u32::from((panning >> index) & 0x11) << 8
                | u32::from(self.is_dac_enabled(channel)) << 16
                | u32::from(self.state.is_active(channel)) << 17
                | u32::from(volumes[index]) << 24;
            inputs | u128::from(channel_inputs) << (32 * index)
        })
    }

    /// Adds each change in a channel's output since the last call to the BLEP
    /// buffers. NR51 decides which channels go to which side.
    fn process_channel_deltas(&mut self, phase: usize) {
        let inputs = self.output_inputs();
        let previous_inputs = self.previous_output_inputs.replace(inputs);
        if previous_inputs == Some(inputs) {
            debug_assert!(
                self.outputs_are_mixed(),
                "a channel output changed without a change in output_inputs"
            );
            return;
        }
        let changed_inputs = previous_inputs.map_or(u128::MAX, |previous| previous ^ inputs);
        let panning = self.registers[NR51];
        let table = &*BLEP_TABLE;
        for channel in Channel::ALL {
            let index = channel.index();
            if (changed_inputs >> (32 * index)) & 0xFFFF_FFFF == 0 {
                continue;
            }
            let left = self.channel_output(channel, is_bit_set(panning, 4 + index as u8));
            let right = self.channel_output(channel, is_bit_set(panning, index as u8));

            let left_delta = left - self.previous_left_outputs[index];
            let right_delta = right - self.previous_right_outputs[index];
            if left_delta != 0.0 {
                self.blep_left.add_delta(left_delta, phase, table);
            }
            if right_delta != 0.0 {
                self.blep_right.add_delta(right_delta, phase, table);
            }
            self.previous_left_outputs[index] = left;
            self.previous_right_outputs[index] = right;
        }
    }

    /// Whether every channel's output on each side is the one last mixed into
    /// the BLEP buffers.
    fn outputs_are_mixed(&self) -> bool {
        let panning = self.registers[NR51];
        Channel::ALL.into_iter().all(|channel| {
            let index = channel.index();
            let left = self.channel_output(channel, is_bit_set(panning, 4 + index as u8));
            let right = self.channel_output(channel, is_bit_set(panning, index as u8));
            left.to_bits() == self.previous_left_outputs[index].to_bits()
                && right.to_bits() == self.previous_right_outputs[index].to_bits()
        })
    }

    /// Feeds this M-cycle's output into the BLEP buffers, at the M-cycle's
    /// position within the current output sample, and moves the output on by
    /// one M-cycle.
    fn enqueue_audio_samples(&mut self) {
        let phase = self.sample_clock as usize * BLEP_PHASES / T_CYCLES_PER_SECOND as usize;
        self.process_channel_deltas(phase);
        self.advance_sample_clock();
    }

    /// Moves the output on by one M-cycle and queues an output sample when the
    /// M-cycle completes one. An M-cycle is never longer than an output sample
    /// (see `MAX_SAMPLE_RATE`), so it completes at most one.
    fn advance_sample_clock(&mut self) {
        self.sample_clock += self.sample_clock_per_cycle();
        self.audio_buffers_full_in = None;
        if self.sample_clock >= T_CYCLES_PER_SECOND {
            self.sample_clock -= T_CYCLES_PER_SECOND;
            self.queue_output_samples(1);
        }
    }

    /// Queues `count` stereo output samples in a row: each reads the BLEP buffers, applies the
    /// NR50 master volume and passes through the high-pass filter.
    fn queue_output_samples(&mut self, count: u32) {
        let master_volume = self.registers[NR50];
        let left_master_volume = f32::from((master_volume & 0b0111_0000) >> 4) + 1.0;
        let right_master_volume = f32::from(master_volume & 0b111) + 1.0;
        let filter = HighpassFilter {
            rate: self.highpass_rate,
        };
        self.highpass_left_charge = filter.queue_samples(
            &mut self.left_sample_queue,
            &mut self.blep_left,
            self.highpass_left_charge,
            left_master_volume,
            count,
        );
        self.highpass_right_charge = filter.queue_samples(
            &mut self.right_sample_queue,
            &mut self.blep_right,
            self.highpass_right_charge,
            right_master_volume,
            count,
        );
    }
}

/// The high-pass filter the output samples go through, like the capacitor on
/// the real hardware's audio output, which removes the constant part of the
/// signal.
struct HighpassFilter {
    /// How much of its charge the capacitor keeps per output sample.
    rate: f32,
}

impl HighpassFilter {
    /// Queues `count` output samples of one side: each reads `blep`, scales it
    /// by the side's master volume (1 to 8) and filters it, with the
    /// capacitor holding `charge` at the start. Returns the charge at the end.
    fn queue_samples(
        &self,
        queue: &mut Vec<f32>,
        blep: &mut BlepBuffer,
        mut charge: f32,
        master_volume: f32,
        count: u32,
    ) -> f32 {
        queue.extend((0..count).map(|_| {
            let sample = blep.read() * master_volume / 8.0;
            charge = charge * self.rate + sample * (1.0 - self.rate);
            sample - charge
        }));
        charge
    }
}

/// Whether frame sequencer step `step` counts the envelopes down: every
/// eighth step, 64 times a second.
const fn clocks_envelopes(step: u8) -> bool {
    step & 7 == 7
}

/// Whether frame sequencer step `step` clocks the length counters: every
/// second step, 256 times a second.
const fn clocks_lengths(step: u8) -> bool {
    step & 1 == 1
}

/// Whether frame sequencer step `step` clocks channel 1's sweep: every fourth
/// step, 128 times a second.
const fn clocks_sweep(step: u8) -> bool {
    step & 3 == 3
}

/// How much of its charge the high-pass filter's capacitor keeps per output
/// sample at `sample_rate`.
fn highpass_rate(sample_rate: u32) -> f32 {
    HIGHPASS_CHARGE_KEPT_PER_T_CYCLE.powf(f64::from(T_CYCLES_PER_SECOND) / f64::from(sample_rate))
        as f32
}

/// The bit of the system counter (the internal counter behind DIV) whose
/// falling edge steps the frame sequencer: DIV bit 4, or DIV bit 5 in double
/// speed, where the counter runs twice as fast, so the frame sequencer still
/// steps 512 times per second.
fn div_apu_bit(cgb_double_speed: bool) -> u16 {
    if cgb_double_speed { 0x2000 } else { 0x1000 }
}

impl MemoryMapped for Apu {
    fn read_byte(&self, address: u16) -> u8 {
        match address {
            0xFF10..=0xFF3F => self.caught_up().read_register((address - 0xFF10) as usize),
            _ => {
                unreachable!("the address bus only routes APU addresses here, got 0x{address:04X}")
            }
        }
    }

    fn write_byte(&mut self, address: u16, value: u8) {
        match address {
            0xFF10..=0xFF3F => {
                self.prepare_for_change();
                self.write_register((address - 0xFF10) as usize, value);
            }
            _ => {
                unreachable!("the address bus only routes APU addresses here, got 0x{address:04X}")
            }
        }
    }
}

/// Save states hold the sound hardware and the audio output still being worked
/// out (the sample clock, the band-limited steps not yet heard, the high-pass
/// filter's charge and the channel levels last mixed), so a loaded state
/// sounds exactly like the run it was saved from. They leave out what the
/// model decides (`cgb_mode`), since states only load on the same model, and
/// what only schedules the catch-up. The sound hardware includes the address
/// on the CPU's bus that the wave channel's glitches read (`bus_address`).
/// Loading rejects every value a corrupted state could make the APU overflow
/// with (`ApuState::validate`, the sample clock and the levels).
///
/// The sample rate belongs to the frontend's sound card, not the console, so
/// it isn't saved: a state loaded into an emulator with another rate goes on
/// at that emulator's rate. The sample clock is saved as a fraction of a
/// sample and the levels and charge don't depend on the rate, so only the
/// band-limited steps of the last few output samples before saving play out
/// over as many samples at the new rate (each still adds up to its full size,
/// so nothing clicks).
impl Serializable for Apu {
    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        if self.pending_cycles > 0 {
            return self.caught_up().serialize(writer);
        }
        let mut fields = FieldWriter::new(writer);
        fields.field("registers", &self.registers)?;
        fields.field("state", &self.state)?;
        fields.field("cgb_double_speed", &self.cgb_double_speed)?;
        fields.field("system_counter", &self.system_counter)?;
        fields.field("late_div_events", &self.late_div_events)?;
        fields.field("pending_div_event", &self.pending_div_event)?;
        fields.field("bus_address", &self.bus_address)?;
        fields.field("sample_clock", &self.sample_clock)?;
        fields.field_with("previous_left_outputs", levels_encoding, |writer| {
            write_levels(&self.previous_left_outputs, writer)
        })?;
        fields.field_with("previous_right_outputs", levels_encoding, |writer| {
            write_levels(&self.previous_right_outputs, writer)
        })?;
        fields.field("blep_left", &self.blep_left)?;
        fields.field("blep_right", &self.blep_right)?;
        fields.field("highpass_left_charge", &self.highpass_left_charge)?;
        fields.field("highpass_right_charge", &self.highpass_right_charge)?;
        fields.finish()
    }

    fn deserialize(&mut self, reader: &mut dyn Read) -> io::Result<()> {
        self.prepare_for_change();
        read_fields(reader, |name, bytes| {
            match name {
                "registers" => self.registers.deserialize(bytes)?,
                "state" => self.state.deserialize(bytes)?,
                "cgb_double_speed" => self.cgb_double_speed.deserialize(bytes)?,
                "system_counter" => self.system_counter.deserialize(bytes)?,
                "late_div_events" => self.late_div_events.deserialize(bytes)?,
                "pending_div_event" => self.pending_div_event.deserialize(bytes)?,
                "bus_address" => self.bus_address.deserialize(bytes)?,
                "sample_clock" => self.sample_clock.deserialize(bytes)?,
                "previous_left_outputs" => read_levels(&mut self.previous_left_outputs, bytes)?,
                "previous_right_outputs" => read_levels(&mut self.previous_right_outputs, bytes)?,
                "blep_left" => self.blep_left.deserialize(bytes)?,
                "blep_right" => self.blep_right.deserialize(bytes)?,
                "highpass_left_charge" => self.highpass_left_charge.deserialize(bytes)?,
                "highpass_right_charge" => self.highpass_right_charge.deserialize(bytes)?,
                _ => return Ok(false),
            }
            Ok(true)
        })?;

        self.state.validate()?;
        let levels_plausible = self
            .previous_left_outputs
            .iter()
            .chain(&self.previous_right_outputs)
            .chain([&self.highpass_left_charge, &self.highpass_right_charge])
            .all(|level| level.abs() <= MAX_PLAUSIBLE_LEVEL);
        if self.sample_clock >= T_CYCLES_PER_SECOND || !levels_plausible {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "APU audio output out of range",
            ));
        }
        self.div_apu_bit = div_apu_bit(self.cgb_double_speed);
        self.update_held_back_events();
        self.audio_buffers_full_in = None;
        // Working every channel's output out again gives the levels saved
        // above for the channels that didn't change, so nothing is added.
        self.previous_output_inputs = None;
        Ok(())
    }
}

/// Writes each channel's last mixed level.
fn write_levels(levels: &[f32; CHANNEL_COUNT], writer: &mut dyn Write) -> io::Result<()> {
    levels.iter().try_for_each(|level| level.serialize(writer))
}

/// How `write_levels` stores the levels.
fn levels_encoding() -> String {
    array_encoding::<f32>(CHANNEL_COUNT)
}

/// Reads the levels `write_levels` wrote.
fn read_levels(levels: &mut [f32; CHANNEL_COUNT], reader: &mut dyn Read) -> io::Result<()> {
    levels
        .iter_mut()
        .try_for_each(|level| level.deserialize(reader))
}

#[cfg(test)]
mod tests;

mod blep;
mod catch_up;
mod envelope;
mod length;
mod noise;
mod square;
mod sweep;
mod wave;
