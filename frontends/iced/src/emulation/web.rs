//! The browser session. Browsers give WebAssembly a single thread, so
//! instead of a background thread the emulator runs on each frame tick,
//! emulating the frames that are due by then.
//!
//! The browser's sound takes 2,048 samples from the queue at a time (cpal's
//! WebAudio buffer, about 43 ms), so filling the queue whenever it runs low
//! would emulate two or three frames on one tick and none on the next ones,
//! and only the last of each burst would be shown. Instead the clock says
//! how many frames are due, one at a time, and the queue's length only
//! corrects that count slowly, which keeps the emulation in step with the
//! sound without following its bursts.

use super::{Command, Machine, Playback, ask};
use crate::notices::Notices;
use crate::rom::Rom;
use iced::time::{Duration, Instant};
use nubo_core::Model;

/// How long the Game Boy takes to draw a frame: 70,224 clock ticks at
/// 4,194,304 Hz, about 59.73 frames a second.
const FRAME_DURATION: Duration = Duration::from_nanos(16_742_706);
/// How many stereo samples the audio queue holds on average. The browser
/// takes 2,048 at a time, and the queue must still hold that many just
/// before each take, though up to a frame of samples may come a tick late.
const TARGET_QUEUED_SAMPLES: f64 = 2560.0;
/// Below this many queued samples the sound is about to run out, so the
/// emulator catches up at once, such as when the game starts or goes on.
const LOW_QUEUED_SAMPLES: f64 = 512.0;
/// The emulator stops adding samples beyond this many, such as while the
/// browser hasn't allowed the sound to play yet.
const HIGH_QUEUED_SAMPLES: f64 = TARGET_QUEUED_SAMPLES + 4096.0;
/// The share of the difference between the queued samples and
/// `TARGET_QUEUED_SAMPLES` made up each second. Small enough that the
/// queue's 2,048-sample steps barely change how many frames are due, and
/// large enough to follow the sound card's clock drifting from the
/// browser's.
const QUEUE_CORRECTION_PER_SECOND: f64 = 0.5;
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
            let elapsed = self
                .last_tick
                .map_or(Duration::ZERO, |last| (now - last).min(LONGEST_TICK));
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

    /// Adds the frames that `elapsed` takes to the ones due, drawing the
    /// audio queue toward `TARGET_QUEUED_SAMPLES`.
    fn make_frames_due(&mut self, elapsed: Duration) {
        let frame_samples =
            FRAME_DURATION.as_secs_f64() * f64::from(self.machine.audio.sample_rate());
        let queued = self.machine.audio.queued_samples() as f64;
        let missing_frames = (TARGET_QUEUED_SAMPLES - queued) / frame_samples;

        self.frames_due += elapsed.as_secs_f64()
            * (1.0 / FRAME_DURATION.as_secs_f64() + missing_frames * QUEUE_CORRECTION_PER_SECOND);
        if queued < LOW_QUEUED_SAMPLES {
            self.frames_due = self.frames_due.max(missing_frames);
        }
        self.frames_due = self
            .frames_due
            .min((HIGH_QUEUED_SAMPLES - queued) / frame_samples);
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
