//! Measures how much the desktop app's emulator thread (the session paced
//! by the sound card, in `nubo_frontend::session`) wakes the processor,
//! which is most of what a running game costs a laptop's battery besides the
//! emulation itself. Run it with `nubo measure-efficiency <rom> [--cgb |
//! --model <name>]` (`command_line`): it plays the game without a window or
//! sound for a few seconds in each situation below and prints one line of
//! JSON, which the nightly CI keeps in its metrics log.
//!
//! The emulation's own cost is measured separately, as instructions per
//! emulated second with `cargo benchmark` under cachegrind. Real battery
//! drain and the window's drawing can only be measured on real hardware.

use nubo_core::Model;
use nubo_frontend::battery::BatterySaves;
use nubo_frontend::notices::Notices;
use nubo_frontend::rom::Rom;
use nubo_frontend::session::{ActivityCount, Command, Playback, Saves, Session};
use nubo_frontend::settings::FastForwardSpeed;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// How long the game runs before each measurement starts, so the boot ROM
/// and the start of the game are left out.
const SETTLE_TIME: Duration = Duration::from_secs(2);
/// How long each situation is measured.
const MEASURE_TIME: Duration = Duration::from_secs(5);

/// The rates measured in one situation.
struct Rates {
    /// How often the emulator thread woke up, per second.
    wakeups: f64,
    /// Audio buffers emulated per second.
    buffers: f64,
    /// Frames handed to the window per second (repeats of a frame are
    /// skipped, but for the first).
    frames: f64,
}

/// Runs the measurement that `nubo measure-efficiency <rom>` asks for
/// (`command_line`), printing its JSON, and returns the exit code.
pub fn run(rom_path: &Path, model: Model) -> ExitCode {
    match measure(rom_path, model) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Measures the game at `rom_path` on `model` in four situations and returns
/// them as one line of JSON:
///
/// - `playing`: at normal speed;
/// - `paused`: the thread should hardly wake at all;
/// - `fast_forward`: at the default fast forward speed;
/// - `still_screen`: a cartridge that never starts, so the screen stays
///   still after the boot ROM and no frames should reach the window.
///
/// The game's battery saves go to a temporary folder, so the player's own
/// saves are never touched. Each situation starts from no saves at all, in
/// an empty folder of its own, so no measurement starts from what an
/// earlier one (or an earlier run) left; the saves are still written, as
/// for a player, so that work stays in the numbers.
pub fn measure(rom_path: &Path, model: Model) -> Result<String, String> {
    let rom = Rom::read(rom_path)?;
    let folder = TemporaryFolder::new()?;
    let saves = |situation: &str| folder.empty_saves(situation, &rom.path);
    let normal = Playback {
        running: true,
        fast_forward: None,
        volume: 0.0,
        rewinding: false,
    };

    let playing = measure_session(&rom, saves("playing")?, model, normal, |_| ())?;
    let paused = measure_session(&rom, saves("paused")?, model, normal, |session| {
        session.send(Command::SetPlayback(Playback {
            running: false,
            ..normal
        }));
    })?;
    let fast_forward = measure_session(
        &rom,
        saves("fast_forward")?,
        model,
        Playback {
            fast_forward: Some(FastForwardSpeed::default()),
            ..normal
        },
        |_| (),
    )?;
    let blank = Rom {
        path: rom.path.with_file_name("nubo-efficiency-blank.gb"),
        bytes: vec![0; 0x8000].into(),
    };
    let still_screen = measure_session(&blank, saves("still_screen")?, model, normal, |_| ())?;

    Ok(format!(
        "{{\"playing\":{},\"paused\":{},\"fast_forward\":{},\"still_screen\":{}}}",
        json(&playing),
        json(&paused),
        json(&fast_forward),
        json(&still_screen),
    ))
}

/// A folder of this run's own in the system's temporary folder, deleted with
/// everything in it when the measurement ends.
struct TemporaryFolder {
    path: PathBuf,
}

impl TemporaryFolder {
    /// Creates the folder, empty, named after this process, so two
    /// measurements running at once never share one.
    fn new() -> Result<TemporaryFolder, String> {
        let path = std::env::temp_dir().join(format!("nubo-efficiency-{}", std::process::id()));
        // A folder left by a process that had the same id before and was
        // killed.
        if path.exists() {
            fs::remove_dir_all(&path).map_err(|error| folder_error(&path, &error))?;
        }
        fs::create_dir_all(&path).map_err(|error| folder_error(&path, &error))?;
        Ok(TemporaryFolder { path })
    }

    /// Battery saves for the ROM at `rom_path`, in a new, empty folder named
    /// `situation`.
    fn empty_saves(&self, situation: &str, rom_path: &Path) -> Result<Arc<BatterySaves>, String> {
        let data_folder = self.path.join(situation);
        fs::create_dir(&data_folder).map_err(|error| folder_error(&data_folder, &error))?;
        Ok(BatterySaves::in_data_folder(&data_folder, rom_path))
    }
}

impl Drop for TemporaryFolder {
    fn drop(&mut self) {
        // Nothing else uses the folder; one left behind is only clutter.
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// The message for a temporary folder that couldn't be made ready.
fn folder_error(path: &Path, error: &std::io::Error) -> String {
    format!("Couldn't prepare the folder {}: {error}", path.display())
}

/// Starts `rom` playing as `playback` says, lets it settle, then calls
/// `prepare` and measures the thread for `MEASURE_TIME`.
fn measure_session(
    rom: &Rom,
    saves: Arc<BatterySaves>,
    model: Model,
    playback: Playback,
    prepare: impl FnOnce(&Session),
) -> Result<Rates, String> {
    let session = Session::start(
        rom,
        Saves::Kept(saves),
        model,
        playback,
        &Notices::default(),
    )?;
    thread::sleep(SETTLE_TIME);
    prepare(&session);
    // A command takes effect on the thread's next pass; a moment later the
    // counts only show the situation being measured.
    thread::sleep(Duration::from_millis(100));

    let start = session.activity();
    let started_at = Instant::now();
    thread::sleep(MEASURE_TIME);
    let end = session.activity();
    let seconds = started_at.elapsed().as_secs_f64();

    Ok(rates(start, end, seconds))
}

/// The rates between two counts taken `seconds` apart.
fn rates(start: ActivityCount, end: ActivityCount, seconds: f64) -> Rates {
    let per_second = |from: u64, to: u64| to.saturating_sub(from) as f64 / seconds;
    Rates {
        wakeups: per_second(start.wakeups, end.wakeups),
        buffers: per_second(start.buffers, end.buffers),
        frames: per_second(start.frames, end.frames),
    }
}

/// `rates` as a JSON object.
fn json(rates: &Rates) -> String {
    format!(
        "{{\"wakeups_per_second\":{:.1},\"buffers_per_second\":{:.1},\"frames_per_second\":{:.1}}}",
        rates.wakeups, rates.buffers, rates.frames
    )
}

#[cfg(test)]
mod tests;
