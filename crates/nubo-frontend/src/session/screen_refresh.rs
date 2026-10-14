//! The session paced by the screen's refreshes, which the browser uses.
//! Browsers give WebAssembly a single thread, so instead of a background
//! thread the emulator runs on each frame tick, when the app calls
//! [`Session::run_due_frames`], emulating the frames that are due by then.
//!
//! The browser's sound takes a whole buffer of samples from the queue at a
//! time (2,048 with cpal's WebAudio backend, about 43 ms), so filling the
//! queue whenever it runs low would emulate two or three frames on one tick
//! and none on the next ones, and only the last of each burst would be
//! shown. Instead the screen's refreshes, or the clock on screens frames
//! can't be locked to, say how many frames are due, one at a time, and the
//! queue's length only changes how many samples each emulated second makes
//! (see `session/pacing.rs`).

use super::pacing::{FRAME_DURATION, Lock, RefreshRate, SampleRateControl};
use super::{Command, Machine, Playback, ask};
use crate::notices::Notices;
use crate::rom::Rom;
use nubo_core::Model;
use refresh_clock::RefreshClock;

/// How many sound buffers the audio queue holds on average. The sound takes
/// a whole buffer at a time, and the queue must still hold one just before
/// each take, though up to a frame of samples may come a tick late.
const TARGET_QUEUED_BUFFERS: f64 = 1.25;
/// Below this many sound buffers in the queue the sound is about to run
/// out, so the emulator catches up at once, such as when the game starts or
/// goes on.
const LOW_QUEUED_BUFFERS: f64 = 0.25;
/// The emulator stops adding samples beyond this many sound buffers, such as
/// while the browser hasn't allowed the sound to play yet.
const HIGH_QUEUED_BUFFERS: f64 = TARGET_QUEUED_BUFFERS + 2.0;
/// The share each new time between ticks has in their average, which makes
/// frames due on screens frames aren't locked to.
const TICK_AVERAGING: f64 = 0.05;
/// The longest time between two ticks that is emulated, in seconds. Browsers
/// stop the ticks in hidden tabs, and the game should go on where it was
/// rather than race through the time it missed.
const LONGEST_TICK: f64 = 0.1;

/// A running game. Dropping it writes any pending battery saves.
pub struct Session {
    machine: Machine,
    /// Counts finished frames, so the UI can tell when the screen changed.
    frame_sequence: u64,
    /// Frames the clock says are due but haven't been emulated yet.
    frames_due: f64,
    /// When the screen refreshed for the last frames made due, in seconds,
    /// or `None` when the game just started or went on.
    last_tick: Option<f64>,
    /// When the screen refreshes.
    refresh_clock: RefreshClock,
    /// The average time between ticks in seconds, or `None` before the
    /// second tick.
    average_tick: Option<f64>,
    /// The screen's refresh rate, to lock frames to.
    refresh_rate: RefreshRate,
    /// Sets how many samples each emulated second makes.
    sample_rate_control: SampleRateControl,
    /// The sample rate the emulator makes its sound at.
    emulator_sample_rate: u32,
    /// Seconds of emulated time still owed to the frames run while the
    /// screen is off, which finish no frame to stop at, or negative when they
    /// ran ahead (see [`Session::run_frame`]).
    screen_off_credit: f64,
}

impl Session {
    /// Creates the emulator and loads `rom` on `model`, playing as
    /// `playback` says from the start, returning the load error if it
    /// couldn't. Problems while the game runs go to `notices`.
    pub fn start(
        rom: &Rom,
        model: Model,
        playback: Playback,
        notices: &Notices,
    ) -> Result<Session, String> {
        let machine = Machine::start(rom, model, playback, notices)?;
        Ok(Session {
            emulator_sample_rate: machine.audio.sample_rate(),
            machine,
            frame_sequence: 0,
            frames_due: 0.0,
            last_tick: None,
            refresh_clock: RefreshClock::start(),
            average_tick: None,
            refresh_rate: RefreshRate::default(),
            sample_rate_control: SampleRateControl::default(),
            screen_off_credit: 0.0,
        })
    }

    /// Carries out `command` right away.
    pub fn send(&mut self, command: Command) {
        if matches!(command, Command::SetPlayback(_)) {
            self.last_tick = None;
        }
        self.machine.handle(command);
    }

    /// Captures the game's complete state.
    pub fn save_state(&mut self) -> Result<Vec<u8>, String> {
        ask(|command| self.send(command), Command::SaveState)
    }

    /// Restores a state captured with [`Session::save_state`].
    pub fn load_state(&mut self, state: Vec<u8>) -> Result<(), String> {
        ask(
            |command| self.send(command),
            |reply| Command::LoadState(state, reply),
        )?
    }

    /// Returns the newest frame and its sequence number, or `None` if it is
    /// still the frame with the given sequence number.
    pub fn frame_since(&mut self, sequence: u64) -> Option<(u64, Vec<u8>)> {
        (self.frame_sequence != sequence).then(|| {
            (
                self.frame_sequence,
                self.machine.emulator.frame_buffer().to_vec(),
            )
        })
    }

    /// Emulates the frames that have come due by the screen's latest
    /// refresh, and writes battery saves once they stop changing. The app
    /// calls it on each frame tick. When the screen refreshed comes from the
    /// session's own clock (see `refresh_clock.rs`), not from the app,
    /// whose ticks come too unevenly. To fast forward, it emulates the
    /// speed's multiplier of frames for every one due, and only plays the
    /// sound of the last.
    pub fn run_due_frames(&mut self) {
        if self.machine.playback.running {
            let at = self.refresh_clock.last_refresh();
            let elapsed = self
                .last_tick
                .zip(at)
                .map(|(last, at)| (at - last).clamp(0.0, LONGEST_TICK));
            self.last_tick = at.or(self.last_tick);
            self.make_frames_due(elapsed);

            while self.frames_due >= 1.0 {
                self.frames_due -= 1.0;
                for _ in 1..self.machine.playback.buffers_per_played() {
                    self.run_frame(false);
                }
                self.run_frame(true);
            }
        }

        self.machine.saves.flush_when_settled();
    }

    /// Adds the frames due since the last tick to the ones due: one for
    /// each refresh or every few refreshes when frames are locked to them,
    /// otherwise as many as fit in the average time between ticks. Then sets
    /// the emulator's sample rate so the sound keeps up with the frames and
    /// the audio queue stays at its target. `elapsed` is the time since the
    /// last tick in seconds, or `None` on the first tick after the game
    /// started or went on, which adds none.
    fn make_frames_due(&mut self, elapsed: Option<f64>) {
        let sample_rate = f64::from(self.machine.audio.sample_rate());
        let frame_samples = FRAME_DURATION.as_secs_f64() * sample_rate;
        let buffer = self.machine.audio.buffer_samples() as f64;
        let target = TARGET_QUEUED_BUFFERS * buffer;
        let queued = self.machine.audio.queued_samples() as f64;

        if let Some(elapsed) = elapsed.filter(|&elapsed| elapsed > 0.0) {
            let tick = self.average_tick.map_or(elapsed, |average| {
                average + (elapsed - average) * TICK_AVERAGING
            });
            self.average_tick = Some(tick);
            self.refresh_rate.observe(elapsed);

            let lock = self.refresh_rate.lock();
            self.frames_due += lock.map_or(tick / FRAME_DURATION.as_secs_f64(), |lock| {
                lock.frames_in(elapsed)
            });

            let adjustment = self
                .sample_rate_control
                .update(elapsed, queued, target, sample_rate);
            let speed = lock.map_or(1.0, Lock::speed);
            self.set_emulator_sample_rate(sample_rate * (1.0 + adjustment) / speed);
        }

        if queued < LOW_QUEUED_BUFFERS * buffer {
            self.frames_due = self.frames_due.max((target - queued) / frame_samples);
        }
        self.frames_due = self
            .frames_due
            .min((HIGH_QUEUED_BUFFERS * buffer - queued) / frame_samples);
    }

    /// Has the emulator make its sound at `sample_rate`, rounded to whole
    /// samples a second (steps of 0.002% at 48 kHz).
    fn set_emulator_sample_rate(&mut self, sample_rate: f64) {
        let sample_rate = sample_rate.round() as u32;
        if sample_rate != self.emulator_sample_rate {
            self.emulator_sample_rate = sample_rate;
            self.machine.emulator.set_sample_rate(sample_rate);
        }
    }

    /// Emulates audio buffers until the Game Boy finishes its next frame,
    /// playing their sound if `play` is set. A frame lasts longer than a
    /// buffer at the browser's sample rates, so this finishes exactly one.
    /// While the screen is off no frame finishes, and it stops once it has
    /// emulated a frame's time instead, carrying what it ran over in
    /// `screen_off_credit` so those frames keep their exact length on
    /// average.
    fn run_frame(&mut self, play: bool) {
        self.screen_off_credit += FRAME_DURATION.as_secs_f64();
        loop {
            let (playing_time, frame) = if play {
                self.machine.play_buffer()
            } else {
                self.machine.skip_buffer()
            };
            if frame.is_some() {
                self.frame_sequence += 1;
                self.screen_off_credit = 0.0;
                return;
            }
            // The buffer holds samples at the emulator's rate, not the sound
            // card's, which its playing time was worked out with.
            self.screen_off_credit -= playing_time.as_secs_f64()
                * f64::from(self.machine.audio.sample_rate())
                / f64::from(self.emulator_sample_rate);
            if self.screen_off_credit <= 0.0 {
                return;
            }
        }
    }
}

mod refresh_clock;
