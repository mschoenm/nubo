//! The session paced by the sound card, which the desktop uses: the
//! emulator runs on its own thread, and the audio queue paces it. The
//! thread sleeps until the sound card is about to run out of samples, then
//! emulates more. The app sends commands over a channel (waking the thread
//! so it sees them) and picks up finished frames from a shared buffer,
//! after the thread calls the app's callback to say a new one is there.

use super::{Command, LatestFrames, Machine, Playback, Saves, ask, stopped_error};
use crate::notices::Notices;
use crate::rom::Rom;
use nubo_core::{Emulator, Model};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// The longest the emulator thread waits for the sound card. The card's
/// callback and commands wake it sooner, as does the device going away (see
/// `AudioOutput`); this only matters if the callback stops coming without a
/// word from the device.
const LONGEST_SLEEP: Duration = Duration::from_secs(1);

/// The two newest finished frames, shared between the two threads.
#[derive(Default)]
struct SharedFrame {
    /// Goes up by one for every new frame that differs from the one before
    /// (and for the first frame like it, see [`publish`]), so readers can
    /// tell whether there is a frame they haven't shown.
    sequence: u64,
    /// The newest frame and the one the emulator finished before it, for
    /// frame blending.
    frames: LatestFrames,
    /// Whether the frame published last was the same as the one before it.
    repeated: bool,
    /// Called for every new frame while the app listens (see
    /// [`FrameListener::listen`]).
    on_frame: Option<Box<dyn FnMut() + Send>>,
}

/// Numbers sessions in the order they start (see [`FrameListener`]).
static NEXT_SESSION_NUMBER: AtomicU64 = AtomicU64::new(0);

/// Tells the app about one session's new frames (see
/// [`Session::frame_listener`]). It hashes as the session's number, so that
/// an app that tells its listeners apart by their hash, as Iced's
/// subscriptions do, gives each session a listener of its own. The `Arc`'s
/// address wouldn't do: a game started right after another stopped can get
/// the same address, and Iced would then keep the stopped session's
/// subscription.
pub struct FrameListener {
    session_number: u64,
    frame: Arc<Mutex<SharedFrame>>,
}

impl Hash for FrameListener {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.session_number.hash(state);
    }
}

impl FrameListener {
    /// Calls `on_frame` for every new frame from now on, replacing any
    /// earlier callback. It is called once straight away, since frames
    /// finished before the app listened were never announced. The app then
    /// picks up the newest frame, with the one before it, with
    /// [`Session::frame_since`]. A frame still waiting when the next one
    /// comes is replaced by it, so the callback can merge the calls the app
    /// hasn't handled yet into one, such as by sending into a channel that
    /// holds one message.
    ///
    /// The callback runs while the frame is locked, the first time on the
    /// thread that calls `listen` and after that on the emulator thread, so
    /// it must be quick and must not call the session.
    pub fn listen(&self, on_frame: impl FnMut() + Send + 'static) {
        // Installing it and the first call happen under one lock, so a frame finished in between
        // can't go unannounced.
        if let Ok(mut frame) = self.frame.lock() {
            let on_frame = frame.on_frame.insert(Box::new(on_frame));
            on_frame();
        }
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
    activity: Arc<Activity>,
    thread: Option<JoinHandle<()>>,
}

/// Counts what the emulator thread does, to measure how often it wakes the
/// processor (see the app's `nubo measure-efficiency`). Every wake-up
/// keeps a laptop's processor from resting, so fewer of them save battery.
#[derive(Debug, Default)]
struct Activity {
    /// How often the thread went to sleep, and so woke up again.
    wakeups: AtomicU64,
    /// Audio buffers emulated, played or skipped.
    buffers: AtomicU64,
}

/// What the emulator thread had done at one moment (see
/// [`Session::activity`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivityCount {
    /// How often the thread went to sleep, and so woke up again.
    pub wakeups: u64,
    /// Audio buffers emulated, played or skipped.
    pub buffers: u64,
    /// Frames handed to the app. Frames exactly like the one before are
    /// skipped, but for the first.
    pub frames: u64,
}

impl Session {
    /// Starts an emulator thread for `rom` on `model`, with its battery
    /// saves kept in `saves`, playing as `playback` says from the start, and
    /// waits until the ROM has loaded, returning the load error if it
    /// couldn't. Problems while the game runs, such as a missing sound
    /// device, go to `notices`.
    pub fn start(
        rom: &Rom,
        saves: Saves,
        model: Model,
        playback: Playback,
        notices: &Notices,
    ) -> Result<Session, String> {
        let (commands, command_receiver) = mpsc::channel();
        let (loaded_sender, loaded_receiver) = mpsc::channel();
        let frame = Arc::new(Mutex::new(SharedFrame::default()));
        let running = Arc::new(AtomicBool::new(true));
        let activity = Arc::new(Activity::default());

        let thread = {
            let rom = rom.clone();
            let frame = frame.clone();
            let running = running.clone();
            let activity = activity.clone();
            let notices = notices.clone();

            thread::Builder::new()
                .name(String::from("emulator"))
                .spawn(
                    move || match Machine::start(&rom, saves, model, playback, &notices) {
                        Ok(machine) => {
                            machine.audio.wake_when_low(thread::current());
                            let _ = loaded_sender.send(Ok(()));
                            EmulatorThread {
                                machine,
                                commands: command_receiver,
                                frame,
                                running,
                                activity,
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
                activity,
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

    /// Something that tells the app whenever the emulator finishes a new
    /// frame, so its window only redraws when there is something new to
    /// show: about 60 times a second at normal speed, and more when fast
    /// forwarding, up to what the screen can show.
    pub fn frame_listener(&self) -> FrameListener {
        FrameListener {
            session_number: self.number,
            frame: self.frame.clone(),
        }
    }

    /// Copies the newest frame and the one the emulator finished before it
    /// into `frames`, reusing their memory, and returns the newest's
    /// sequence number. Returns `None` and leaves `frames` alone if the
    /// newest is still the frame with the given sequence number.
    pub fn frame_since(&self, sequence: u64, frames: &mut LatestFrames) -> Option<u64> {
        self.frame.lock().ok()?.copy_since(sequence, frames)
    }

    /// What the emulator thread has done since the session started.
    pub fn activity(&self) -> ActivityCount {
        ActivityCount {
            wakeups: self.activity.wakeups.load(Ordering::Relaxed),
            buffers: self.activity.buffers.load(Ordering::Relaxed),
            frames: self.frame.lock().map_or(0, |frame| frame.sequence),
        }
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

impl SharedFrame {
    /// See [`Session::frame_since`].
    fn copy_since(&self, sequence: u64, frames: &mut LatestFrames) -> Option<u64> {
        (self.sequence != sequence).then(|| {
            frames.clone_from(&self.frames);
            self.sequence
        })
    }
}

/// What the emulator thread works with.
struct EmulatorThread {
    machine: Machine,
    commands: Receiver<Command>,
    frame: Arc<Mutex<SharedFrame>>,
    /// Cleared when the session ends.
    running: Arc<AtomicBool>,
    activity: Arc<Activity>,
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
                if self.machine.handle(command) {
                    publish_frame(&self.frame, &self.machine.emulator);
                }
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
    /// loop, and the sound stays at its normal pitch. While rewinding it
    /// plays silence instead, stepping back through the game's recent past
    /// (see `Machine::rewind_buffer`).
    fn step(&mut self) {
        self.machine.saves.flush_when_settled();

        // Pausing wrote the battery saves, and nothing new comes in while
        // the game is paused, so only a command or the end of the session
        // can give the thread something to do.
        if !self.machine.playback.running {
            self.count_wakeup();
            thread::park();
            return;
        }

        if self.machine.playback.rewinding {
            // Fast forward's buffers still due belong to the time being
            // rewound; running them after the rewind would undo part of it.
            self.fast_forward.clear();
            if self.machine.audio.needs_samples() {
                self.activity.buffers.fetch_add(1, Ordering::Relaxed);
                if self.machine.rewind_buffer().1 {
                    publish_frame(&self.frame, &self.machine.emulator);
                }
            } else {
                self.count_wakeup();
                self.machine.audio.wait_until_samples_needed(LONGEST_SLEEP);
            }
            return;
        }

        if self.machine.audio.needs_samples() {
            // Buffers fast forward hasn't emulated yet go first, so it keeps
            // its exact speed.
            while self.fast_forward.take_buffer() {
                self.skip_buffer();
            }

            self.activity.buffers.fetch_add(1, Ordering::Relaxed);
            let (playing_time, new_frame) = self.machine.play_buffer();
            if new_frame {
                publish_frame(&self.frame, &self.machine.emulator);
            }
            let unplayed = self.machine.playback.buffers_per_played() - 1;
            self.fast_forward.spread(unplayed, playing_time);
        } else if let Some(wait) = self.fast_forward.time_until_next_buffer() {
            if wait.is_zero() && self.fast_forward.take_buffer() {
                self.skip_buffer();
            } else {
                self.count_wakeup();
                thread::park_timeout(wait);
            }
        } else {
            self.count_wakeup();
            self.machine.audio.wait_until_samples_needed(LONGEST_SLEEP);
        }
    }

    /// Counts a sleep the thread is about to go into, and so the wake-up
    /// after it.
    fn count_wakeup(&self) {
        self.activity.wakeups.fetch_add(1, Ordering::Relaxed);
    }

    /// Emulates one audio buffer without playing it, for fast forward.
    fn skip_buffer(&mut self) {
        self.activity.buffers.fetch_add(1, Ordering::Relaxed);
        if self.machine.skip_buffer().1 {
            publish_frame(&self.frame, &self.machine.emulator);
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

    /// Drops the unplayed buffers still due.
    fn clear(&mut self) {
        self.remaining = 0;
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

/// Copies the emulator's finished frame into the shared buffer for the app,
/// keeping the one before it, and calls the app's callback if it listens. A
/// call the app hasn't handled yet already covers this frame, since the app
/// always takes the newest one and the one before it. Once
/// the picture stops changing, the first frame like the one before is still
/// handed on, so a picture mixed with the frame before it (frame blending)
/// settles; the ones after it are skipped, so still screens such as menus
/// and text boxes don't redraw the window.
fn publish_frame(frame: &Mutex<SharedFrame>, emulator: &Emulator) {
    publish(frame, |frames| frames.update(emulator));
}

/// Lets `update` take in a new frame and say whether it differs from the one
/// before. Unless an unchanged frame was already handed on, counts it and
/// calls the app's callback.
fn publish(frame: &Mutex<SharedFrame>, update: impl FnOnce(&mut LatestFrames) -> bool) {
    if let Ok(mut shared) = frame.lock() {
        let changed = update(&mut shared.frames);
        if !changed && shared.repeated {
            return;
        }
        shared.repeated = !changed;
        shared.sequence += 1;
        if let Some(on_frame) = &mut shared.on_frame {
            on_frame();
        }
    }
}

#[cfg(test)]
mod tests;
