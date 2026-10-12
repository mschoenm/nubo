//! The browser session. Browsers give WebAssembly a single thread, so
//! instead of a background thread the emulator runs on each frame tick
//! until the audio queue is full again.

use super::{Command, Machine, Playback, ask};
use crate::notices::Notices;
use crate::rom::Rom;
use nubo_core::Model;

/// A running game. Dropping it writes any pending battery saves.
pub struct Session {
    machine: Machine,
    /// Counts finished frames, so the UI can tell when the screen changed.
    frame_sequence: u64,
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
        })
    }

    /// Carries out `command` right away.
    pub fn send(&mut self, command: Command) {
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

    /// Emulates until the audio queue is full, then returns the newest frame
    /// and its sequence number, or `None` if it is still the frame with the
    /// given sequence number.
    pub fn frame_since(&mut self, sequence: u64) -> Option<(u64, Vec<u8>)> {
        self.run_until_audio_queued();
        (self.frame_sequence != sequence).then(|| {
            (
                self.frame_sequence,
                self.machine.emulator.frame_buffer().to_vec(),
            )
        })
    }

    /// Emulates while the audio queue needs samples, and writes battery
    /// saves once they stop changing. To fast forward, it emulates several
    /// audio buffers for every one it plays, all on this tick: the desktop
    /// spreads them over the time the played one takes, but the browser only
    /// shows one frame per tick anyway.
    fn run_until_audio_queued(&mut self) {
        while self.machine.playback.running && self.machine.audio.needs_samples() {
            for _ in 1..self.machine.playback.buffers_per_played() {
                if self.machine.skip_buffer().is_some() {
                    self.frame_sequence += 1;
                }
            }
            if self.machine.play_buffer().1.is_some() {
                self.frame_sequence += 1;
            }
        }

        self.machine.saves.flush_when_settled();
    }
}
