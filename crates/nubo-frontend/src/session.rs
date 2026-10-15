//! Running a game: the emulator with its sound and battery saves, played
//! at the Game Boy's own speed, and what an app shows and keeps of it.
//!
//! - [`GameSession`] is what an app holds while a game runs: the game's
//!   playback (pausing, fast forward, volume), its cheats, the palette, the
//!   save state slots and the picture on screen.
//! - `Session` runs the emulator underneath. It takes [`Command`]s, which
//!   `Machine::handle` carries out, and comes in two kinds that pace the
//!   game differently:
//!   - **by the sound card** on the desktop (`session/sound_card.rs`): the
//!     emulator runs on a thread of its own, which sleeps until the sound
//!     card's queue runs low or a command comes, and tells the app about
//!     each new frame through a callback (`FrameListener`);
//!   - **by the screen's refreshes** in the browser, which gives
//!     WebAssembly a single thread (`session/screen_refresh.rs`): the app
//!     calls `Session::run_due_frames` on each refresh, and
//!     `session/pacing.rs` decides how many frames are due.
//!
//! The sound goes out through cpal (`session/audio.rs`), so this module
//! needs the `desktop` feature (the desktop's sound) or the `web` feature
//! (the browser's).

use crate::battery::{BatteryCartridgeEffects, BatterySaves};
use crate::cheats::Cheat;
use crate::notices::Notices;
use crate::picture::Frame;
use crate::rom::Rom;
use crate::settings::FastForwardSpeed;
use crate::sgb_menu::BorderClock;
use crate::time::Duration;
use audio::AudioOutput;
use history::History;
use nubo_core::{
    CartridgeEffects, ControllerMapping, ControllerState, Emulator, Key, Model, NoSaves, RtcClock,
    SgbBorder,
};
use pacing::FRAME_DURATION;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};

pub use game::{GameSession, StartOptions};

/// How many times faster than play rewinding goes back.
const REWIND_SPEED: f64 = 2.0;
/// How many samples of silence a buffer holds while rewinding.
const REWIND_BUFFER_SAMPLES: usize = 512;
#[cfg(target_arch = "wasm32")]
pub use screen_refresh::Session;
#[cfg(not(target_arch = "wasm32"))]
pub use sound_card::{ActivityCount, FrameListener, Session};

/// Whether the game runs, how fast and how loud.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Playback {
    /// Whether the game runs, rather than being paused.
    pub running: bool,
    /// The fast forward speed, or `None` for real time.
    pub fast_forward: Option<FastForwardSpeed>,
    /// How loud to play, from 0 (silent) to 1 (as loud as the game).
    pub volume: f32,
    /// Whether the game goes back through its recent past instead of
    /// forward (see `history`), silently.
    pub rewinding: bool,
}

impl Playback {
    /// How many audio buffers to emulate for each one played: one at normal
    /// speed, and the speed's multiplier while fast forwarding. The sound of
    /// the others is dropped, so it keeps its pitch.
    pub fn buffers_per_played(self) -> u32 {
        self.fast_forward.map_or(1, FastForwardSpeed::multiplier)
    }
}

/// Where a game keeps its battery saves.
#[derive(Clone)]
pub enum Saves {
    /// In these battery saves, with the cartridge clock on the wall clock.
    Kept(Arc<BatterySaves>),
    /// Nowhere: the game starts without save data and keeps none, and its
    /// cartridge clock follows emulated time, so every run is the same. For
    /// runs that must leave nothing behind, such as the GB Emulator
    /// Shootout's.
    Discarded,
}

impl Saves {
    /// The battery saves of the ROM at `rom_path` in this build's storage
    /// (see [`BatterySaves::for_rom`]), or none without `keep`.
    pub fn for_rom(rom_path: &Path, keep: bool, notices: &Notices) -> Saves {
        if keep {
            Saves::Kept(BatterySaves::for_rom(rom_path, notices.clone()))
        } else {
            Saves::Discarded
        }
    }

    /// What connects the emulated cartridge to these saves.
    fn cartridge_effects(&self) -> Box<dyn CartridgeEffects> {
        match self {
            Saves::Kept(saves) => Box::new(BatteryCartridgeEffects::new(saves.clone())),
            Saves::Discarded => Box::new(NoSaves {
                rtc_clock: RtcClock::EmulatedTime,
            }),
        }
    }

    /// Writes the saves the game has stopped writing to for a moment (see
    /// [`BatterySaves::flush_when_settled`]).
    fn flush_when_settled(&self) {
        if let Saves::Kept(saves) = self {
            saves.flush_when_settled();
        }
    }

    /// Writes the saves still waiting.
    fn flush(&self) {
        if let Saves::Kept(saves) = self {
            saves.flush();
        }
    }
}

/// A request from the app to the emulator.
#[derive(Debug)]
pub enum Command {
    /// Presses a Game Boy button.
    Press(Key),
    /// Lets go of a Game Boy button.
    Release(Key),
    /// Plays as this says from now on.
    SetPlayback(Playback),
    /// What the controllers are doing now.
    SetControllerStates(Vec<ControllerState>),
    /// Which controller buttons press which Game Boy buttons.
    SetControllerMapping(ControllerMapping),
    /// Turns a cheat on or off, as its `enabled` field says.
    SetCheat(Cheat),
    /// Asks for a save state, sent back over the channel.
    SaveState(Sender<Vec<u8>>),
    /// Restores a save state and sends back whether that worked.
    LoadState(Vec<u8>, Sender<Result<(), String>>),
    /// On a Super Game Boy, colors the screen with the player's palette, or
    /// with the game's colors for `None` (see `sgb_menu`).
    SetSgbPlayerPalette(Option<[u16; 4]>),
    /// On a Super Game Boy, picks the border around the screen.
    SetSgbBorder(SgbBorder),
    /// Runs this many frames, whether the game is paused or not, and sends
    /// back the last two the emulator finished (see
    /// [`GameSession::run_frames`]).
    RunFrames(u32, Sender<Vec<Frame>>),
}

/// The emulated Game Boy with its sound output and battery saves, playing
/// the way the last [`Playback`] said, with its recent past for rewinding.
/// Dropping it writes the battery saves still waiting.
struct Machine {
    emulator: Emulator,
    audio: AudioOutput,
    saves: Saves,
    playback: Playback,
    history: History,
    /// Playing time gone by while rewinding since the last step back.
    rewind_time: Duration,
    /// Turns a Super Game Boy's border to its night or day look as the
    /// player's clock says.
    border_clock: BorderClock,
}

impl Machine {
    /// Opens the sound output and loads `rom` on `model` with its battery
    /// saves kept in `saves`, returning the load error if it couldn't. It
    /// plays as `playback` says from the start, so a muted player hears
    /// nothing. Problems while it runs, such as a missing sound device, go
    /// to `notices`.
    fn start(
        rom: &Rom,
        saves: Saves,
        model: Model,
        playback: Playback,
        notices: &Notices,
    ) -> Result<Machine, String> {
        let audio = AudioOutput::open(notices);
        Machine::new(&rom.bytes, saves, audio, model, playback)
    }

    /// Loads `rom` on `model`, playing through `audio` as `playback` says and
    /// keeping its battery saves in `saves`.
    fn new(
        rom: &[u8],
        saves: Saves,
        mut audio: AudioOutput,
        model: Model,
        playback: Playback,
    ) -> Result<Machine, String> {
        audio.set_volume(playback.volume);
        if !playback.running {
            audio.set_paused(true);
        }

        let mut emulator = Emulator::new(model);
        emulator.set_sample_rate(audio.sample_rate());
        emulator
            .load_rom(rom, saves.cartridge_effects())
            .map_err(|error| error.to_string())?;
        let border_clock = BorderClock::start(&mut emulator);

        Ok(Machine {
            emulator,
            audio,
            saves,
            playback,
            history: History::default(),
            rewind_time: Duration::ZERO,
            border_clock,
        })
    }

    /// Carries out a command from the UI. Returns whether it changed the
    /// frame on screen, which the app then has to be shown even while the
    /// game is paused.
    fn handle(&mut self, command: Command) -> bool {
        match command {
            Command::Press(key) => self.emulator.press_key(key),
            Command::Release(key) => self.emulator.release_key(key),
            Command::SetPlayback(playback) => self.set_playback(playback),
            Command::SetControllerStates(states) => self.emulator.set_controller_states(&states),
            Command::SetControllerMapping(mapping) => self.emulator.set_controller_mapping(mapping),
            Command::SetCheat(cheat) => cheat.apply_to(&mut self.emulator),
            Command::SaveState(reply) => {
                let _ = reply.send(self.emulator.save_state());
            }
            Command::LoadState(state, reply) => {
                let result = self
                    .emulator
                    .load_state(&state)
                    .map_err(|error| error.to_string());
                // The history leads up to the moment before the load, so
                // rewinding through it would jump away from the loaded one.
                if result.is_ok() {
                    self.history.clear();
                    self.rewind_time = Duration::ZERO;
                }
                let _ = reply.send(result);
            }
            Command::SetSgbPlayerPalette(palette) => {
                self.emulator.set_sgb_player_palette(palette);
                return true;
            }
            Command::SetSgbBorder(border) => {
                self.emulator.set_sgb_border(border);
                return true;
            }
            Command::RunFrames(count, reply) => {
                let _ = reply.send(self.run_frames(count));
            }
        }
        false
    }

    /// Runs `count` frames of emulated time with `Emulator::run_frame`,
    /// without playing their sound, and returns the last two frames that
    /// finished, the newest last. Runs the same from the same state, so a
    /// save state and a count make a picture that can always be made again.
    fn run_frames(&mut self, count: u32) -> Vec<Frame> {
        let (mut left, mut right) = (Vec::new(), Vec::new());
        let mut frames = Vec::new();
        for _ in 0..count {
            self.emulator.run_frame(&mut left, &mut right);
            left.clear();
            right.clear();
            if self.emulator.take_frame().is_some() {
                if frames.len() == 2 {
                    frames.remove(0);
                }
                frames.push(Frame::latest(&self.emulator));
            }
        }
        frames
    }

    /// Plays as `playback` says from now on. Pausing also writes the battery
    /// saves still waiting: the game can't finish them while it is paused,
    /// and nothing else would write them before it goes on.
    fn set_playback(&mut self, playback: Playback) {
        if playback.running != self.playback.running {
            self.audio.set_paused(!playback.running);
            if !playback.running {
                self.saves.flush();
            }
        }
        self.audio.set_volume(playback.volume);
        self.playback = playback;
    }

    /// Emulates one audio buffer's worth of time and queues its sound.
    /// Returns how long that sound takes to play, and whether a frame
    /// finished meanwhile (see `Frame::latest`).
    fn play_buffer(&mut self) -> (Duration, bool) {
        let (left, right) = self.emulator.step_until_next_audio_buffer();
        let playing_time = self.audio.queue(left, right);
        (playing_time, self.frame_finished())
    }

    /// Emulates one audio buffer's worth of time without playing it, for
    /// fast forward. Returns how long that sound would have taken to play,
    /// and whether a frame finished meanwhile.
    fn skip_buffer(&mut self) -> (Duration, bool) {
        let samples = self.emulator.step_until_next_audio_buffer().0.len();
        let emulated_time = self.samples_time(samples);
        (emulated_time, self.frame_finished())
    }

    /// Whether the emulator finished a frame since this was last asked,
    /// keeping a state for rewinding every few frames and keeping a Super
    /// Game Boy's border in step with the clock.
    fn frame_finished(&mut self) -> bool {
        let finished = self.emulator.take_frame().is_some();
        if finished {
            if self.history.frame_finished() {
                self.history.push(self.emulator.save_state());
            }
            self.border_clock.frame_finished(&mut self.emulator);
        }
        finished
    }

    /// How long `samples` take to play at the sound card's rate.
    fn samples_time(&self, samples: usize) -> Duration {
        Duration::from_secs_f64(samples as f64 / f64::from(self.audio.sample_rate()))
    }

    /// Plays one buffer of silence while rewinding, and when it is time,
    /// steps back to the newest state kept (see [`History`]), running to the
    /// end of the frame after it to show it. That goes back at
    /// [`REWIND_SPEED`] times the speed of play. Returns how long the
    /// silence takes to play, and whether there is a new frame to show; at
    /// the oldest state kept the picture stays.
    fn rewind_buffer(&mut self) -> (Duration, bool) {
        let silence = vec![0.0; REWIND_BUFFER_SAMPLES];
        let playing_time = self.audio.queue(&silence, &silence);
        self.rewind_time += playing_time;

        let step = FRAME_DURATION.mul_f64(f64::from(history::FRAMES_PER_STATE) / REWIND_SPEED);
        if self.rewind_time < step {
            return (playing_time, false);
        }
        // The time past the step counts towards the next one, so the speed
        // doesn't depend on how the buffers' length divides into it.
        self.rewind_time = (self.rewind_time - step).min(step);

        let Some(state) = self.history.pop() else {
            return (playing_time, false);
        };
        if self.emulator.load_state(&state).is_err() {
            self.history.clear();
            return (playing_time, false);
        }
        while self.emulator.take_frame().is_none() {
            self.emulator.step_until_next_audio_buffer();
        }
        (playing_time, true)
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        self.saves.flush();
    }
}

/// Sends a command that answers over a channel with `send`, and waits for
/// the answer. In the browser the command is carried out before `send`
/// returns, so the answer is already there and this doesn't wait.
fn ask<T>(
    send: impl FnOnce(Command),
    command: impl FnOnce(Sender<T>) -> Command,
) -> Result<T, String> {
    let (reply, answer) = mpsc::channel();
    send(command(reply));
    answer.recv().map_err(|_| stopped_error())
}

/// What went wrong when the emulator thread ended before it answered.
fn stopped_error() -> String {
    String::from("the emulator stopped unexpectedly")
}

mod audio;
mod game;
mod history;
#[cfg_attr(
    all(not(target_arch = "wasm32"), not(test)),
    expect(
        dead_code,
        reason = "only the browser session paces frames this way so far"
    )
)]
mod pacing;
#[cfg(target_arch = "wasm32")]
mod screen_refresh;
#[cfg(not(target_arch = "wasm32"))]
mod sound_card;
#[cfg(test)]
mod tests;
