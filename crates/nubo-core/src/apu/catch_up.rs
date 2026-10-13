//! Running the channels lazily, a batch of M-cycles at a time.
//!
//! The address bus steps the APU once per M-cycle, but running the channels
//! and mixing their output every M-cycle is slow, and most M-cycles change
//! nothing a game can hear or read. So `Apu::step` only counts the M-cycle as
//! pending, and `Apu::catch_up` runs all pending M-cycles at once, like
//! SameBoy's `GB_apu_run`. It is called:
//!
//! - in the M-cycle in which the audio buffers fill up, and after that in the
//!   M-cycle of each audio sample, so the emulator stops and hands out the
//!   buffers at the same moment as before,
//! - before anything outside the channels' own timing reads or changes their
//!   state: register accesses, frame sequencer steps that change what the
//!   channels play, speed switches,
//! - on every M-cycle in the states where running two stretches of time in
//!   one go would give a different result than running them one after the
//!   other: while channel 1's sweep calculates, while a DMG noise trigger is
//!   delayed, and while the wave channel reads the CPU's bus (a stopped wave
//!   channel only reads it in the M-cycle in which its timer runs out, so
//!   the M-cycles before that one wait for it).
//!
//! The result is exactly the same as running every M-cycle on its own, down to
//! the bits of every audio sample: a batch is cut at each M-cycle in which a
//! channel's output can change, and that change is mixed into the BLEP buffers
//! at the M-cycle's own position within the output sample. Each channel's next
//! change stays known between cuts, so a cut only works out the next change of
//! the channel that just changed.
//!
//! The APU unit tests check this: they play fixed and random register writes
//! on every model lazily, in bulk (`advance_cycles`) and one M-cycle at a time,
//! and compare the audio and the end state (and, for the lazy run, the
//! PCM12/PCM34 reads). Debug builds also check that a frame sequencer step
//! judged quiet changed nothing the channels read, that cached predictions
//! match freshly worked out ones, and that the mixer's test for skipping a
//! channel covers everything the channel's output is worked out from.

use crate::apu::{
    Apu, CHANNEL_COUNT, Channel, MAX_AUDIO_BUFFER_SIZE, NR51, SYSTEM_COUNTER_TICKS_PER_M_CYCLE,
    Square, T_CYCLES_PER_SECOND,
};
use std::borrow::Cow;
use std::fmt;

/// The most M-cycles one run covers, so its ticks fit in 16 bits.
const MAX_CYCLES_PER_RUN: u32 = 0x7FFF;
/// The value of `Apu::next_output_changes` for a channel whose output
/// can't change.
pub(super) const NO_OUTPUT_CHANGE: u32 = u32::MAX;

impl Apu {
    /// Runs every pending M-cycle and mixes the output changes they make, in
    /// the same order and at the same moments as running them one by one.
    pub(crate) fn catch_up(&mut self) {
        let mut cycles_left = std::mem::take(&mut self.pending_cycles);
        if cycles_left == 0 {
            return;
        }

        // The PCM12/PCM34 read glitches only come from the last tick of the
        // current M-cycle's run. Earlier M-cycles' glitches were cleared when
        // the next M-cycle began.
        let pcm_mask_at_start = self.pcm_mask;
        let ticks_per_cycle = self.ticks_per_cycle();
        let mut cycles_until_change = if cycles_left == 1 || self.outputs_changed_since_mixed() {
            1
        } else {
            self.cycles_until_output_change()
        };
        loop {
            let cycles = cycles_until_change.min(cycles_left).min(MAX_CYCLES_PER_RUN);
            self.pcm_mask = pcm_mask_at_start;
            let ticks = u16::try_from(cycles).expect("runs are at most 0x7FFF M-cycles long")
                * ticks_per_cycle;
            self.run(ticks);
            self.count_down_output_changes(u32::from(ticks));
            self.advance_steady_output(cycles - 1);
            self.enqueue_audio_samples();
            cycles_left -= cycles;
            if cycles_left == 0 {
                break;
            }
            cycles_until_change = self.cycles_until_output_change();
        }
        if !self.stepped_this_cycle {
            self.pcm_mask = pcm_mask_at_start;
        }
        self.schedule_catch_up();
    }

    /// Moves the output on by `cycles` M-cycles in which no channel output
    /// changes, queueing the audio samples that fall due in them.
    fn advance_steady_output(&mut self, cycles: u32) {
        if cycles == 0 {
            return;
        }
        self.audio_buffers_full_in = None;
        let clock = u64::from(self.sample_clock)
            + u64::from(cycles) * u64::from(self.sample_clock_per_cycle());
        let whole_sample = u64::from(T_CYCLES_PER_SECOND);
        self.sample_clock =
            u32::try_from(clock % whole_sample).expect("less than one output sample");
        let samples = clock / whole_sample;
        if samples > 0 {
            self.queue_output_samples(
                u32::try_from(samples).expect("at most one sample per M-cycle"),
            );
        }
    }

    /// Catches up before something outside the channels' own timing reads or
    /// changes their state, and makes the next M-cycle work out the deadline
    /// again, so the new state decides how long the M-cycles after it may wait.
    pub(super) fn prepare_for_change(&mut self) {
        self.catch_up();
        self.catch_up_deadline = 1;
        self.schedule_outdated = true;
        self.next_output_changes = [None; CHANNEL_COUNT];
    }

    /// Runs the pending M-cycles now that `catch_up_deadline` has come, unless
    /// the deadline only came because it was outdated and turns out to be
    /// later once worked out again. Running the M-cycles that follow a change
    /// later gives the same result, as long as the changed state doesn't have
    /// to run every M-cycle, which `schedule_catch_up` checks.
    pub(super) fn catch_up_when_due(&mut self) {
        if self.schedule_outdated {
            self.schedule_catch_up();
            if self.pending_cycles < self.catch_up_deadline {
                return;
            }
        }
        self.catch_up();
    }

    /// The APU with its pending M-cycles run, for reads through a shared
    /// reference. The address bus catches the APU up before the CPU reads it,
    /// so this only copies the APU for other reads (save states, tests), and
    /// then without the queued samples, which those reads don't see.
    pub(super) fn caught_up(&self) -> Cow<'_, Apu> {
        if self.pending_cycles == 0 {
            Cow::Borrowed(self)
        } else {
            let mut apu = self.copy_with_samples(Vec::new(), Vec::new());
            apu.catch_up();
            Cow::Owned(apu)
        }
    }

    /// A copy of the APU with `left_samples` and `right_samples` queued instead
    /// of its own samples. Every field is listed, so a new one can't be
    /// forgotten.
    fn copy_with_samples(&self, left_samples: Vec<f32>, right_samples: Vec<f32>) -> Apu {
        Apu {
            registers: self.registers,
            state: self.state,
            model: self.model,
            cgb_mode: self.cgb_mode,
            cgb_double_speed: self.cgb_double_speed,
            pcm_mask: self.pcm_mask,
            system_counter: self.system_counter,
            bus_address: self.bus_address,
            sample_clock: self.sample_clock,
            late_div_events: self.late_div_events,
            pending_div_event: self.pending_div_event,
            held_back_events: self.held_back_events,
            div_apu_bit: self.div_apu_bit,
            left_sample_queue: left_samples,
            right_sample_queue: right_samples,
            previous_left_outputs: self.previous_left_outputs,
            previous_right_outputs: self.previous_right_outputs,
            previous_output_inputs: self.previous_output_inputs,
            next_output_changes: self.next_output_changes,
            blep_left: self.blep_left.clone(),
            blep_right: self.blep_right.clone(),
            highpass_left_charge: self.highpass_left_charge,
            highpass_right_charge: self.highpass_right_charge,
            highpass_rate: self.highpass_rate,
            sample_rate: self.sample_rate,
            pending_cycles: self.pending_cycles,
            // Worked out from the queues, which may differ.
            audio_buffers_full_in: None,
            catch_up_deadline: self.catch_up_deadline,
            schedule_outdated: self.schedule_outdated,
            stepped_this_cycle: self.stepped_this_cycle,
        }
    }

    /// Whether an access to `address` reads state that pending M-cycles can
    /// change: the sound registers, wave RAM, PCM12 and PCM34.
    #[inline]
    pub(crate) fn reads_channel_state(address: u16) -> bool {
        // Nearly every access is below 0xFF10, so that is checked on its own.
        if address < 0xFF10 {
            return false;
        }
        matches!(address, 0xFF10..=0xFF3F | 0xFF76 | 0xFF77)
    }

    /// Works out how many M-cycles may be pending before `step` runs them,
    /// counted from the last catch-up. Audio samples are only queued when the
    /// M-cycles they fall in run, but nothing can see them before the buffers
    /// are full: until then, the M-cycles wait for the one that fills the
    /// buffers, and after it, for each next sample.
    fn schedule_catch_up(&mut self) {
        self.schedule_outdated = false;
        let audio_deadline = match self.known_cycles_until_audio_buffers_full() {
            0 => self.cycles_until_output_sample(),
            cycles => cycles,
        };
        self.catch_up_deadline = match self.cycles_until_timing_dependent_run() {
            Some(cycles) => {
                // These states change the outputs in ways the predictions don't foresee.
                self.next_output_changes = [None; CHANNEL_COUNT];
                audio_deadline.min(cycles)
            }
            None => audio_deadline,
        };
    }

    /// `exact_cycles_until_audio_buffers_full`, worked out again only after
    /// the audio output moved on or changed.
    fn known_cycles_until_audio_buffers_full(&mut self) -> u32 {
        let cycles = match self.audio_buffers_full_in {
            Some(cycles) => cycles,
            None => {
                let cycles = self.exact_cycles_until_audio_buffers_full();
                self.audio_buffers_full_in = Some(cycles);
                cycles
            }
        };
        debug_assert_eq!(cycles, self.exact_cycles_until_audio_buffers_full());
        cycles
    }

    /// The number of M-cycles until `audio_buffers_full` becomes true, counted
    /// from before the pending M-cycles: up to and including the one in which
    /// the sample clock completes the last missing sample.
    fn exact_cycles_until_audio_buffers_full(&self) -> u32 {
        let queued = self
            .left_sample_queue
            .len()
            .min(self.right_sample_queue.len());
        let missing_samples = MAX_AUDIO_BUFFER_SIZE.saturating_sub(queued) as u64;
        let clock_needed = (missing_samples * u64::from(T_CYCLES_PER_SECOND))
            .saturating_sub(u64::from(self.sample_clock));
        u32::try_from(clock_needed.div_ceil(u64::from(self.sample_clock_per_cycle())))
            .expect("a buffer lasts less than 2^32 M-cycles at every accepted sample rate")
    }

    /// Whether the channels are in a state where running several M-cycles in
    /// one go would differ from running them one at a time, and if so, the
    /// number of M-cycles up to and including the next one whose run depends
    /// on when it runs. The states are: channel 1's sweep is calculating (it
    /// runs before the squares within a run), a DMG noise trigger is delayed,
    /// or the wave channel reads wave RAM at the address the CPU's bus holds
    /// in that M-cycle. All of them depend on the next M-cycle, except a
    /// stopped wave channel's timer, which only reads the bus in the M-cycle
    /// in which it runs out, so the M-cycles before it may wait for that one.
    fn cycles_until_timing_dependent_run(&self) -> Option<u32> {
        let state = &self.state;
        if self.sweep_calculating()
            || state.noise.dmg_delayed_start != 0
            || state.wave.bugged_read_countdown != 0
        {
            return Some(1);
        }
        self.stopped_wave_timer_runs().then(|| {
            (u32::from(state.wave.sample_countdown) + 1).div_ceil(u32::from(self.ticks_per_cycle()))
        })
    }

    /// The number of M-cycles up to and including the one in which the next
    /// audio sample is queued.
    fn cycles_until_output_sample(&self) -> u32 {
        (T_CYCLES_PER_SECOND - self.sample_clock).div_ceil(self.sample_clock_per_cycle())
    }

    /// Whether anything the channel outputs are worked out from changed since
    /// they were last mixed into the BLEP buffers.
    fn outputs_changed_since_mixed(&self) -> bool {
        self.previous_output_inputs != Some(self.output_inputs())
    }

    /// The number of M-cycles up to and including the first one in which a
    /// run changes a channel's output, which only happens when a square or
    /// wave channel moves to its next step or the noise channel's LFSR steps
    /// (outside the states `cycles_until_timing_dependent_run` covers). It may also be an
    /// earlier M-cycle: cutting a batch where nothing changes costs time, but
    /// changes nothing.
    ///
    /// Each channel's next change stays known until a run reaches it or
    /// something outside the channels' own timing changes them, so after a cut
    /// usually only the channel that just changed is worked out again.
    fn cycles_until_output_change(&mut self) -> u32 {
        if cfg!(debug_assertions) {
            self.check_known_output_changes();
        }
        let mut ticks_until_change = NO_OUTPUT_CHANGE;
        // By reference on purpose: iterating `Channel::ALL` by value lets the
        // compiler unroll the loop and work out every channel's prediction up
        // front, known or not, which costs 0.44% more instructions overall.
        for (index, &channel) in Channel::ALL.iter().enumerate() {
            let ticks = match self.next_output_changes[index] {
                Some(ticks) => ticks,
                None => {
                    let ticks = self
                        .ticks_until_channel_output_change(channel)
                        .unwrap_or(NO_OUTPUT_CHANGE);
                    self.next_output_changes[index] = Some(ticks);
                    ticks
                }
            };
            ticks_until_change = ticks_until_change.min(ticks);
        }
        match ticks_until_change {
            NO_OUTPUT_CHANGE => u32::MAX,
            ticks => ticks.div_ceil(u32::from(self.ticks_per_cycle())),
        }
    }

    /// Checks that no known output change, counted down since it was worked
    /// out, comes later than working it out again now says: a prediction that
    /// is too late would let a batch run past a change and mix it at the
    /// wrong moment. Debug builds check this on every batch.
    fn check_known_output_changes(&self) {
        for (&next, &channel) in self.next_output_changes.iter().zip(&Channel::ALL) {
            if let Some(known) = next {
                let fresh = self
                    .ticks_until_channel_output_change(channel)
                    .unwrap_or(NO_OUTPUT_CHANGE);
                assert!(
                    known <= fresh,
                    "{channel:?}'s next output change was predicted {known} ticks away, but is {fresh}"
                );
            }
        }
    }

    /// The number of 2 MHz ticks up to and including the step that next
    /// changes `channel`'s output (or an earlier one), as long as
    /// nothing outside the channels' own timing changes them first. `None`
    /// when no step does, which includes channels NR51 sends to neither side:
    /// their output changes nothing that is heard.
    fn ticks_until_channel_output_change(&self, channel: Channel) -> Option<u32> {
        if self.registers[NR51] & (0x11 << channel.index()) == 0 {
            return None;
        }
        match channel {
            Channel::Square1 => self.ticks_until_square_output_change(Square::Channel1),
            Channel::Square2 => self.ticks_until_square_output_change(Square::Channel2),
            Channel::Wave => self.ticks_until_wave_output_change(),
            Channel::Noise => self.ticks_until_noise_output_change(),
        }
    }

    /// Moves the known output changes `ticks` 2 MHz ticks closer, after a
    /// run. Those the run reached are forgotten, since the channel may have
    /// changed there.
    fn count_down_output_changes(&mut self, ticks: u32) {
        for next in &mut self.next_output_changes {
            match *next {
                Some(NO_OUTPUT_CHANGE) | None => {}
                Some(until_change) if until_change <= ticks => *next = None,
                Some(until_change) => *next = Some(until_change - ticks),
            }
        }
    }
}

impl Clone for Apu {
    fn clone(&self) -> Self {
        let mut copy = self.copy_with_samples(
            self.left_sample_queue.clone(),
            self.right_sample_queue.clone(),
        );
        copy.audio_buffers_full_in = self.audio_buffers_full_in;
        copy
    }
}

/// Prints the APU as it is once its pending M-cycles have run, without the
/// catch-up's scheduling (the pending M-cycles, deadlines and cached
/// predictions). `--verify-boot` and `clone_with_rom_matches_a_real_boot`
/// compare the emulator's `Debug` output, and two APUs that are the same
/// except for when they last caught up must compare equal.
impl fmt::Debug for Apu {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut caught_up = self.clone();
        caught_up.catch_up();
        let Apu {
            registers,
            state,
            model,
            cgb_mode,
            cgb_double_speed,
            pcm_mask,
            system_counter,
            bus_address,
            sample_clock,
            late_div_events,
            pending_div_event,
            held_back_events,
            div_apu_bit,
            left_sample_queue,
            right_sample_queue,
            previous_left_outputs,
            previous_right_outputs,
            blep_left,
            blep_right,
            highpass_left_charge,
            highpass_right_charge,
            highpass_rate,
            sample_rate,
            previous_output_inputs: _,
            next_output_changes: _,
            pending_cycles: _,
            audio_buffers_full_in: _,
            catch_up_deadline: _,
            schedule_outdated: _,
            stepped_this_cycle: _,
        } = &caught_up;
        formatter
            .debug_struct("Apu")
            .field("registers", registers)
            .field("state", state)
            .field("model", model)
            .field("cgb_mode", cgb_mode)
            .field("cgb_double_speed", cgb_double_speed)
            .field("pcm_mask", pcm_mask)
            .field("system_counter", system_counter)
            .field("bus_address", bus_address)
            .field("sample_clock", sample_clock)
            .field("late_div_events", late_div_events)
            .field("pending_div_event", pending_div_event)
            .field("held_back_events", held_back_events)
            .field("div_apu_bit", div_apu_bit)
            .field("left_sample_queue", left_sample_queue)
            .field("right_sample_queue", right_sample_queue)
            .field("previous_left_outputs", previous_left_outputs)
            .field("previous_right_outputs", previous_right_outputs)
            .field("blep_left", blep_left)
            .field("blep_right", blep_right)
            .field("highpass_left_charge", highpass_left_charge)
            .field("highpass_right_charge", highpass_right_charge)
            .field("highpass_rate", highpass_rate)
            .field("sample_rate", sample_rate)
            .finish_non_exhaustive()
    }
}

/// Moving through stretches of M-cycles without DIV-APU edges in one call, for
/// when nothing else needs the M-cycles one at a time either, such as while the
/// CPU is halted.
impl Apu {
    /// Moves the APU on by `cycles` M-cycles in one call, the same as calling
    /// `begin_machine_cycle` and `step` that many times. Only allowed while
    /// `can_advance_in_bulk` holds, with the CPU speed fixed and no DIV-APU
    /// edge (see `cycles_until_div_apu_edge`) within the M-cycles. Call it
    /// before `system_counter_changed` reports where the M-cycles leave the
    /// system counter, as the address bus does.
    pub(crate) fn advance_cycles(&mut self, cycles: u32) {
        debug_assert!(self.can_advance_in_bulk());
        debug_assert!(
            cycles < self.cycles_until_div_apu_edge(),
            "a DIV-APU edge falls within the {cycles} M-cycles"
        );
        if cycles == 0 {
            return;
        }
        self.pcm_mask = [0xFF; 2];
        self.stepped_this_cycle = true;
        self.pending_cycles += cycles;
        if self.pending_cycles >= self.catch_up_deadline {
            self.catch_up_when_due();
        }
    }

    /// Whether `advance_cycles` may be used now: no frame sequencer event is
    /// held back to the start of the next M-cycle, and the channels aren't in
    /// a state that has to run one M-cycle at a time.
    pub(crate) fn can_advance_in_bulk(&self) -> bool {
        !self.held_back_events && self.cycles_until_timing_dependent_run().is_none()
    }

    /// The number of M-cycles up to and including the one in which the
    /// DIV-APU bit of the system counter next rises or falls, counted from the
    /// last value `system_counter_changed` saw.
    pub(crate) fn cycles_until_div_apu_edge(&self) -> u32 {
        let bit = u32::from(self.div_apu_bit);
        let counter = u32::from(self.system_counter);
        (bit - (counter & (bit - 1))).div_ceil(u32::from(SYSTEM_COUNTER_TICKS_PER_M_CYCLE))
    }

    /// The number of further `step`s up to and including the one that runs the
    /// pending M-cycles. The audio buffers only fill when they run, and the
    /// deadline is never later than the M-cycle that fills them, so they
    /// can't fill before then.
    pub(crate) fn cycles_until_catch_up(&self) -> u32 {
        self.catch_up_deadline.saturating_sub(self.pending_cycles)
    }

    /// The number of M-cycles until `audio_buffers_full` becomes true.
    ///
    /// When the samples fall due doesn't depend on what the channels play, so
    /// the pending M-cycles don't have to run first: they count as passed.
    #[cfg(test)]
    pub(super) fn cycles_until_audio_buffers_full(&mut self) -> u32 {
        match self.known_cycles_until_audio_buffers_full() {
            0 => 0,
            cycles if cycles > self.pending_cycles => cycles - self.pending_cycles,
            _ => {
                self.catch_up();
                self.exact_cycles_until_audio_buffers_full()
            }
        }
    }
}
