//! Measures how fast the emulator core runs games, and fingerprints what they
//! produce so optimizations can be checked to change nothing.
//!
//! Every ROM runs on each chosen model for a fixed stretch of emulated time
//! with a scripted sequence of button presses, without a window or sound. The
//! speed is emulated time divided by the time spent inside the emulator; the
//! target is at least 20 times real time on both DMG and CGB. The
//! fingerprints are hashes of every frame (with its border on a Super Game
//! Boy) and the audio buffer it came with, every audio sample and the final
//! save state; `--record` writes them to a file and `--compare` checks a
//! later run against it. Run over the games and all test ROMs, this shows
//! that an optimization changes nothing at all, far beyond what the test
//! ROMs' pass/fail results show. Built with the core's `eager` feature, which
//! runs every M-cycle on its own, it checks the lazy shortcuts against
//! stepping one M-cycle at a time.
//!
//! `--check-save-states` also checks that a save state taken halfway through
//! each run, loaded into a new emulator, goes on exactly like the run it was
//! taken from. `--shard` runs one of several equal parts of the ROMs, so the
//! nightly CI can split these checks over several machines; `--compare` then
//! needs a file recorded with the same `--shard`.

use clap::Parser;
use image::RgbaImage;
use nubo_core::{Emulator, Key, Model, NoSaves, RtcClock};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The sample rate the emulator makes its sound at, which sets how much
/// emulated time an audio buffer holds.
const SAMPLE_RATE: u32 = 44100;
/// The audio buffer the core fills on each `step_until_next_audio_buffer`.
const SAMPLES_PER_BUFFER: u32 = 512;
/// The Game Boy's frame rate.
const FRAMES_PER_SECOND: f64 = 4_194_304.0 / 70224.0;
const SCREEN_WIDTH: u32 = 160;
const SCREEN_HEIGHT: u32 = 144;

/// Benchmark the emulator core on a set of games
#[derive(Parser)]
#[command(name = "nubo-benchmark")]
struct Args {
    /// ROM files (.gb, .gbc or .zip) or folders to search for them
    #[arg(required = true)]
    paths: Vec<PathBuf>,

    /// Seconds of emulated time to run each game for
    #[arg(long, default_value_t = 30.0, value_parser = parse_seconds)]
    seconds: f64,

    /// Hardware models to run every game on, separated by commas
    #[arg(
        long,
        value_name = "MODEL,...",
        value_delimiter = ',',
        default_value = "dmg,cgb"
    )]
    models: Vec<Model>,

    /// Write the fingerprints of this run to a file
    #[arg(long, value_name = "FILE")]
    record: Option<PathBuf>,

    /// Check the fingerprints of this run against a file written with --record
    #[arg(long, value_name = "FILE")]
    compare: Option<PathBuf>,

    /// How many games to run at the same time. More than one makes the speeds
    /// less reliable, but checking fingerprints faster
    #[arg(short, long, value_name = "N", default_value_t = 1)]
    jobs: usize,

    /// Save each game's last frame as a PNG in this folder, named after the
    /// ROM's path and the model
    #[arg(long, value_name = "FOLDER")]
    screenshots: Option<PathBuf>,

    /// Halfway through each run, save a state, load it into a new emulator
    /// and check that both go on to make exactly the same frames, sound and
    /// state
    #[arg(long)]
    check_save_states: bool,

    /// Run only one of several equal parts of the ROMs, such as 2/4 for the
    /// second of four, so several machines can share a long check
    #[arg(long, value_name = "PART", value_parser = parse_shard)]
    shard: Option<Shard>,
}

/// One of several equal parts of the ROMs, chosen with `--shard`: every
/// `count`th ROM, starting with ROM number `number` (counted from 1). Taking
/// every `count`th ROM spreads each folder of test ROMs over all the parts, so
/// they take about as long as each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Shard {
    number: usize,
    count: usize,
}

impl Shard {
    /// Whether the ROM at `position` in the list of all ROMs (counted from 0)
    /// is in this part.
    fn contains(self, position: usize) -> bool {
        position % self.count == self.number - 1
    }
}

/// Reads the number of seconds to run each game for, which must be more than
/// 0 and finite: 0 would run nothing and infinity would run forever.
fn parse_seconds(text: &str) -> Result<f64, String> {
    text.parse::<f64>()
        .ok()
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
        .ok_or_else(|| String::from("expected a number of seconds, more than 0"))
}

/// Reads a part such as `2/4`.
fn parse_shard(text: &str) -> Result<Shard, String> {
    let invalid = || format!("expected a part such as 2/4, not {text}");
    let (number, count) = text.split_once('/').ok_or_else(invalid)?;
    let number: usize = number.trim().parse().map_err(|_| invalid())?;
    let count: usize = count.trim().parse().map_err(|_| invalid())?;
    if number == 0 || number > count {
        return Err(format!(
            "{text} is not a part: the first number must be from 1 to the second"
        ));
    }
    Ok(Shard { number, count })
}

/// One game on one model.
struct Run {
    rom: PathBuf,
    model: Model,
}

/// What a run measured and produced.
struct Outcome {
    /// How much emulated time the run covered.
    emulated: Duration,
    /// Time spent inside the emulator's steps, which the speed is measured
    /// against, not the run's whole wall time.
    spent: Duration,
    /// How many frames the emulator handed over.
    frames: u64,
    fingerprints: Fingerprints,
    /// With `--check-save-states`: how a save state taken halfway went on,
    /// `Err` saying how it first differed from the run it came from.
    save_state_check: Option<Result<(), String>>,
}

/// Hashes of everything a run produced.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Fingerprints {
    frames: u64,
    audio: u64,
    state: u64,
}

impl Fingerprints {
    /// The three fingerprints in hexadecimal, separated by spaces, as a
    /// `--record` file stores them after a run's key and a tab.
    fn to_line(self) -> String {
        format!(
            "{:016x} {:016x} {:016x}",
            self.frames, self.audio, self.state
        )
    }

    /// Reads fingerprints written by [`Fingerprints::to_line`], or `None`
    /// when the text isn't three hexadecimal numbers.
    fn from_line(line: &str) -> Option<Self> {
        let mut parts = line
            .split_whitespace()
            .map(|part| u64::from_str_radix(part, 16));
        Some(Fingerprints {
            frames: parts.next()?.ok()?,
            audio: parts.next()?.ok()?,
            state: parts.next()?.ok()?,
        })
    }
}

/// A fast, non-cryptographic 64-bit hash (the mixing step of FxHash), good
/// enough to notice any change in a run's output.
#[derive(Default)]
struct Hasher {
    state: u64,
}

impl Hasher {
    /// Mixes one 64-bit word into the hash.
    fn add_word(&mut self, word: u64) {
        self.state = (self.state.rotate_left(5) ^ word).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    /// Mixes `bytes` into the hash, eight at a time (little-endian), then the
    /// bytes left over one by one.
    fn add_bytes(&mut self, bytes: &[u8]) {
        let (chunks, remainder) = bytes.as_chunks::<8>();
        for &chunk in chunks {
            self.add_word(u64::from_le_bytes(chunk));
        }
        for &byte in remainder {
            self.add_word(u64::from(byte));
        }
    }

    /// Mixes audio samples into the hash by their exact bits.
    fn add_samples(&mut self, samples: &[f32]) {
        for sample in samples {
            self.add_word(u64::from(sample.to_bits()));
        }
    }
}

/// Cartridge hooks that start every game without save data and count the
/// real-time clock in emulated time, so every run is the same.
const NO_SAVES: NoSaves = NoSaves {
    rtc_clock: RtcClock::EmulatedTime,
};

/// The buttons the script holds at `seconds` of emulated time: Start a few
/// times to get past title screens and menus, then running right while
/// jumping, so games get to their busier screens.
fn scripted_keys(seconds: f64) -> Vec<Key> {
    let mut keys = Vec::new();
    if seconds < 10.0 {
        if seconds >= 4.0 && seconds % 2.0 < 0.15 {
            keys.push(Key::Start);
        }
    } else {
        keys.push(Key::Right);
        if seconds % 1.0 < 0.4 {
            keys.push(Key::A);
        }
        if seconds % 3.0 < 1.5 {
            keys.push(Key::B);
        }
    }
    keys
}

/// Every Game Boy button, to release the ones the script doesn't hold.
const ALL_KEYS: [Key; 8] = [
    Key::Down,
    Key::Up,
    Key::Left,
    Key::Right,
    Key::Start,
    Key::Select,
    Key::A,
    Key::B,
];

/// A new emulator for `job` with `rom` inserted.
fn start(job: &Run, rom: &[u8]) -> Result<Emulator, String> {
    let mut emulator = Emulator::new(job.model);
    emulator.set_sample_rate(SAMPLE_RATE);
    emulator
        .load_rom(rom, Box::new(NO_SAVES))
        .map_err(|error| format!("Failed to load {}: {error}", job.rom.display()))?;
    Ok(emulator)
}

/// Presses `keys` and releases every other button.
fn hold_keys(emulator: &mut Emulator, keys: &[Key]) {
    for key in ALL_KEYS {
        if keys.contains(&key) {
            emulator.press_key(key);
        } else {
            emulator.release_key(key);
        }
    }
}

/// A copy of a run, made by loading a save state of it into a new emulator,
/// that runs alongside it to the end, noting when its sound and its picture
/// first differ.
struct LoadedCopy {
    emulator: Emulator,
    /// Audio buffers run since the state was loaded.
    buffers: u64,
    sound_differs_after: Option<u64>,
    picture_differs_after: Option<u64>,
}

impl LoadedCopy {
    /// Saves `original`'s state and loads it into a new emulator for `job`,
    /// holding the same keys: the state keeps the buttons the game saw, but
    /// the keys held belong to the player rather than the console, and take
    /// over from the state's buttons when the copy runs.
    fn load(job: &Run, rom: &[u8], original: &Emulator, held: &[Key]) -> Result<Self, String> {
        let mut emulator = start(job, rom)?;
        emulator
            .load_state(&original.save_state())
            .map_err(|error| format!("loading it failed: {error}"))?;
        hold_keys(&mut emulator, held);
        Ok(LoadedCopy {
            emulator,
            buffers: 0,
            sound_differs_after: None,
            picture_differs_after: None,
        })
    }

    /// Runs the copy for the audio buffer the original just made, and notes
    /// whether it made the same samples and frame.
    fn follow(&mut self, left: &[f32], right: &[f32], frame: Option<&[u8]>) {
        let (copy_left, copy_right) = self.emulator.step_until_next_audio_buffer();
        if !(bit_identical(left, copy_left) && bit_identical(right, copy_right)) {
            self.sound_differs_after.get_or_insert(self.buffers);
        }
        if frame != self.emulator.take_frame() {
            self.picture_differs_after.get_or_insert(self.buffers);
        }
        self.buffers += 1;
    }

    /// How the copy went compared with the original, whose state at the end
    /// is `final_state`.
    fn verdict(&self, final_state: &[u8]) -> Result<(), String> {
        let mut differences = Vec::new();
        if let Some(buffers) = self.picture_differs_after {
            differences.push(format!("picture after {buffers} buffers"));
        }
        if let Some(buffers) = self.sound_differs_after {
            differences.push(format!("sound after {buffers} buffers"));
        }
        if self.emulator.save_state() != final_state {
            differences.push(String::from("state at the end"));
        }
        if differences.is_empty() {
            Ok(())
        } else {
            Err(format!("differs: {}", differences.join(", ")))
        }
    }
}

/// Whether two runs of samples are the same down to the last bit.
fn bit_identical(first: &[f32], second: &[f32]) -> bool {
    first.len() == second.len()
        && first
            .iter()
            .zip(second)
            .all(|(first, second)| first.to_bits() == second.to_bits())
}

/// Runs one game on one model for `seconds` of emulated time.
fn run(
    job: &Run,
    seconds: f64,
    screenshots: Option<&Path>,
    check_save_states: bool,
) -> Result<Outcome, String> {
    let rom = fs::read(&job.rom)
        .map_err(|error| format!("Failed to read {}: {error}", job.rom.display()))?;
    let mut emulator = start(job, &rom)?;

    let buffer_seconds = f64::from(SAMPLES_PER_BUFFER) / f64::from(SAMPLE_RATE);
    let buffers = (seconds / buffer_seconds).ceil() as u64;
    let mut frame_hasher = Hasher::default();
    let mut audio_hasher = Hasher::default();
    let mut frames = 0;
    let mut spent = Duration::ZERO;
    let mut held: Vec<Key> = Vec::new();
    let mut copy: Option<LoadedCopy> = None;
    let mut save_state_check = None;

    for buffer in 0..buffers {
        let keys = scripted_keys(buffer as f64 * buffer_seconds);
        if keys != held {
            hold_keys(&mut emulator, &keys);
            if let Some(copy) = &mut copy {
                hold_keys(&mut copy.emulator, &keys);
            }
            held = keys;
        }

        let started = Instant::now();
        let (left, right) = emulator.step_until_next_audio_buffer();
        spent += started.elapsed();
        audio_hasher.add_samples(left);
        audio_hasher.add_samples(right);
        let samples = copy.is_some().then(|| (left.to_vec(), right.to_vec()));
        let new_frame = emulator.take_frame().is_some();
        if new_frame {
            // Frontends pace themselves by the buffer a frame comes with, and
            // save states leave out which frame is waiting, so a frame handed
            // over one buffer later must change the fingerprint too.
            frame_hasher.add_word(buffer);
            frame_hasher.add_bytes(emulator.frame_buffer());
            if let Some(bordered) = emulator.frame_with_border() {
                frame_hasher.add_bytes(bordered);
            }
            frames += 1;
        }
        let frame = new_frame.then(|| emulator.frame_buffer());

        if let (Some(loaded), Some((left, right))) = (&mut copy, &samples) {
            loaded.follow(left, right, frame);
        }
        if check_save_states && buffer == buffers / 2 {
            match LoadedCopy::load(job, &rom, &emulator, &held) {
                Ok(loaded) => copy = Some(loaded),
                Err(error) => save_state_check = Some(Err(error)),
            }
        }
    }

    let final_state = emulator.save_state();
    if let Some(loaded) = copy {
        save_state_check = Some(loaded.verdict(&final_state));
    }
    let mut state_hasher = Hasher::default();
    state_hasher.add_bytes(&final_state);

    if let Some(folder) = screenshots {
        save_screenshot(emulator.frame_buffer(), folder, &run_name(job))?;
    }

    Ok(Outcome {
        emulated: Duration::from_secs_f64(buffers as f64 * buffer_seconds),
        spent,
        frames,
        fingerprints: Fingerprints {
            frames: frame_hasher.state,
            audio: audio_hasher.state,
            state: state_hasher.state,
        },
        save_state_check,
    })
}

/// Saves `frame` (the Game Boy screen, in RGBA) as a PNG in `folder`, named
/// after `name` with slashes, backslashes, colons and spaces turned into
/// underscores.
fn save_screenshot(frame: &[u8], folder: &Path, name: &str) -> Result<(), String> {
    fs::create_dir_all(folder)
        .map_err(|error| format!("Failed to create {}: {error}", folder.display()))?;
    let image = RgbaImage::from_raw(SCREEN_WIDTH, SCREEN_HEIGHT, frame.to_vec())
        .ok_or("The frame has the wrong size")?;
    let path = folder.join(format!("{}.png", name.replace(['/', '\\', ':', ' '], "_")));
    image
        .save(&path)
        .map_err(|error| format!("Failed to save {}: {error}", path.display()))
}

/// The name a run's screenshot is saved under: the ROM's path and the model.
/// The whole path, since ROMs in different folders can share a file name.
fn run_name(job: &Run) -> String {
    format!("{} [{}]", job.rom.display(), job.model)
}

/// The key a run's fingerprints are recorded under: the ROM's path, the model and the length of
/// the run.
fn fingerprint_key(job: &Run, seconds: f64) -> String {
    format!("{} [{}] {seconds}s", job.rom.display(), job.model)
}

/// The ROMs in `paths`, searching folders and their subfolders.
fn find_roms(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut roms = Vec::new();
    for path in paths {
        if path.is_dir() {
            let mut entries: Vec<PathBuf> = fs::read_dir(path)
                .map_err(|error| format!("Failed to read {}: {error}", path.display()))?
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .collect();
            entries.sort();
            let nested: Vec<PathBuf> = entries
                .into_iter()
                .filter(|entry| entry.is_dir() || is_rom(entry))
                .collect();
            roms.extend(find_roms(&nested)?);
        } else {
            roms.push(path.clone());
        }
    }
    Ok(roms)
}

/// Whether `path` names a ROM the benchmark can run: a `.gb`, `.gbc` or
/// `.zip` file, whatever its case.
fn is_rom(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["gb", "gbc", "zip"]
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

/// Reads a fingerprint file written with `--record`.
fn read_fingerprints(path: &Path) -> Result<BTreeMap<String, Fingerprints>, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (name, fingerprints) = line
                .split_once('\t')
                .ok_or_else(|| format!("Malformed line in {}: {line}", path.display()))?;
            let fingerprints = Fingerprints::from_line(fingerprints)
                .ok_or_else(|| format!("Malformed line in {}: {line}", path.display()))?;
            Ok((name.to_owned(), fingerprints))
        })
        .collect()
}

fn main() -> ExitCode {
    match benchmark(&Args::parse()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Runs every game on every model, prints the speeds and records or compares
/// the fingerprints. Returns whether everything checked out: every game ran,
/// every fingerprint was recorded and matched, nothing recorded was left
/// out, and every save state went on like its run.
fn benchmark(args: &Args) -> Result<bool, String> {
    let expected = args.compare.as_deref().map(read_fingerprints).transpose()?;
    let jobs: Vec<Run> = find_roms(&args.paths)?
        .into_iter()
        .enumerate()
        .filter(|&(position, _)| args.shard.is_none_or(|shard| shard.contains(position)))
        .flat_map(|(_, rom)| {
            args.models.iter().map(move |&model| Run {
                rom: rom.clone(),
                model,
            })
        })
        .collect();

    let next_job = Mutex::new(0);
    let outcomes: Mutex<Vec<Option<Result<Outcome, String>>>> =
        Mutex::new(jobs.iter().map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..args.jobs.max(1) {
            scope.spawn(|| {
                loop {
                    let index = {
                        let mut next = next_job.lock().expect("no worker panics");
                        let index = *next;
                        *next += 1;
                        index
                    };
                    let Some(job) = jobs.get(index) else { break };
                    let outcome = run(
                        job,
                        args.seconds,
                        args.screenshots.as_deref(),
                        args.check_save_states,
                    );
                    outcomes.lock().expect("no worker panics")[index] = Some(outcome);
                }
            });
        }
    });

    let mut all_good = true;
    let mut record = String::new();
    let mut seen = BTreeSet::new();
    let mut slowest: BTreeMap<String, f64> = BTreeMap::new();
    println!(
        "{:<60} {:>8} {:>8}  fingerprints (frames, audio, state)",
        "game", "speed", "fps"
    );
    for (job, outcome) in jobs
        .iter()
        .zip(outcomes.into_inner().expect("no worker panics"))
    {
        let name = fingerprint_key(job, args.seconds);
        let short_name: String = job.rom.file_stem().map_or_else(String::new, |stem| {
            stem.to_string_lossy().chars().take(45).collect()
        });
        let label = format!("{short_name} [{}]", job.model);
        seen.insert(name.clone());
        let outcome = match outcome.expect("every job ran") {
            Ok(outcome) => outcome,
            Err(error) => {
                all_good = false;
                println!("{label:<60} FAILED: {error}");
                continue;
            }
        };
        let speed = outcome.emulated.as_secs_f64() / outcome.spent.as_secs_f64();
        let fps = outcome.frames as f64 / outcome.spent.as_secs_f64();
        let line = outcome.fingerprints.to_line();
        let mut verdict = match expected.as_ref().map(|expected| expected.get(&name)) {
            None => String::new(),
            Some(None) => {
                all_good = false;
                "  NOT RECORDED".to_owned()
            }
            Some(Some(recorded)) if *recorded == outcome.fingerprints => "  same".to_owned(),
            Some(Some(recorded)) => {
                all_good = false;
                let mut differences = Vec::new();
                if recorded.frames != outcome.fingerprints.frames {
                    differences.push("frames");
                }
                if recorded.audio != outcome.fingerprints.audio {
                    differences.push("audio");
                }
                if recorded.state != outcome.fingerprints.state {
                    differences.push("state");
                }
                format!("  DIFFERENT {}", differences.join(", "))
            }
        };
        match &outcome.save_state_check {
            None => {}
            Some(Ok(())) => verdict.push_str("  save state ok"),
            Some(Err(difference)) => {
                all_good = false;
                let _ = write!(verdict, "  SAVE STATE: {difference}");
            }
        }
        println!("{label:<60} {speed:>7.1}x {fps:>8.0}  {line}{verdict}");
        let _ = writeln!(record, "{name}\t{line}");
        let model = job.model.to_string();
        let slowest_speed = slowest.entry(model).or_insert(f64::INFINITY);
        *slowest_speed = slowest_speed.min(speed);
    }
    for (model, speed) in &slowest {
        println!(
            "slowest on {model}: {speed:.1}x ({:.0} fps)",
            speed * FRAMES_PER_SECOND
        );
    }

    if let Some(path) = &args.record {
        fs::write(path, record)
            .map_err(|error| format!("Failed to write {}: {error}", path.display()))?;
    }
    if let Some(expected) = &expected {
        for missing in expected.keys().filter(|key| !seen.contains(*key)) {
            all_good = false;
            println!("MISSING from this run: {missing}");
        }
    }
    if expected.is_some() || args.check_save_states {
        println!(
            "{}",
            if all_good {
                "Everything matches"
            } else {
                "Some runs failed or differ"
            }
        );
    }
    Ok(all_good)
}

#[cfg(test)]
mod tests;
