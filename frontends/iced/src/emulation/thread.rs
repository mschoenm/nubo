//! The desktop session: the emulator runs on its own thread, and the audio
//! queue paces it. The thread sleeps until the sound card is about to run
//! out of samples, then emulates more. The UI thread sends commands over a
//! channel (waking the thread so it sees them) and picks up finished frames
//! from a shared buffer, after the thread tells it a new one is there.

use super::{Command, Machine, Playback, ask, stopped_error};
use crate::notices::Notices;
use crate::rom::Rom;
use iced::Subscription;
use iced::futures::channel::mpsc as frame_channel;
use nubo_core::Model;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// The longest the emulator thread waits for the sound card. The card's
/// callback and commands wake it sooner; this only matters if the callback
/// stops coming, such as when the sound device goes away.
const LONGEST_SLEEP: Duration = Duration::from_secs(1);

/// The latest finished frame, shared between the two threads.
#[derive(Default)]
struct SharedFrame {
    /// Goes up by one for every new frame that differs from the one before,
    /// so readers can tell whether the pixels changed since they last looked.
    sequence: u64,
    pixels: Vec<u8>,
    /// Told about every new frame while the UI listens (see
    /// [`Session::new_frames`]).
    listener: Option<frame_channel::Sender<()>>,
}

/// Numbers sessions in the order they start (see [`FrameListener`]).
static NEXT_SESSION_NUMBER: AtomicU64 = AtomicU64::new(0);

/// One session's shared frame, told apart from other sessions' by the
/// session's number, so that each session's [`Session::new_frames`] is a
/// subscription of its own. The `Arc`'s address wouldn't do: a game started
/// right after another stopped can get the same address, and Iced would then
/// keep the stopped session's subscription.
struct FrameListener {
    session_number: u64,
    frame: Arc<Mutex<SharedFrame>>,
}

impl Hash for FrameListener {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.session_number.hash(state);
    }
}

impl FrameListener {
    /// Starts telling a new receiver about new frames, replacing any
    /// earlier one. A channel of 0 still holds one message per sender, so
    /// notices of frames the UI hasn't picked up yet merge into one. One
    /// notice is sent straight away, since frames finished before the UI
    /// listened were never announced.
    fn listen(&self) -> frame_channel::Receiver<()> {
        let (mut sender, receiver) = frame_channel::channel(0);
        let _ = sender.try_send(());
        if let Ok(mut frame) = self.frame.lock() {
            frame.listener = Some(sender);
        }
        receiver
    }
}

/// A running game. Dropping it stops the emulator thread and writes any
/// pending battery saves.
pub struct Session {
    /// Tells this session apart from the others (see [`FrameListener`]).
    number: u64,
    commands: Sender<Command>,
    frame: Arc<Mutex<SharedFrame>>,
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Session {
    /// Starts an emulator thread for `rom` on `model`, playing as `playback`
    /// says from the start, and waits until the ROM has loaded, returning
    /// the load error if it couldn't. Problems while the game runs, such as
    /// a missing sound device, go to `notices`.
    pub fn start(
        rom: &Rom,
        model: Model,
        playback: Playback,
        notices: &Notices,
    ) -> Result<Session, String> {
        let (commands, command_receiver) = mpsc::channel();
        let (loaded_sender, loaded_receiver) = mpsc::channel();
        let frame = Arc::new(Mutex::new(SharedFrame::default()));
        let running = Arc::new(AtomicBool::new(true));

        let thread = {
            let rom = rom.clone();
            let frame = frame.clone();
            let running = running.clone();
            let notices = notices.clone();

            thread::Builder::new()
                .name(String::from("emulator"))
                .spawn(
                    move || match Machine::start(&rom, model, playback, &notices) {
                        Ok(machine) => {
                            machine.audio.wake_when_low(thread::current());
                            let _ = loaded_sender.send(Ok(()));
                            EmulatorThread {
                                machine,
                                commands: command_receiver,
                                frame,
                                running,
                                fast_forward: FastForward::new(),
                            }
                            .run();
                        }
                        Err(error) => {
                            let _ = loaded_sender.send(Err(error));
                        }
                    },
                )
                .map_err(|error| error.to_string())?
        };

        match loaded_receiver.recv() {
            Ok(Ok(())) => Ok(Session {
                number: NEXT_SESSION_NUMBER.fetch_add(1, Ordering::Relaxed),
                commands,
                frame,
                running,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                let _ = thread.join();
                Err(stopped_error())
            }
        }
    }

    /// Passes `command` to the emulator thread and wakes it to handle it.
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
    }

    /// Captures the game's complete state, waiting for the emulator thread to
    /// finish what it is doing (a few milliseconds at most).
    pub fn save_state(&self) -> Result<Vec<u8>, String> {
        ask(|command| self.send(command), Command::SaveState)
    }

    /// Restores a state captured with [`Session::save_state`].
    pub fn load_state(&self, state: Vec<u8>) -> Result<(), String> {
        ask(
            |command| self.send(command),
            |reply| Command::LoadState(state, reply),
        )?
    }

    /// Produces an item whenever the emulator finishes a frame, so the window
    /// only redraws when there is something new to show: about 60 times a
    /// second at normal speed, and more when fast forwarding, up to what the
    /// screen can show.
    pub fn new_frames(&self) -> Subscription<()> {
        let listener = FrameListener {
            session_number: self.number,
            frame: self.frame.clone(),
        };
        Subscription::run_with(listener, FrameListener::listen)
    }

    /// Returns the newest frame and its sequence number, or `None` if it is
    /// still the frame with the given sequence number.
    pub fn frame_since(&self, sequence: u64) -> Option<(u64, Vec<u8>)> {
        let frame = self.frame.lock().ok()?;
        (frame.sequence != sequence).then(|| (frame.sequence, frame.pixels.clone()))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

/// What the emulator thread works with.
struct EmulatorThread {
    machine: Machine,
    commands: Receiver<Command>,
    frame: Arc<Mutex<SharedFrame>>,
    /// Cleared when the session ends.
    running: Arc<AtomicBool>,
    fast_forward: FastForward,
}

impl EmulatorThread {
    /// Handles commands and emulates whenever the audio queue needs more
    /// samples, until the session ends. In between it sleeps until something
    /// wakes it: the sound card's callback as the queue runs low, a command,
    /// or the end of the session.
    ///
    /// No wake-up can get lost between deciding to sleep and sleeping: an
    /// [`unpark`](thread::Thread::unpark) that comes before the thread parks
    /// leaves a token, and the park then returns straight away.
    fn run(mut self) {
        while self.running.load(Ordering::Relaxed) {
            for command in self.commands.try_iter() {
                self.machine.handle(command);
            }
            self.step();
        }
    }

    /// Emulates what the sound card needs next, or sleeps until something
    /// wakes the thread.
    ///
    /// To fast forward, it emulates several audio buffers for each one it
    /// plays (see [`super::Playback::buffers_per_played`]): the played one
    /// when the queue needs it, and the others spread over the time that one
    /// takes to play (see [`FastForward`]). The sound card still paces the
    /// loop, and the sound stays at its normal pitch.
    fn step(&mut self) {
        self.machine.saves.flush_when_settled();

        // Pausing wrote the battery saves, and nothing new comes in while
        // the game is paused, so only a command or the end of the session
        // can give the thread something to do.
        if !self.machine.playback.running {
            thread::park();
            return;
        }

        if self.machine.audio.needs_samples() {
            // Buffers fast forward hasn't emulated yet go first, so it keeps
            // its exact speed.
            while self.fast_forward.take_buffer() {
                self.skip_buffer();
            }

            let (playing_time, pixels) = self.machine.play_buffer();
            if let Some(pixels) = pixels {
                publish_frame(&self.frame, pixels);
            }
            let unplayed = self.machine.playback.buffers_per_played() - 1;
            self.fast_forward.spread(unplayed, playing_time);
        } else if let Some(wait) = self.fast_forward.time_until_next_buffer() {
            if wait.is_zero() && self.fast_forward.take_buffer() {
                self.skip_buffer();
            } else {
                thread::park_timeout(wait);
            }
        } else {
            self.machine.audio.wait_until_samples_needed(LONGEST_SLEEP);
        }
    }

    /// Emulates one audio buffer without playing it, for fast forward.
    fn skip_buffer(&mut self) {
        if let Some(pixels) = self.machine.skip_buffer().1 {
            publish_frame(&self.frame, pixels);
        }
    }
}

/// The audio buffers fast forward emulates without playing them, spread
/// evenly over the time the played buffer before them takes to play. That
/// way their frames come out at an even pace, and a screen faster than
/// 60 Hz can show them, instead of arriving all at once with only the last
/// one shown.
struct FastForward {
    /// Buffers still to emulate before the next played one.
    remaining: u32,
    /// The time between two of them.
    interval: Duration,
    /// When the next one is due.
    next_at: Instant,
}

impl FastForward {
    fn new() -> Self {
        FastForward {
            remaining: 0,
            interval: Duration::ZERO,
            next_at: Instant::now(),
        }
    }

    /// Schedules `buffers` unplayed buffers over `playing_time`, the time the
    /// buffer just queued takes to play.
    fn spread(&mut self, buffers: u32, playing_time: Duration) {
        self.remaining = buffers;
        self.interval = playing_time / (buffers + 1);
        self.next_at = Instant::now() + self.interval;
    }

    /// How long until the next unplayed buffer is due, or `None` if there
    /// are none left.
    fn time_until_next_buffer(&self) -> Option<Duration> {
        (self.remaining > 0).then(|| self.next_at.saturating_duration_since(Instant::now()))
    }

    /// Counts one unplayed buffer as emulated, returning `false` if there
    /// were none left.
    fn take_buffer(&mut self) -> bool {
        if self.remaining == 0 {
            return false;
        }
        self.remaining -= 1;
        self.next_at += self.interval;
        true
    }
}

/// Copies a finished frame into the shared buffer for the UI thread, and
/// tells the UI if it listens. A notice the UI hasn't picked up yet already
/// covers this frame, since the UI always takes the newest one. A frame
/// exactly like the one before is skipped, so still screens such as menus
/// and text boxes don't redraw the window at all.
fn publish_frame(frame: &Mutex<SharedFrame>, pixels: &[u8]) {
    if let Ok(mut shared) = frame.lock() {
        if shared.pixels == pixels {
            return;
        }
        shared.pixels.clear();
        shared.pixels.extend_from_slice(pixels);
        shared.sequence += 1;
        if let Some(listener) = &mut shared.listener {
            let _ = listener.try_send(());
        }
    }
}

#[cfg(test)]
mod tests;
