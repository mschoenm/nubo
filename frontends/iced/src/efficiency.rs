//! Measures how much the desktop app's emulator thread wakes the processor,
//! which is most of what a running game costs a laptop's battery besides the
//! emulation itself. Run it with `nubo --measure-efficiency <rom> [--model
//! <name>]`: it plays the game without a window or sound for a few seconds in
//! each situation below and prints one line of JSON, which the nightly CI
//! keeps in its metrics log.
//!
//! The emulation's own cost is measured separately, as instructions per
//! emulated second with `cargo benchmark` under cachegrind. Real battery
//! drain and the window's drawing can only be measured on real hardware.

use crate::battery::BatterySaves;
use crate::emulation::{ActivityCount, Command, Playback, Session};
use crate::notices::Notices;
use crate::rom::Rom;
use crate::settings::FastForwardSpeed;
use nubo_core::Model;
use std::env;
use std::path::{Path, PathBuf};
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
    /// Frames handed to the window per second (identical frames are
    /// skipped).
    frames: f64,
}

/// Runs the measurement when the command line starts with
/// `--measure-efficiency`, printing its JSON and returning the exit code, or
/// returns `None` for a normal start of the app.
pub fn run_from_command_line() -> Option<i32> {
    let mut args = env::args().skip(1);
    if args.next().as_deref() != Some("--measure-efficiency") {
        return None;
    }

    let mut rom_path = None;
    let mut model = Model::Dmg;
    while let Some(arg) = args.next() {
        if arg == "--model" {
            match args.next().map(|name| name.parse()) {
                Some(Ok(parsed)) => model = parsed,
                Some(Err(error)) => {
                    eprintln!("{error}");
                    return Some(2);
                }
                None => {
                    eprintln!("--model needs a value such as dmg, mgb, sgb, cgb or agb");
                    return Some(2);
                }
            }
        } else {
            rom_path = Some(PathBuf::from(arg));
        }
    }
    let Some(rom_path) = rom_path else {
        eprintln!("usage: nubo --measure-efficiency <rom> [--model <name>]");
        return Some(2);
    };

    match measure(&rom_path, model) {
        Ok(json) => {
            println!("{json}");
            Some(0)
        }
        Err(error) => {
            eprintln!("{error}");
            Some(1)
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
pub fn measure(rom_path: &Path, model: Model) -> Result<String, String> {
    let rom = Rom::read(rom_path)?;
    let saves_directory = std::env::temp_dir().join("nubo-efficiency");
    let saves = || BatterySaves::in_directory(&saves_directory, &rom.path);
    let normal = Playback {
        running: true,
        fast_forward: None,
        volume: 0.0,
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
    let session = Session::start_with_saves(rom, saves, model, playback, &Notices::default())?;
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
