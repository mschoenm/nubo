//! The browser session. Browsers give WebAssembly a single thread, so
//! instead of a background thread the emulator runs on each frame tick,
//! emulating the frames that are due by then.
//!
//! The browser's sound takes a whole buffer of samples from the queue at a
//! time (2,048 with cpal's WebAudio backend, about 43 ms), so filling the
//! queue whenever it runs low would emulate two or three frames on one tick
//! and none on the next ones, and only the last of each burst would be
//! shown. Instead the clock says how many frames are due, one at a time, and
//! the queue's length only corrects that count slowly, which keeps the
//! emulation in step with the sound without following its bursts. Both the
//! time between ticks and the queue's length are averaged first: ticks come
//! a little early or late, and the queue jumps by a whole buffer, and either
//! would otherwise move a frame to the tick before or after when it is due
//! close to the edge between two ticks.

use super::{Command, Machine, Playback, ask};
use crate::notices::Notices;
use crate::rom::Rom;
use iced::time::{Duration, Instant};
use nubo_core::Model;

/// How long the Game Boy takes to draw a frame: 70,224 clock ticks at
/// 4,194,304 Hz, about 59.73 frames a second.
const FRAME_DURATION: Duration = Duration::from_nanos(16_742_706);
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
/// The share of the difference between the queued samples and their target
/// made up each second: large enough to follow the sound card's clock
/// drifting from the browser's.
const QUEUE_CORRECTION_PER_SECOND: f64 = 0.5;
/// The share each new time between ticks has in their average.
const TICK_AVERAGING: f64 = 0.05;
/// How long the queue's length is averaged over, in seconds. Long enough to
/// smooth out its jumps by a whole sound buffer.
const QUEUE_AVERAGING_SECONDS: f64 = 0.5;
/// The longest time between two ticks that is emulated. Browsers stop the
/// ticks in hidden tabs, and the game should go on where it was rather than
/// race through the time it missed.
const LONGEST_TICK: Duration = Duration::from_millis(100);

/// A running game. Dropping it writes any pending battery saves.
pub struct Session {
    machine: Machine,
    /// Counts finished frames, so the UI can tell when the screen changed.
    frame_sequence: u64,
    /// Frames the clock says are due but haven't been emulated yet.
    frames_due: f64,
    /// When the last frames were made due, or `None` when the game just
    /// started or went on.
    last_tick: Option<Instant>,
    /// The average time between ticks in seconds, or `None` before the
    /// second tick.
    average_tick: Option<f64>,
    /// The average number of queued samples, or `None` before the first
    /// tick.
    average_queued: Option<f64>,
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
        Ok(Session {
            machine: Machine::start(rom, model, playback, notices)?,
            frame_sequence: 0,
            frames_due: 0.0,
            last_tick: None,
            average_tick: None,
            average_queued: None,
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

    /// Emulates the frames due by now, then returns the newest frame and its
    /// sequence number, or `None` if it is still the frame with the given
    /// sequence number.
    pub fn frame_since(&mut self, sequence: u64) -> Option<(u64, Vec<u8>)> {
        self.run_due_frames();
        (self.frame_sequence != sequence).then(|| {
            (
                self.frame_sequence,
                self.machine.emulator.frame_buffer().to_vec(),
            )
        })
    }

    /// Emulates the frames that have come due since the last tick, and
    /// writes battery saves once they stop changing. To fast forward, it
    /// emulates the speed's multiplier of frames for every one due, and only
    /// plays the sound of the last.
    fn run_due_frames(&mut self) {
        if self.machine.playback.running {
            let now = Instant::now();
            let elapsed = self.last_tick.map(|last| (now - last).min(LONGEST_TICK));
            self.last_tick = Some(now);
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

    /// Adds the frames due in the average time between ticks to the ones
    /// due, drawing the averaged audio queue toward its target. `elapsed` is
    /// the time since the last tick, or `None` on the first tick after the
    /// game started or went on, which adds none.
    fn make_frames_due(&mut self, elapsed: Option<Duration>) {
        let frame_samples =
            FRAME_DURATION.as_secs_f64() * f64::from(self.machine.audio.sample_rate());
        let buffer = self.machine.audio.buffer_samples() as f64;
        let target = TARGET_QUEUED_BUFFERS * buffer;
        let queued = self.machine.audio.queued_samples() as f64;

        if let Some(elapsed) = elapsed.map(|elapsed| elapsed.as_secs_f64()) {
            let tick = self.average_tick.map_or(elapsed, |average| {
                average + (elapsed - average) * TICK_AVERAGING
            });
            self.average_tick = Some(tick);

            let share = 1.0 - (-elapsed / QUEUE_AVERAGING_SECONDS).exp();
            let average_queued = self
                .average_queued
                .map_or(queued, |average| average + (queued - average) * share);
            self.average_queued = Some(average_queued);

            let missing_frames = (target - average_queued) / frame_samples;
            self.frames_due += tick
                * (1.0 / FRAME_DURATION.as_secs_f64()
                    + missing_frames * QUEUE_CORRECTION_PER_SECOND);
        }

        if queued < LOW_QUEUED_BUFFERS * buffer {
            self.frames_due = self.frames_due.max((target - queued) / frame_samples);
        }
        self.frames_due = self
            .frames_due
            .min((HIGH_QUEUED_BUFFERS * buffer - queued) / frame_samples);
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
            let (emulated_time, frame) = if play {
                self.machine.play_buffer()
            } else {
                self.machine.skip_buffer()
            };
            if frame.is_some() {
                self.frame_sequence += 1;
                self.screen_off_credit = 0.0;
                return;
            }
            self.screen_off_credit -= emulated_time.as_secs_f64();
            if self.screen_off_credit <= 0.0 {
                return;
            }
        }
    }
}
