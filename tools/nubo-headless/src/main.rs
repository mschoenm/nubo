//! Runs a ROM without a window or sound for a fixed stretch of emulated time
//! and optionally saves a screenshot of the last frame. Useful for checking
//! test ROMs by hand. Every run of the same ROM ends on the same frame,
//! however fast the machine is. On the Super Game Boy models the screenshot
//! is the whole 256×224 picture with its border.

use chrono::Local;
use clap::Parser;
use image::RgbaImage;
use nubo_core::{Emulator, Model, NoSaves, RtcClock, SGB_FRAME_HEIGHT, SGB_FRAME_WIDTH};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

const SCREEN_WIDTH: u32 = 160;
const SCREEN_HEIGHT: u32 = 144;
/// Nothing is played, so any sample rate works. At this one, the core's
/// audio buffers of 512 samples are exactly 1/128 of a second.
const SAMPLE_RATE: u32 = 65536;

/// Run Nubo emulator headlessly without display or audio
#[derive(Parser)]
#[command(name = "nubo-headless")]
struct Args {
    /// Path to the ROM file
    rom: PathBuf,

    /// Emulated seconds to run: Game Boy time, the same on any machine
    seconds: f64,

    /// Capture a screenshot at the end
    #[arg(long)]
    screenshot: bool,

    /// Directory to save the screenshot (defaults to current directory)
    #[arg(long, value_name = "FOLDER")]
    screenshot_path: Option<PathBuf>,

    /// Run in CGB (Game Boy Color) mode, the same as --model cgb
    #[arg(long, conflicts_with = "model")]
    cgb: bool,

    /// Hardware model: dmg-0, dmg, mgb, sgb, sgb2, cgb-0, cgb-a, cgb-b, cgb-c, cgb-d, cgb-e (or cgb), agb
    #[arg(long, value_name = "NAME")]
    model: Option<Model>,
}

/// Saves the emulator's latest frame (with its border on a Super Game Boy) as
/// `<rom name>_<timestamp>.png` in `output_dir` (or the current directory) and
/// returns the file's path.
fn save_screenshot(
    emulator: &Emulator,
    rom_path: &Path,
    output_dir: Option<&Path>,
) -> Result<PathBuf, String> {
    let screenshot = match emulator.frame_with_border() {
        Some(frame) => RgbaImage::from_raw(SGB_FRAME_WIDTH, SGB_FRAME_HEIGHT, frame.to_vec()),
        None => RgbaImage::from_raw(
            SCREEN_WIDTH,
            SCREEN_HEIGHT,
            emulator.frame_buffer().to_vec(),
        ),
    }
    .ok_or("Failed to create image buffer")?;

    let rom_stem = rom_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("screenshot");

    let timestamp = Local::now().format("%y%m%d_%H%M%S");
    let filename = format!("{rom_stem}_{timestamp}.png");

    let output_path = match output_dir {
        Some(dir) => {
            fs::create_dir_all(dir)
                .map_err(|error| format!("Failed to create output directory: {error}"))?;
            dir.join(&filename)
        }
        None => PathBuf::from(&filename),
    };

    screenshot
        .save(&output_path)
        .map_err(|error| format!("Failed to save screenshot: {error}"))?;

    Ok(output_path)
}

/// Cartridge hooks that start every game without saved RAM or clock state,
/// discard whatever the game saves, and let a cartridge clock count emulated
/// time, so every run is the same.
const NO_SAVES: NoSaves = NoSaves {
    rtc_clock: RtcClock::EmulatedTime,
};

fn main() -> ExitCode {
    match run(&Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Loads the ROM, runs it for the requested emulated time and saves the
/// screenshot.
fn run(args: &Args) -> Result<(), String> {
    if !(args.seconds.is_finite() && args.seconds >= 0.0) {
        return Err(format!("Invalid number of seconds {}", args.seconds));
    }
    // Rounded to whole samples; a run ends with the audio buffer that reaches
    // this many, at most 1/128 of a second later.
    let samples_to_run = (args.seconds * f64::from(SAMPLE_RATE)).round() as u64;

    let rom_data = fs::read(&args.rom)
        .map_err(|error| format!("Failed to read ROM file {}: {error}", args.rom.display()))?;

    let model = args
        .model
        .unwrap_or(if args.cgb { Model::CgbE } else { Model::Dmg });
    let mut emulator = Emulator::new(model);
    emulator.set_sample_rate(SAMPLE_RATE);

    let header = emulator
        .load_rom(&rom_data, Box::new(NO_SAVES))
        .map_err(|error| format!("Failed to load ROM: {error}"))?;

    println!("Loaded: {}", header.title);
    println!("Running for {} emulated seconds...", args.seconds);

    let start = Instant::now();
    let mut frame_count: u64 = 0;
    let mut samples: u64 = 0;

    while samples < samples_to_run {
        let (left, _) = emulator.step_until_next_audio_buffer();
        samples += left.len() as u64;
        if emulator.take_frame().is_some() {
            frame_count += 1;
        }
    }

    let elapsed = start.elapsed();
    println!(
        "Ran {:.3} emulated seconds ({} frames) in {:.2}s ({:.1} fps)",
        samples as f64 / f64::from(SAMPLE_RATE),
        frame_count,
        elapsed.as_secs_f64(),
        frame_count as f64 / elapsed.as_secs_f64()
    );

    if args.screenshot {
        let path = save_screenshot(&emulator, &args.rom, args.screenshot_path.as_deref())?;
        println!("Screenshot saved to: {}", path.display());
    }

    Ok(())
}
