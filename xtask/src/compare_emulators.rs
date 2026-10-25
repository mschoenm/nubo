//! `cargo xtask compare-emulators`: measures how fast Nubo runs games next to
//! other Game Boy emulators: SameBoy, Gambatte, mGBA, binjgb and Gearboy.
//!
//! Each of them is fetched with git at a pinned commit into
//! `target/emulator-comparison` and built with the compiler flags of its own
//! release builds, together with a small harness from
//! `xtask/emulator_harnesses` that runs a ROM the way nubo-benchmark runs it
//! in Nubo (see `harness.h`): 44,100 Hz audio in buffers of 512 samples, the
//! same scripted buttons, every frame drawn, and only the time spent inside
//! the emulator counted. Nubo runs in nubo-benchmark itself, and also built
//! with the `eager` feature (every M-cycle on its own) to show what its lazy
//! shortcuts add. The other emulators are only built and run on this machine,
//! never distributed with Nubo or ported from (Gambatte is GPL-2.0-only).
//!
//! The runs are timed one at a time, round after round, and the table shows
//! each one's best round, since a busy machine only ever slows a run down.
//! With `--instructions`, Valgrind's callgrind counts the instructions each
//! emulator runs inside its per-buffer function instead: much slower, but the
//! same however busy the machine is.
//!
//! Unlike everything else in the workspace, this needs more than Rust: git, a
//! C and C++ compiler, make, RGBDS (for SameBoy's free boot ROMs, which
//! Gambatte and Gearboy run too) and CMake (for mGBA). An emulator whose tools
//! are missing is left out with a note saying what to install. It runs on
//! Linux and macOS; `--instructions` needs Valgrind, which runs on Linux.

use crate::{Result, benchmark_roms, build_commit, on_path, target_dir};
use clap::builder::PossibleValue;
use clap::{Args, ValueEnum};
use image::RgbImage;
use std::collections::BTreeMap;
use std::fmt::Write;
use std::fs;
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::thread;

const SAMPLE_RATE: f64 = 44_100.0;
const SAMPLES_PER_BUFFER: f64 = 512.0;
const DEFAULT_SECONDS: f64 = 30.0;
/// Valgrind runs programs about 50 times slower, so instruction counts use
/// shorter runs by default.
const DEFAULT_INSTRUCTION_SECONDS: f64 = 10.0;
const DEFAULT_ROUNDS: u32 = 3;
const SCREEN_WIDTH: u32 = 160;
const SCREEN_HEIGHT: u32 = 144;

/// An emulator the comparison can run.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Emulator {
    Nubo,
    /// Nubo built with the `eager` feature, which runs every M-cycle on its
    /// own instead of letting the PPU and APU catch up later.
    NuboEager,
    SameBoy,
    Gambatte,
    Mgba,
    Binjgb,
    Gearboy,
}

const EMULATORS: [Emulator; 7] = [
    Emulator::Nubo,
    Emulator::NuboEager,
    Emulator::SameBoy,
    Emulator::Gambatte,
    Emulator::Mgba,
    Emulator::Binjgb,
    Emulator::Gearboy,
];

/// A git repository at the commit the comparison builds.
#[derive(Clone, Copy)]
struct Source {
    repository: &'static str,
    commit: &'static str,
}

impl Emulator {
    /// The name `--emulators` takes and files are named after.
    fn id(self) -> &'static str {
        match self {
            Emulator::Nubo => "nubo",
            Emulator::NuboEager => "nubo-eager",
            Emulator::SameBoy => "sameboy",
            Emulator::Gambatte => "gambatte",
            Emulator::Mgba => "mgba",
            Emulator::Binjgb => "binjgb",
            Emulator::Gearboy => "gearboy",
        }
    }

    /// The name the table and messages show.
    fn name(self) -> &'static str {
        match self {
            Emulator::Nubo => "Nubo",
            Emulator::NuboEager => "Nubo eager",
            Emulator::SameBoy => "SameBoy",
            Emulator::Gambatte => "Gambatte",
            Emulator::Mgba => "mGBA",
            Emulator::Binjgb => "binjgb",
            Emulator::Gearboy => "Gearboy",
        }
    }

    /// Where the code of an emulator other than Nubo comes from.
    fn source(self) -> Option<Source> {
        let (repository, commit) = match self {
            Emulator::Nubo | Emulator::NuboEager => return None,
            Emulator::SameBoy => (
                "https://github.com/LIJI32/SameBoy",
                "c4cf7abab79a4e22dc17e0704995976cb2751b2d",
            ),
            Emulator::Gambatte => (
                "https://github.com/pokemon-speedrunning/gambatte-core",
                "5a41a68c25402421fb1983ddadc9faf2418ddb0f",
            ),
            Emulator::Mgba => (
                "https://github.com/mgba-emu/mgba",
                "143adf5aa2a60c6036810b0cd72445e13d74b0e6",
            ),
            Emulator::Binjgb => (
                "https://github.com/binji/binjgb",
                "16621111ed0ee73bcc45c912a823bcebedcffc0f",
            ),
            Emulator::Gearboy => (
                "https://github.com/drhelius/Gearboy",
                "a1fceab64438be1668d1151de3726a4515033900",
            ),
        };
        Some(Source { repository, commit })
    }

    /// The harness's source file in `xtask/emulator_harnesses`.
    fn harness(self) -> &'static str {
        match self {
            Emulator::Nubo | Emulator::NuboEager => "",
            Emulator::SameBoy => "sameboy.c",
            Emulator::Gambatte => "gambatte.cpp",
            Emulator::Mgba => "mgba.c",
            Emulator::Binjgb => "binjgb.c",
            Emulator::Gearboy => "gearboy.cpp",
        }
    }

    /// The programs building it needs besides Rust.
    fn tools(self) -> &'static [&'static str] {
        match self {
            Emulator::Nubo | Emulator::NuboEager => &[],
            Emulator::SameBoy => &["git", "make", "cc", "rgbasm"],
            Emulator::Gambatte => &["git", "make", "cc", "c++", "rgbasm"],
            Emulator::Mgba => &["git", "cmake", "make", "cc"],
            Emulator::Binjgb => &["git", "cc"],
            Emulator::Gearboy => &["git", "make", "cc", "c++", "rgbasm"],
        }
    }

    /// Whether it runs SameBoy's boot ROMs (the others either can't run a
    /// boot ROM or, like Nubo, have their own).
    fn runs_sameboy_boot_roms(self) -> bool {
        matches!(
            self,
            Emulator::SameBoy | Emulator::Gambatte | Emulator::Gearboy
        )
    }

    /// The function callgrind counts instructions in: the one that runs the
    /// emulator until its next audio buffer is ready.
    fn counted_function(self) -> &'static str {
        match self {
            Emulator::Nubo | Emulator::NuboEager => "*step_until_next_audio_buffer*",
            _ => "run_buffer*",
        }
    }
}

/// The hardware models runs can be on.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Model {
    /// The original Game Boy (SameBoy's DMG-B, Nubo's DMG).
    Dmg,
    /// The Game Boy Color (CPU CGB E where the emulator tells revisions apart).
    Cgb,
}

impl Model {
    /// The model's name on the harnesses' and the benchmark's command lines
    /// and in screenshot file names.
    fn id(self) -> &'static str {
        match self {
            Model::Dmg => "dmg",
            Model::Cgb => "cgb",
        }
    }

    /// The name the table and progress lines show.
    fn label(self) -> &'static str {
        match self {
            Model::Dmg => "DMG",
            Model::Cgb => "CGB",
        }
    }
}

impl ValueEnum for Emulator {
    fn value_variants<'a>() -> &'a [Self] {
        &EMULATORS
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(PossibleValue::new(self.id()))
    }
}

impl ValueEnum for Model {
    fn value_variants<'a>() -> &'a [Self] {
        &[Model::Dmg, Model::Cgb]
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(PossibleValue::new(self.id()))
    }
}

/// The command line of `cargo xtask compare-emulators`, as clap reads it.
/// The doc comments on its fields are the help that `--help` prints.
#[derive(Args, Debug)]
pub struct Arguments {
    /// ROM files (.gb or .gbc) or folders to search for them; the benchmark games when none
    /// are given
    roms: Vec<PathBuf>,
    /// Seconds of emulated time to run each game for [default: 30, or 10 with
    /// --instructions]
    #[arg(long, value_name = "SECONDS", value_parser = parse_seconds)]
    seconds: Option<f64>,
    /// How many times to time each run; the table shows the fastest
    #[arg(long, value_name = "N", default_value_t = DEFAULT_ROUNDS, value_parser = clap::value_parser!(u32).range(1..))]
    rounds: u32,
    /// The hardware models to run every game on, separated by commas
    #[arg(
        long,
        value_name = "MODEL,...",
        value_delimiter = ',',
        default_value = "dmg,cgb"
    )]
    models: Vec<Model>,
    /// The emulators to run, separated by commas [default: all of them]
    #[arg(long, value_name = "EMULATOR,...", value_delimiter = ',')]
    emulators: Vec<Emulator>,
    /// Count the instructions each emulator runs with Valgrind instead of timing it (Linux)
    #[arg(long)]
    instructions: bool,
    /// Also keep each run's last frame, to check every emulator got as far
    #[arg(long)]
    screenshots: bool,
}

/// Reads the value of `--seconds`: a positive number.
fn parse_seconds(text: &str) -> Result<f64> {
    text.parse::<f64>()
        .ok()
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
        .ok_or_else(|| String::from("expected a positive number"))
}

/// What to compare, from the command line.
struct Options {
    /// ROM files and folders; the benchmark games when empty.
    roms: Vec<PathBuf>,
    seconds: f64,
    rounds: u32,
    /// The models to run on, in the order of `Model`, each once.
    models: Vec<Model>,
    /// The emulators to run, in the order of `EMULATORS`, each once.
    emulators: Vec<Emulator>,
    instructions: bool,
    screenshots: bool,
}

impl From<Arguments> for Options {
    fn from(arguments: Arguments) -> Self {
        let emulators = if arguments.emulators.is_empty() {
            EMULATORS.to_vec()
        } else {
            arguments.emulators
        };
        Options {
            roms: arguments.roms,
            seconds: arguments.seconds.unwrap_or(if arguments.instructions {
                DEFAULT_INSTRUCTION_SECONDS
            } else {
                DEFAULT_SECONDS
            }),
            rounds: arguments.rounds,
            models: sorted_once(arguments.models),
            emulators: sorted_once(emulators),
            instructions: arguments.instructions,
            screenshots: arguments.screenshots,
        }
    }
}

/// `items` in their own order, each once.
fn sorted_once<T: Ord>(mut items: Vec<T>) -> Vec<T> {
    items.sort();
    items.dedup();
    items
}

/// Builds the chosen emulators, runs them on the chosen ROMs and prints the
/// table.
pub fn run_comparison(arguments: Arguments) -> Result {
    if cfg!(windows) {
        return Err(String::from(
            "compare-emulators runs on Linux and macOS (on Windows, run it in WSL)",
        ));
    }
    let options = Options::from(arguments);
    if options.instructions && !on_path("valgrind") {
        return Err(String::from(
            "--instructions counts instructions with Valgrind, which wasn't found (it runs on Linux)",
        ));
    }
    let roms = find_roms(&options.roms)?;
    if roms.is_empty() {
        return Err(String::from("no .gb or .gbc files found"));
    }
    let root = target_dir().join("emulator-comparison");

    let (ready, left_out) = build::prepare(&options.emulators, &root);
    for (emulator, reason) in &left_out {
        eprintln!("Leaving out {}: {reason}", emulator.name());
    }
    if ready.is_empty() {
        return Err(String::from("none of the emulators could be built"));
    }

    let mut runs = Vec::new();
    for rom in 0..roms.len() {
        for &model in &options.models {
            for &emulator in &ready {
                runs.push(Run {
                    emulator,
                    rom,
                    model,
                });
            }
        }
    }
    let labels = rom_labels(&roms);
    let context = RunContext {
        roms: &roms,
        labels: &labels,
        root: &root,
        seconds: options.seconds,
        instructions: options.instructions,
    };
    let results = if options.instructions {
        count_instructions(&runs, &context, options.screenshots)
    } else {
        time_runs(&runs, &context, options.rounds, options.screenshots)
    };

    println!();
    print_versions(&ready);
    println!();
    if options.instructions {
        println!(
            "Millions of instructions per emulated second, counted inside each emulator over {} s \
             (lower is better):",
            options.seconds
        );
    } else {
        let rounds = if options.rounds == 1 {
            String::from("one round")
        } else {
            format!("best of {} rounds", options.rounds)
        };
        println!(
            "Emulated seconds per second spent inside each emulator, {rounds} of {} s \
             (higher is faster):",
            options.seconds
        );
    }
    print!(
        "{}",
        table(
            &labels,
            &options.models,
            &ready,
            &results,
            options.instructions
        )
    );
    for ((rom, model, emulator), outcome) in &results.outcomes {
        if let Err(error) = outcome {
            println!(
                "{} on {} {}: {error}",
                emulator.name(),
                labels[*rom],
                model.label()
            );
        }
    }
    if options.screenshots {
        println!(
            "The last frame of each run is in {}",
            root.join("screenshots").display()
        );
    }
    Ok(())
}

/// The ROMs to run: the files given, the `.gb` and `.gbc` files in the
/// folders given, or the benchmark games.
fn find_roms(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let paths = if paths.is_empty() {
        vec![benchmark_roms::ensure_benchmark_roms()?]
    } else {
        paths.to_vec()
    };
    let mut roms = Vec::new();
    for path in &paths {
        if path.is_dir() {
            collect_roms(path, &mut roms)?;
        } else if path.is_file() {
            roms.push(path.clone());
        } else {
            return Err(format!("{} doesn't exist", path.display()));
        }
    }
    Ok(roms)
}

/// Adds the Game Boy ROMs (`.gb` and `.gbc`) in `folder` and its subfolders
/// to `roms`, sorted by path.
fn collect_roms(folder: &Path, roms: &mut Vec<PathBuf>) -> Result {
    for path in sorted_entries(folder)? {
        if path.is_dir() {
            collect_roms(&path, roms)?;
        } else if has_extension(&path, &["gb", "gbc"]) {
            roms.push(path);
        }
    }
    Ok(())
}

/// The paths of what `folder` holds, sorted, so every run sees the same
/// order.
fn sorted_entries(folder: &Path) -> Result<Vec<PathBuf>> {
    let mut entries: Vec<PathBuf> = fs::read_dir(folder)
        .map_err(|error| format!("failed to read {}: {error}", folder.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect();
    entries.sort();
    Ok(entries)
}

/// Whether `path` ends in one of `extensions`, ignoring case.
fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extensions
                .iter()
                .any(|wanted| extension.eq_ignore_ascii_case(wanted))
        })
}

/// The ROM's file name without its extension.
fn rom_name(rom: &Path) -> String {
    rom.file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned())
}

/// What the table and progress lines call each ROM: its name, or its path
/// when ROMs from different folders share the name.
fn rom_labels(roms: &[PathBuf]) -> Vec<String> {
    let names: Vec<String> = roms.iter().map(|rom| rom_name(rom)).collect();
    roms.iter()
        .zip(&names)
        .map(|(rom, name)| {
            if names.iter().filter(|other| *other == name).count() > 1 {
                rom.display().to_string()
            } else {
                name.clone()
            }
        })
        .collect()
}

/// Where `emulator`'s source is fetched to, under `root`.
fn source_folder(root: &Path, emulator: Emulator) -> PathBuf {
    root.join(emulator.id())
}

/// Where `emulator`'s harness is built to, in `root`'s `bin/`.
fn executable(root: &Path, emulator: Emulator) -> PathBuf {
    root.join("bin").join(emulator.id())
}

/// Where SameBoy's build puts the boot ROMs (`build_boot_roms`).
fn boot_rom_folder(root: &Path) -> PathBuf {
    source_folder(root, Emulator::SameBoy).join("build/bin/BootROMs")
}

/// How many jobs to run at once: the number of cores, or 1 when unknown.
fn parallelism() -> usize {
    thread::available_parallelism().map_or(1, NonZero::get)
}

/// Runs `work` on every item, as many at a time as the machine has cores,
/// and returns the results in no particular order.
fn in_parallel<T: Send, R: Send>(items: Vec<T>, work: impl Fn(T) -> R + Sync) -> Vec<R> {
    let queue = Mutex::new(items.into_iter());
    let results = Mutex::new(Vec::new());
    thread::scope(|scope| {
        for _ in 0..parallelism() {
            scope.spawn(|| {
                loop {
                    let next = queue
                        .lock()
                        .expect("no worker panics holding the queue")
                        .next();
                    let Some(item) = next else { break };
                    let result = work(item);
                    results
                        .lock()
                        .expect("no worker panics holding the results")
                        .push(result);
                }
            });
        }
    });
    results
        .into_inner()
        .expect("no worker panics holding the results")
}

/// One emulator running one ROM on one model.
#[derive(Clone, Copy)]
struct Run {
    emulator: Emulator,
    /// The index of the ROM in the list of ROMs.
    rom: usize,
    model: Model,
}

/// What every run shares.
struct RunContext<'a> {
    roms: &'a [PathBuf],
    /// What to call each ROM, from [`rom_labels`].
    labels: &'a [String],
    root: &'a Path,
    seconds: f64,
    instructions: bool,
}

/// What one run measured.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Measurement {
    /// Emulated seconds per second spent inside the emulator.
    speed: f64,
    /// Seconds of emulated time the run covered.
    emulated: f64,
    /// The instructions callgrind counted, with `--instructions`.
    instructions: Option<u64>,
}

/// The outcome of each emulator on each ROM and model: every round's
/// measurement, or why it failed.
#[derive(Default)]
struct Results {
    outcomes: BTreeMap<(usize, Model, Emulator), Result<Vec<Measurement>>>,
}

impl Results {
    /// Records one round of `run`. An error wins over measurements: once a
    /// round failed, the run counts as failed and later rounds are ignored.
    fn add(&mut self, run: Run, measurement: Result<Measurement>) {
        let outcome = self
            .outcomes
            .entry((run.rom, run.model, run.emulator))
            .or_insert_with(|| Ok(Vec::new()));
        match (outcome, measurement) {
            (Ok(measurements), Ok(measurement)) => measurements.push(measurement),
            (outcome, Err(error)) => *outcome = Err(error),
            (Err(_), Ok(_)) => {}
        }
    }

    /// Whether a round of `run` failed, so later rounds can skip it.
    fn failed(&self, run: Run) -> bool {
        matches!(
            self.outcomes.get(&(run.rom, run.model, run.emulator)),
            Some(Err(_))
        )
    }
}

/// Times every run, one at a time, round after round.
fn time_runs(runs: &[Run], context: &RunContext, rounds: u32, screenshots: bool) -> Results {
    let mut results = Results::default();
    for round in 1..=rounds {
        let mut last_group = None;
        for &run in runs {
            if last_group != Some((run.rom, run.model)) {
                println!(
                    "Round {round} of {rounds}: {} on {}",
                    context.labels[run.rom],
                    run.model.label()
                );
                last_group = Some((run.rom, run.model));
            }
            if !results.failed(run) {
                results.add(run, measure(run, context, screenshots && round == 1));
            }
        }
    }
    results
}

/// Counts the instructions of every run under callgrind, as many at a time as
/// the machine has cores, since the counts don't depend on the machine's load.
fn count_instructions(runs: &[Run], context: &RunContext, screenshots: bool) -> Results {
    let outcomes = in_parallel(runs.to_vec(), |run| {
        let measurement = measure(run, context, screenshots);
        println!(
            "Counted {} on {} {}",
            run.emulator.name(),
            context.labels[run.rom],
            run.model.label()
        );
        (run, measurement)
    });
    let mut results = Results::default();
    for (run, measurement) in outcomes {
        results.add(run, measurement);
    }
    results
}

/// Runs one emulator on one ROM and reads what it measured from its output.
fn measure(run: Run, context: &RunContext, screenshot: bool) -> Result<Measurement> {
    let program = executable(context.root, run.emulator);
    let mut command = if context.instructions {
        let mut command = Command::new("valgrind");
        command
            .arg("--tool=callgrind")
            .arg(format!(
                "--toggle-collect={}",
                run.emulator.counted_function()
            ))
            .arg("--callgrind-out-file=/dev/null")
            .arg(&program);
        command
    } else {
        Command::new(&program)
    };
    let rom = &context.roms[run.rom];
    let seconds = context.seconds.to_string();
    // Numbered by the ROM's place in the list, since ROMs from different
    // folders can share a name.
    let screenshot_path = screenshot.then(|| {
        context
            .root
            .join("screenshots")
            .join(run.emulator.id())
            .join(format!(
                "{:03}-{}-{}.png",
                run.rom + 1,
                rom_name(rom),
                run.model.id()
            ))
    });
    let nubo_screenshots = match (&screenshot_path, run.emulator.source()) {
        (Some(path), None) => Some(path.with_extension("tmp")),
        _ => None,
    };
    if run.emulator.source().is_none() {
        command
            .arg(rom)
            .args(["--models", run.model.id(), "--seconds", &seconds]);
        if let Some(folder) = &nubo_screenshots {
            command.arg("--screenshots").arg(folder);
        }
    } else {
        command
            .arg(rom)
            .arg(run.model.id())
            .arg(&seconds)
            .arg(boot_rom_folder(context.root));
        if let Some(path) = &screenshot_path {
            create_parent(path)?;
            command.arg(path.with_extension("ppm"));
        }
    }

    let output = command
        .output()
        .map_err(|error| format!("failed to run {}: {error}", program.display()))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let parsed = if run.emulator.source().is_none() {
        parse_nubo_speed(&stdout).map(|speed| (speed, nubo_emulated_seconds(context.seconds)))
    } else {
        parse_harness_line(&stdout)
    };
    let (speed, emulated) = parsed.ok_or_else(|| {
        stderr
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty() && !line.starts_with("=="))
            .map_or_else(
                || format!("exited with {} without a result", output.status),
                |line| line.trim().to_owned(),
            )
    })?;
    let instructions = if context.instructions {
        Some(parse_collected(&stderr).ok_or("callgrind reported no instruction count")?)
    } else {
        None
    };

    if let Some(path) = &screenshot_path {
        if let Some(folder) = &nubo_screenshots {
            move_only_file(folder, path)?;
        } else {
            ppm_to_png(&path.with_extension("ppm"), path)?;
        }
    }
    Ok(Measurement {
        speed,
        emulated,
        instructions,
    })
}

/// The emulated time nubo-benchmark runs for: whole audio buffers, rounded up
/// like the harnesses do.
fn nubo_emulated_seconds(seconds: f64) -> f64 {
    let buffer_seconds = SAMPLES_PER_BUFFER / SAMPLE_RATE;
    (seconds / buffer_seconds).ceil() * buffer_seconds
}

/// The speed in nubo-benchmark's line for the run, such as
/// `tobudx [DMG]   86.1x   5139  ...`.
fn parse_nubo_speed(stdout: &str) -> Option<f64> {
    stdout.lines().find_map(|line| {
        let (_, rest) = line.split_once("] ")?;
        rest.split_whitespace()
            .next()?
            .strip_suffix('x')?
            .parse()
            .ok()
    })
}

/// The speed and emulated seconds in a harness's line, such as
/// `speed 19.034x  emulated 30.0018 s  spent 1.5762 s  frames 1792`.
fn parse_harness_line(stdout: &str) -> Option<(f64, f64)> {
    stdout.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        if words.next()? != "speed" {
            return None;
        }
        let speed = words.next()?.strip_suffix('x')?.parse().ok()?;
        if words.next()? != "emulated" {
            return None;
        }
        let emulated = words.next()?.parse().ok()?;
        Some((speed, emulated))
    })
}

/// The instruction count in callgrind's summary, such as
/// `==1234== Collected : 578204262`.
fn parse_collected(stderr: &str) -> Option<u64> {
    stderr.lines().find_map(|line| {
        line.split_once("Collected :")
            .and_then(|(_, count)| count.trim().parse().ok())
    })
}

fn create_parent(path: &Path) -> Result {
    let folder = path.parent().unwrap_or(path);
    fs::create_dir_all(folder)
        .map_err(|error| format!("failed to create {}: {error}", folder.display()))
}

/// Moves the one file nubo-benchmark wrote into `folder` to `path`, and
/// removes the folder.
fn move_only_file(folder: &Path, path: &Path) -> Result {
    let file = sorted_entries(folder)?.into_iter().next().ok_or_else(|| {
        format!(
            "nubo-benchmark wrote no screenshot into {}",
            folder.display()
        )
    })?;
    fs::rename(&file, path).map_err(|error| {
        format!(
            "failed to move {} to {}: {error}",
            file.display(),
            path.display()
        )
    })?;
    fs::remove_dir_all(folder)
        .map_err(|error| format!("failed to remove {}: {error}", folder.display()))
}

/// Turns the binary PPM a harness wrote into a PNG.
fn ppm_to_png(ppm: &Path, png: &Path) -> Result {
    let bytes =
        fs::read(ppm).map_err(|error| format!("failed to read {}: {error}", ppm.display()))?;
    let header = format!("P6\n{SCREEN_WIDTH} {SCREEN_HEIGHT}\n255\n");
    let pixels = bytes
        .strip_prefix(header.as_bytes())
        .ok_or_else(|| format!("{} isn't a 160x144 PPM", ppm.display()))?;
    let image = RgbImage::from_raw(SCREEN_WIDTH, SCREEN_HEIGHT, pixels.to_vec())
        .ok_or_else(|| format!("{} is cut short", ppm.display()))?;
    image
        .save(png)
        .map_err(|error| format!("failed to save {}: {error}", png.display()))?;
    fs::remove_file(ppm).map_err(|error| format!("failed to remove {}: {error}", ppm.display()))
}

/// Prints the commit of each emulator compared.
fn print_versions(emulators: &[Emulator]) {
    let versions: Vec<String> = emulators
        .iter()
        .filter(|emulator| **emulator != Emulator::NuboEager)
        .map(|emulator| {
            let commit = emulator
                .source()
                .map_or_else(build_commit, |source| source.commit.to_owned());
            let short = commit.get(..7).unwrap_or(&commit);
            let dirty = if commit.ends_with("-dirty") {
                " with uncommitted changes"
            } else {
                ""
            };
            format!("{} {short}{dirty}", emulator.name())
        })
        .collect();
    println!("Compared {}", versions.join(", "));
}

/// The table of results: a row for each ROM on each model, a column for each
/// emulator.
fn table(
    labels: &[String],
    models: &[Model],
    emulators: &[Emulator],
    results: &Results,
    instructions: bool,
) -> String {
    let mut rows = vec![
        std::iter::once(String::new())
            .chain(emulators.iter().map(|emulator| emulator.name().to_owned()))
            .collect::<Vec<_>>(),
    ];
    for (rom_index, label) in labels.iter().enumerate() {
        for &model in models {
            let mut row = vec![format!("{label} {}", model.label())];
            for &emulator in emulators {
                let cell = match results.outcomes.get(&(rom_index, model, emulator)) {
                    Some(Ok(measurements)) => summarize(measurements, instructions),
                    _ => None,
                };
                row.push(cell.unwrap_or_else(|| String::from("-")));
            }
            rows.push(row);
        }
    }
    let widths: Vec<usize> = (0..rows[0].len())
        .map(|column| rows.iter().map(|row| row[column].len()).max().unwrap_or(0))
        .collect();
    let mut text = String::new();
    for row in &rows {
        let mut line = format!("{:<width$}", row[0], width = widths[0]);
        for (cell, width) in row.iter().zip(&widths).skip(1) {
            let _ = write!(line, "  {cell:>width$}");
        }
        text.push_str(line.trim_end());
        text.push('\n');
    }
    text
}

/// A cell of the table: the best speed of the rounds, or millions of
/// instructions per emulated second.
fn summarize(measurements: &[Measurement], instructions: bool) -> Option<String> {
    if instructions {
        let measurement = measurements.first()?;
        let millions = measurement.instructions? as f64 / measurement.emulated / 1e6;
        Some(format!("{millions:.0}"))
    } else {
        let best = measurements
            .iter()
            .map(|measurement| measurement.speed)
            .reduce(f64::max)?;
        Some(format!("{best:.1}x"))
    }
}

mod build;

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads `text` as the options after `cargo xtask compare-emulators`.
    fn parse(text: &str) -> std::result::Result<Options, clap::Error> {
        /// A command line that is only the options.
        #[derive(clap::Parser)]
        struct Command {
            #[command(flatten)]
            arguments: Arguments,
        }
        let args = std::iter::once("compare-emulators").chain(text.split_whitespace());
        <Command as clap::Parser>::try_parse_from(args).map(|command| command.arguments.into())
    }

    #[test]
    fn options_default_to_every_emulator_on_both_models() {
        let options = parse("").unwrap();
        assert_eq!(options.emulators, EMULATORS.to_vec());
        assert_eq!(options.models, vec![Model::Dmg, Model::Cgb]);
        assert_eq!(options.seconds, DEFAULT_SECONDS);
        assert_eq!(options.rounds, DEFAULT_ROUNDS);
        assert!(options.roms.is_empty());
    }

    #[test]
    fn options_read_lists_numbers_and_paths() {
        let options =
            parse("roms --emulators sameboy,nubo --models cgb --rounds 5 --seconds 2.5 a.gb")
                .unwrap();
        assert_eq!(options.emulators, vec![Emulator::Nubo, Emulator::SameBoy]);
        assert_eq!(options.models, vec![Model::Cgb]);
        assert_eq!(options.rounds, 5);
        assert_eq!(options.seconds, 2.5);
        assert_eq!(
            options.roms,
            vec![PathBuf::from("roms"), PathBuf::from("a.gb")]
        );
    }

    #[test]
    fn instruction_counts_default_to_shorter_runs() {
        let options = parse("--instructions").unwrap();
        assert_eq!(options.seconds, DEFAULT_INSTRUCTION_SECONDS);
        let options = parse("--instructions --seconds 4").unwrap();
        assert_eq!(options.seconds, 4.0);
    }

    #[test]
    fn options_refuse_what_they_dont_know() {
        assert!(parse("--emulators bgb").is_err());
        assert!(parse("--models sgb").is_err());
        assert!(parse("--seconds -1").is_err());
        assert!(parse("--rounds 0").is_err());
        assert!(parse("--seconds").is_err());
        assert!(parse("--fast").is_err());
    }

    #[test]
    fn nubo_benchmark_lines_give_the_speed() {
        let stdout = "game  speed  fps  fingerprints (frames, audio, state)\n\
                      tobudx [DMG]   86.1x   5139  733e585b953c927a 670f9d81221b7d9d 2762402d14fbfa66\n\
                      slowest on DMG: 86.1x (5139 fps)\n";
        assert_eq!(parse_nubo_speed(stdout), Some(86.1));
    }

    #[test]
    fn harness_lines_give_the_speed_and_emulated_time() {
        let stdout =
            "Some log line\nspeed 19.034x  emulated 30.0018 s  spent 1.5762 s  frames 1792\n";
        assert_eq!(parse_harness_line(stdout), Some((19.034, 30.0018)));
        assert_eq!(parse_harness_line("could not load the ROM\n"), None);
    }

    #[test]
    fn callgrind_summaries_give_the_instruction_count() {
        let stderr = "==5470== Events    : Ir\n==5470== Collected : 578204262\n==5470==\n";
        assert_eq!(parse_collected(stderr), Some(578_204_262));
    }

    #[test]
    fn nubo_runs_whole_buffers() {
        let seconds = nubo_emulated_seconds(30.0);
        assert!((seconds - 2584.0 * 512.0 / 44_100.0).abs() < 1e-9);
    }

    #[test]
    fn roms_that_share_a_name_are_labeled_by_their_path() {
        let roms = [
            PathBuf::from("originals/game.gb"),
            PathBuf::from("hacks/game.gb"),
            PathBuf::from("hacks/other.gbc"),
        ];
        assert_eq!(
            rom_labels(&roms),
            vec![
                PathBuf::from("originals/game.gb").display().to_string(),
                PathBuf::from("hacks/game.gb").display().to_string(),
                String::from("other"),
            ]
        );
    }

    #[test]
    fn the_table_shows_the_best_round_and_failures_as_dashes() {
        let labels = [String::from("tobudx")];
        let mut results = Results::default();
        let run = |emulator| Run {
            emulator,
            rom: 0,
            model: Model::Dmg,
        };
        let speed = |speed| Measurement {
            speed,
            emulated: 30.0,
            instructions: None,
        };
        results.add(run(Emulator::Nubo), Ok(speed(80.0)));
        results.add(run(Emulator::Nubo), Ok(speed(86.12)));
        results.add(run(Emulator::Binjgb), Err(String::from("can't")));
        let text = table(
            &labels,
            &[Model::Dmg],
            &[Emulator::Nubo, Emulator::Binjgb],
            &results,
            false,
        );
        assert_eq!(
            text,
            "             Nubo  binjgb\ntobudx DMG  86.1x       -\n"
        );
    }
}
