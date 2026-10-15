//! Measures how much the desktop app's emulator thread (the session paced
//! by the sound card, in `nubo_frontend::session`) wakes the processor,
//! which is most of what a running game costs a laptop's battery besides the
//! emulation itself. Run it with `nubo --measure-efficiency <rom> [--cgb |
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
use std::path::Path;
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

/// Runs the measurement that `nubo --measure-efficiency <rom>` asks for
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
/// saves are never touched.
pub fn measure(rom_path: &Path, model: Model) -> Result<String, String> {
    let rom = Rom::read(rom_path)?;
    let data_folder = std::env::temp_dir().join("nubo-efficiency");
    let saves = || BatterySaves::in_data_folder(&data_folder, &rom.path);
    let normal = Playback {
        running: true,
        fast_forward: None,
        volume: 0.0,
        rewinding: false,
    };

    let playing = measure_session(&rom, saves(), model, normal, |_| ())?;
    let paused = measure_session(&rom, saves(), model, normal, |session| {
        session.send(Command::SetPlayback(Playback {
            running: false,
            ..normal
        }));
    })?;
    let fast_forward = measure_session(
        &rom,
        saves(),
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
    let still_screen = measure_session(&blank, saves(), model, normal, |_| ())?;

    Ok(format!(
        "{{\"playing\":{},\"paused\":{},\"fast_forward\":{},\"still_screen\":{}}}",
        json(&playing),
        json(&paused),
        json(&fast_forward),
        json(&still_screen),
    ))
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
