//! Runs test ROMs in parallel and decides whether each one passed.

use crate::boot::{BootCache, BootError, Booted};
use crate::download::RomDirs;
use crate::screen::GrayscaleReference;
use crate::shootout::MATCH_HOLD_AUDIO_BUFFERS;
use crate::suites::{Check, Source, TestCase, model_label};
use crate::{gambatte, screen};
use image::RgbImage;
use nubo_core::{Emulator, Key};
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

/// How many audio buffers of the final frame a sound test listens to.
const SOUND_CHECK_AUDIO_BUFFERS: u32 = 2;
/// GBMicrotest's result byte in high RAM and the values it writes there.
const HIGH_RAM_RESULT_ADDRESS: u16 = 0xFF82;
const HIGH_RAM_PASS: u8 = 0x01;
const HIGH_RAM_FAIL: u8 = 0xFF;
/// LCDC (0xFF40) and its bit that turns the screen on.
const LCDC_ADDRESS: u16 = 0xFF40;
const LCDC_SCREEN_ON: u8 = 0x80;
/// Mooneye-style tests signal success by loading the Fibonacci numbers
/// 3, 5, 8, 13, 21, 34 into registers B, C, D, E, H, L...
const PASS_REGISTERS: [u8; 6] = [3, 5, 8, 13, 21, 34];
/// ...and failure by loading 0x42 into all of them.
const FAIL_REGISTERS: [u8; 6] = [0x42; 6];
/// How long to keep running after a failure so the test can print its
/// result on screen before the screen is saved.
const FAILURE_SCREEN_AUDIO_BUFFERS: u32 = 16;
/// Audio buffers after the boot ROM before a test's first button press (a
/// second, by which games have drawn their menu or title screen), how long
/// each press lasts, and the pause after it.
const INPUT_DELAY_AUDIO_BUFFERS: u32 = 128;
const KEY_PRESS_AUDIO_BUFFERS: u32 = 16;
const KEY_RELEASE_AUDIO_BUFFERS: u32 = 16;
/// Real time one audio buffer (1/128 of an emulated second) may take before
/// the test counts as hung, because its emulated time has stopped advancing.
/// Tests that keep running are only limited by their emulated timeout,
/// however slow or busy the machine.
pub(crate) const STALL_LIMIT: Duration = Duration::from_secs(60);
/// How many tests may stall before the run stops. None should, so a few mean
/// the emulator hangs, and running the rest would take hours.
const MAX_HUNG_TESTS: usize = 3;
/// How long a test may go without finishing an audio buffer before the
/// watchdog decides it is stuck inside the emulator, where the test's own
/// stall checks can't run, and ends the run. Longer than `STALL_LIMIT`, so a
/// test whose emulated time stands still fails on its own first.
const STUCK_LIMIT: Duration = Duration::from_secs(90);
/// How often the watchdog looks at the running tests.
const WATCHDOG_INTERVAL: Duration = Duration::from_millis(500);
const STEPS_BETWEEN_STALL_CHECKS: usize = 10_000;

/// The result of one test.
pub(crate) enum Outcome {
    Pass,
    /// The test failed, with the reason.
    Fail(String),
}

/// Runs every test on all CPU cores and returns one outcome per test, in the
/// order of `tests`. The tests that may run longest start first, so they
/// overlap with the many short ones instead of running alone at the end.
/// Screens of failing tests are saved in `failures_dir`. With `verify_boot`,
/// every test runs the real boot ROM and also checks that the boot cache
/// would have started it in the same state.
///
/// Each outcome is stored as soon as its test finishes, so a worker thread
/// that panics outside a test (which the silenced panic hook would otherwise
/// hide) loses nothing else: the test it was on fails with the panic's
/// message, and the other workers run the rest.
///
/// Fails without running the rest once `MAX_HUNG_TESTS` tests have stalled,
/// and ends the process if a test is stuck inside the emulator, so a hang
/// never holds up the run for long.
pub(crate) fn run_all(
    dirs: &RomDirs,
    tests: &[TestCase],
    failures_dir: &Path,
    verify_boot: bool,
) -> Result<Vec<Outcome>, String> {
    let workers = thread::available_parallelism().map_or(4, std::num::NonZero::get);
    let mut order: Vec<usize> = (0..tests.len()).collect();
    order.sort_by_key(|&index| std::cmp::Reverse(tests[index].longest_run_seconds()));
    let next = AtomicUsize::new(0);
    let hung: Mutex<Vec<usize>> = Mutex::new(Vec::new());
    let statuses: Vec<WorkerStatus> = (0..workers).map(|_| WorkerStatus::default()).collect();
    let outcomes: Vec<OnceLock<Outcome>> = tests.iter().map(|_| OnceLock::new()).collect();

    // Panics are caught and reported per test, so silence the default message.
    panic::set_hook(Box::new(|_| {}));

    thread::scope(|scope| {
        let handles: Vec<_> = statuses
            .iter()
            .map(|status| {
                scope.spawn(|| {
                    let mut boot_cache = BootCache::new(verify_boot);
                    while hung.lock().map_or(0, |hung| hung.len()) < MAX_HUNG_TESTS {
                        let Some(&index) = order.get(next.fetch_add(1, Ordering::Relaxed)) else {
                            break;
                        };
                        status.set_test(Some(index));
                        let (outcome, stalled) = run_test(
                            dirs,
                            &tests[index],
                            failures_dir,
                            &mut boot_cache,
                            &status.heartbeat,
                        );
                        status.set_test(None);
                        if stalled && let Ok(mut hung) = hung.lock() {
                            hung.push(index);
                        }
                        let _ = outcomes[index].set(outcome);
                    }
                })
            })
            .collect();
        watch_for_stuck_tests(&handles, &statuses, tests);
        for (handle, status) in handles.into_iter().zip(&statuses) {
            if let Err(payload) = handle.join()
                && let Some(index) = status.test.lock().ok().and_then(|test| *test)
            {
                let _ = outcomes[index].set(Outcome::Fail(format!(
                    "the runner panicked outside the emulator: {}",
                    panic_message(payload.as_ref())
                )));
            }
        }
    });

    let _ = panic::take_hook();

    let hung = hung.into_inner().unwrap_or_default();
    if hung.len() >= MAX_HUNG_TESTS {
        let ids: Vec<String> = hung.iter().map(|&index| tests[index].id()).collect();
        return Err(format!(
            "stopped after {} tests stalled, so the emulator probably hangs:\n  {}",
            hung.len(),
            ids.join("\n  ")
        ));
    }
    Ok(outcomes
        .into_iter()
        .map(|outcome| {
            outcome.into_inner().unwrap_or_else(|| {
                Outcome::Fail("no result: the runner stopped before finishing it".to_string())
            })
        })
        .collect())
}

/// What a worker thread is doing, for the watchdog.
#[derive(Default)]
struct WorkerStatus {
    /// The index of the test it is running, if any.
    test: Mutex<Option<usize>>,
    /// Counts up whenever the running test emulates more time.
    heartbeat: AtomicU64,
}

impl WorkerStatus {
    fn set_test(&self, test: Option<usize>) {
        if let Ok(mut current) = self.test.lock() {
            *current = test;
        }
    }
}

/// Waits for the workers to finish, and ends the process if one of them has
/// run a test for `STUCK_LIMIT` without any progress: the test is then stuck
/// inside the emulator, where its own stall checks can't run, and its
/// thread can't be stopped any other way.
fn watch_for_stuck_tests<T>(
    handles: &[thread::ScopedJoinHandle<'_, T>],
    statuses: &[WorkerStatus],
    tests: &[TestCase],
) {
    // Each worker's test and heartbeat when the watchdog last saw them
    // change, and when that was.
    let mut last_changes: Vec<(Option<usize>, u64, Instant)> =
        statuses.iter().map(|_| (None, 0, Instant::now())).collect();
    while !handles.iter().all(thread::ScopedJoinHandle::is_finished) {
        thread::sleep(WATCHDOG_INTERVAL);
        for ((status, last_change), handle) in statuses.iter().zip(&mut last_changes).zip(handles) {
            // A worker that panicked still names the test it was on.
            if handle.is_finished() {
                continue;
            }
            let test = status.test.lock().ok().and_then(|test| *test);
            let heartbeat = status.heartbeat.load(Ordering::Relaxed);
            if (test, heartbeat) != (last_change.0, last_change.1) {
                *last_change = (test, heartbeat, Instant::now());
            } else if let Some(index) = test
                && last_change.2.elapsed() > STUCK_LIMIT
            {
                eprintln!(
                    "error: {} is stuck inside the emulator: no progress for {} seconds",
                    tests[index].id(),
                    last_change.2.elapsed().as_secs()
                );
                std::process::exit(1);
            }
        }
    }
}

/// The counter a test ticks whenever it emulates more time, which shows the
/// watchdog that it isn't stuck.
pub(crate) struct Progress<'a> {
    heartbeat: &'a AtomicU64,
}

impl Progress<'_> {
    /// Notes that the test emulated more time.
    pub(crate) fn tick(&self) {
        self.heartbeat.fetch_add(1, Ordering::Relaxed);
    }
}

/// Runs one test, turning a panic in the emulator into a failure. Also
/// returns whether the test stalled.
fn run_test(
    dirs: &RomDirs,
    test: &TestCase,
    failures_dir: &Path,
    boot_cache: &mut BootCache,
    heartbeat: &AtomicU64,
) -> (Outcome, bool) {
    let root = dirs.root(test.source);
    let rom = match fs::read(root.join(&test.rom)) {
        Ok(rom) => rom,
        Err(error) => return (Outcome::Fail(format!("failed to read ROM: {error}")), false),
    };
    let reference_paths: Vec<&PathBuf> = match &test.check {
        Check::Screen(path) | Check::EveryFrame(path) | Check::GambatteScreen(path) => vec![path],
        Check::GrayscaleScreen(paths) => paths.iter().collect(),
        Check::Shootout { pass, fail } => pass.iter().chain(fail).collect(),
        Check::Registers | Check::Number(_) | Check::Sound(_) | Check::HighRam => Vec::new(),
    };
    let reference_root = match &test.check {
        Check::Shootout { .. } => dirs.root(Source::Shootout),
        _ => root,
    };
    let references = match reference_paths
        .iter()
        .map(|path| screen::load_reference(&reference_root.join(path)))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(references) => references,
        Err(error) => return (Outcome::Fail(error), false),
    };

    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        emulate(&rom, test, &references, boot_cache, heartbeat)
    }));
    match result {
        Ok(Ok(())) => (Outcome::Pass, false),
        Ok(Err(failure)) => {
            if let Some(actual) = failure.screen {
                save_failure_screens(&actual, references.first(), failures_dir, test);
            }
            (Outcome::Fail(failure.reason), failure.stalled)
        }
        Err(payload) => (
            Outcome::Fail(format!("panicked: {}", panic_message(payload.as_ref()))),
            false,
        ),
    }
}

/// Why a test failed, with the screen at that moment when there is one.
struct Failure {
    reason: String,
    screen: Option<RgbImage>,
    /// Whether the test's emulated time stopped advancing.
    stalled: bool,
}

impl Failure {
    fn new(reason: impl Into<String>) -> Self {
        Failure {
            reason: reason.into(),
            screen: None,
            stalled: false,
        }
    }
}

/// Runs the ROM on the test's model, starting where the boot ROM hands over
/// to the cartridge, and checks its result the way the test reports it.
fn emulate(
    rom: &[u8],
    test: &TestCase,
    references: &[RgbImage],
    boot_cache: &mut BootCache,
    heartbeat: &AtomicU64,
) -> Result<(), Failure> {
    let progress = &Progress { heartbeat };
    let Booted {
        mut emulator,
        audio_buffers,
    } = boot_cache
        .boot(rom, test.model, progress)
        .map_err(|error| match error {
            BootError::TimedOut => Failure {
                reason: "timed out in the boot ROM".to_string(),
                screen: None,
                stalled: true,
            },
            BootError::Failed(reason) => Failure::new(reason),
        })?;
    let emulator = &mut emulator;
    let run = TestRun {
        test,
        audio_buffers: test.deadline.audio_buffers_after_boot(audio_buffers),
        progress,
    };

    match (&test.check, references.first()) {
        (Check::Screen(_), Some(reference)) => run_screen_test(emulator, &run, reference),
        (Check::EveryFrame(_), Some(reference)) => run_every_frame_test(emulator, &run, reference),
        (Check::HighRam, _) => run_high_ram_test(emulator, &run),
        (Check::Number(_) | Check::Sound(_) | Check::GambatteScreen(_), reference) => {
            run_gambatte_test(emulator, &run, reference)
        }
        (Check::GrayscaleScreen(_), _) => run_grayscale_screen_test(emulator, &run, references),
        (Check::Shootout { pass, .. }, _) => {
            run_shootout_test(emulator, &run, references.split_at(pass.len()))
        }
        _ => run_register_test(emulator, &run),
    }
}

/// A test being run, from the end of the boot ROM.
struct TestRun<'a> {
    test: &'a TestCase,
    /// How many audio buffers (1/128 of a second each) the test may run after
    /// the boot ROM before its deadline.
    audio_buffers: u32,
    progress: &'a Progress<'a>,
}

/// Runs until the registers hold the pass or fail values, the CPU locks up
/// (older Mooneye tests end on an undefined opcode), or the test runs out of
/// time.
fn run_register_test(emulator: &mut Emulator, run: &TestRun<'_>) -> Result<(), Failure> {
    let TestRun { test, progress, .. } = *run;
    for _ in 0..run.audio_buffers {
        step_audio_buffer(emulator, progress, test)?;

        let registers = emulator.registers();
        let values = [
            registers.b,
            registers.c,
            registers.d,
            registers.e,
            registers.h,
            registers.l,
        ];
        if values == PASS_REGISTERS {
            return Ok(());
        }
        let locked_up = emulator.is_locked_up();
        if values == FAIL_REGISTERS || locked_up {
            for _ in 0..FAILURE_SCREEN_AUDIO_BUFFERS {
                step_audio_buffer(emulator, progress, test)?;
            }
            let reason = if locked_up {
                "the test ended without passing"
            } else {
                "the test reported a failure"
            };
            return Err(Failure {
                reason: reason.to_string(),
                screen: Some(screen::normalize(emulator.frame_buffer(), test.model)),
                stalled: false,
            });
        }
    }

    Err(Failure {
        reason: format!("no result after {}", test.deadline),
        screen: Some(screen::normalize(emulator.frame_buffer(), test.model)),
        stalled: false,
    })
}

/// Runs until a frame matches the reference screenshot, or the test runs out
/// of time, pressing the test's buttons along the way.
fn run_screen_test(
    emulator: &mut Emulator,
    run: &TestRun<'_>,
    reference: &RgbImage,
) -> Result<(), Failure> {
    let TestRun { test, progress, .. } = *run;
    let mut held = None;
    for audio_buffer in 0..run.audio_buffers {
        let key = held_key(test.input, audio_buffer);
        if key != held {
            if let Some(key) = held {
                emulator.release_key(key);
            }
            if let Some(key) = key {
                emulator.press_key(key);
            }
            held = key;
        }
        step_audio_buffer(emulator, progress, test)?;

        if let Some(frame) = emulator.take_frame()
            && screen::differing_pixels(&screen::normalize(frame, test.model), reference) == 0
        {
            return Ok(());
        }
    }

    let actual = screen::normalize(emulator.frame_buffer(), test.model);
    let differing = screen::differing_pixels(&actual, reference);
    Err(Failure {
        reason: format!(
            "{} pixels differ from the reference after {}",
            differing, test.deadline
        ),
        screen: Some(actual),
        stalled: false,
    })
}

/// The button `input` holds down this many audio buffers after the boot ROM:
/// each one in turn is pressed and released after a short delay.
fn held_key(input: &[Key], audio_buffers_after_boot: u32) -> Option<Key> {
    let time = audio_buffers_after_boot.checked_sub(INPUT_DELAY_AUDIO_BUFFERS)?;
    let period = KEY_PRESS_AUDIO_BUFFERS + KEY_RELEASE_AUDIO_BUFFERS;
    let key = input.get(usize::try_from(time / period).ok()?)?;
    (time % period < KEY_PRESS_AUDIO_BUFFERS).then_some(*key)
}

/// Runs until the timeout and fails as soon as a frame differs from the
/// reference screenshot. The boot ROM's logo stays on screen until the game
/// first turns the screen off, so only the frames after that count.
fn run_every_frame_test(
    emulator: &mut Emulator,
    run: &TestRun<'_>,
    reference: &RgbImage,
) -> Result<(), Failure> {
    let TestRun { test, progress, .. } = *run;
    let timeout = run.audio_buffers;
    let mut audio_buffers = 0;
    while emulator.read_memory(LCDC_ADDRESS) & LCDC_SCREEN_ON != 0 {
        if audio_buffers >= timeout {
            return Err(Failure::new("the game never turned the screen off"));
        }
        if emulator.step_toward_next_audio_buffer(1) {
            audio_buffers += 1;
            progress.tick();
        }
    }
    emulator.take_frame();

    for _ in audio_buffers..timeout {
        step_audio_buffer(emulator, progress, test)?;
        if let Some(frame) = emulator.take_frame() {
            let actual = screen::normalize(frame, test.model);
            let differing = screen::differing_pixels(&actual, reference);
            if differing > 0 {
                return Err(Failure {
                    reason: format!(
                        "a frame has {differing} pixels that differ from the reference"
                    ),
                    screen: Some(actual),
                    stalled: false,
                });
            }
        }
    }
    Ok(())
}

/// Runs a test for its timeout after the boot ROM and checks the screen
/// against each of its screenshots in grayscale.
fn run_grayscale_screen_test(
    emulator: &mut Emulator,
    run: &TestRun<'_>,
    references: &[RgbImage],
) -> Result<(), Failure> {
    let TestRun { test, progress, .. } = *run;
    for _ in 0..run.audio_buffers {
        step_audio_buffer(emulator, progress, test)?;
    }

    let actual = screen::normalize(emulator.frame_buffer(), test.model);
    let fewest_differing = references
        .iter()
        .map(|reference| screen::differing_pixels_in_grayscale(&actual, reference))
        .min();
    let Some(fewest_differing) = fewest_differing else {
        return Err(Failure::new("the test has no reference screenshot"));
    };
    if fewest_differing == 0 {
        return Ok(());
    }
    Err(Failure {
        reason: format!(
            "{} pixels differ from the closest reference after {}",
            fewest_differing, test.deadline
        ),
        screen: Some(actual),
        stalled: false,
    })
}

/// What a frame of a GB Emulator Shootout test matches.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Pass,
    Fail,
}

/// Runs a test of the GB Emulator Shootout the way its harness judges it:
/// it passes once the screen has matched one of the `pass` screenshots for
/// `MATCH_HOLD_AUDIO_BUFFERS`, fails once it has matched one of the `fail`
/// screenshots that long, and fails at the deadline. Screens are compared in
/// grayscale, as the Shootout does, in the colors the app shows with
/// `--raw-screen` (see `screen::as_shown`): unlike the
/// other screenshot checks, the original Game Boy's grays aren't mapped to
/// the screenshots' own, since the harness takes screenshots of the app.
fn run_shootout_test(
    emulator: &mut Emulator,
    run: &TestRun<'_>,
    (pass, fail): (&[RgbImage], &[RgbImage]),
) -> Result<(), Failure> {
    let TestRun { test, progress, .. } = *run;
    let pass_grays: Vec<GrayscaleReference> = pass.iter().map(GrayscaleReference::new).collect();
    let fail_grays: Vec<GrayscaleReference> = fail.iter().map(GrayscaleReference::new).collect();
    // What the screen matches, and since which audio buffer.
    let mut shown: Option<(Verdict, u32)> = None;
    for audio_buffer in 0..run.audio_buffers {
        step_audio_buffer(emulator, progress, test)?;
        if let Some(frame) = emulator.take_frame() {
            let matches_any = |references: &[GrayscaleReference]| {
                references
                    .iter()
                    .any(|reference| reference.matches_as_shown(frame))
            };
            let verdict = if matches_any(&pass_grays) {
                Some(Verdict::Pass)
            } else if matches_any(&fail_grays) {
                Some(Verdict::Fail)
            } else {
                None
            };
            if verdict != shown.map(|(shown, _)| shown) {
                shown = verdict.map(|verdict| (verdict, audio_buffer));
            }
        }
        if let Some((verdict, since)) = shown
            && audio_buffer + 1 - since >= MATCH_HOLD_AUDIO_BUFFERS
        {
            if verdict == Verdict::Pass {
                return Ok(());
            }
            return Err(Failure {
                reason: "the screen matches a fail screenshot".to_string(),
                screen: Some(screen::as_shown(emulator.frame_buffer())),
                stalled: false,
            });
        }
    }

    let actual = screen::as_shown(emulator.frame_buffer());
    let reason = if shown.is_some() {
        format!(
            "the screen matched a screenshot too briefly to count before the deadline of {}",
            test.deadline
        )
    } else {
        let fewest_differing = pass
            .iter()
            .map(|reference| screen::differing_pixels_in_grayscale(&actual, reference))
            .min()
            .unwrap_or(0);
        format!(
            "{} pixels differ from the closest pass screenshot after {}",
            fewest_differing, test.deadline
        )
    };
    Err(Failure {
        reason,
        screen: Some(actual),
        stalled: false,
    })
}

/// Runs a GBMicrotest ROM until it writes its result to high RAM, for up to
/// the test's timeout after the boot ROM.
fn run_high_ram_test(emulator: &mut Emulator, run: &TestRun<'_>) -> Result<(), Failure> {
    let TestRun { test, progress, .. } = *run;
    for _ in 0..run.audio_buffers {
        step_audio_buffer(emulator, progress, test)?;

        match emulator.read_memory(HIGH_RAM_RESULT_ADDRESS) {
            HIGH_RAM_PASS => return Ok(()),
            HIGH_RAM_FAIL => {
                let actual = emulator.read_memory(HIGH_RAM_RESULT_ADDRESS - 2);
                let expected = emulator.read_memory(HIGH_RAM_RESULT_ADDRESS - 1);
                return Err(Failure::new(format!(
                    "the test read 0x{actual:02X} instead of 0x{expected:02X}"
                )));
            }
            _ => {}
        }
    }
    Err(Failure::new(format!("no result after {}", test.deadline)))
}

/// Runs a Gambatte test for a fixed number of frames after the boot ROM and
/// then checks the number on screen, the sound, or the whole screen.
fn run_gambatte_test(
    emulator: &mut Emulator,
    run: &TestRun<'_>,
    reference: Option<&RgbImage>,
) -> Result<(), Failure> {
    let TestRun { test, progress, .. } = *run;
    let audio_buffers = run.audio_buffers.max(SOUND_CHECK_AUDIO_BUFFERS);
    for _ in SOUND_CHECK_AUDIO_BUFFERS..audio_buffers {
        step_audio_buffer(emulator, progress, test)?;
    }
    let mut samples = Vec::new();
    for _ in 0..SOUND_CHECK_AUDIO_BUFFERS {
        let (left, right) = emulator.step_until_next_audio_buffer();
        samples.extend(left.iter().zip(right).map(|(left, right)| left + right));
    }

    let actual = gambatte::normalize(emulator.frame_buffer(), test.model);
    let reason = match &test.check {
        Check::Number(number) if number.is_empty() => {
            "the ROM's name gives no number to expect".to_string()
        }
        Check::Number(number) if !gambatte::shows_number(&actual, number) => {
            let shown = gambatte::shown_number(&actual, number.len());
            if shown.is_empty() {
                format!("expected {number} on screen, but no number is shown")
            } else {
                format!("expected {number} on screen, but it shows {shown}")
            }
        }
        Check::Sound(expected) if gambatte::is_silent(&samples) == *expected => {
            if *expected {
                "expected sound, but the output is silent".to_string()
            } else {
                "expected silence, but the output changes".to_string()
            }
        }
        Check::GambatteScreen(_) => match reference {
            Some(reference) if screen::differing_pixels(&actual, reference) > 0 => format!(
                "{} pixels differ from the reference",
                screen::differing_pixels(&actual, reference)
            ),
            _ => return Ok(()),
        },
        _ => return Ok(()),
    };
    Err(Failure {
        reason,
        screen: Some(actual),
        stalled: false,
    })
}

/// Emulates one audio buffer's worth of time, failing the test if that takes
/// longer than `STALL_LIMIT` of real time.
fn step_audio_buffer(
    emulator: &mut Emulator,
    progress: &Progress<'_>,
    test: &TestCase,
) -> Result<(), Failure> {
    let started = Instant::now();
    while !emulator.step_toward_next_audio_buffer(STEPS_BETWEEN_STALL_CHECKS) {
        progress.tick();
        if started.elapsed() > STALL_LIMIT {
            return Err(Failure {
                reason: format!(
                    "emulated time stopped advancing for {} seconds of wall-clock time",
                    STALL_LIMIT.as_secs()
                ),
                screen: Some(screen::normalize(emulator.frame_buffer(), test.model)),
                stalled: true,
            });
        }
    }
    progress.tick();
    Ok(())
}

/// Saves the failing screen as a PNG in `failures_dir/<model>/`, and with a
/// reference also a `.diff.png` (the reference with the differing pixels
/// painted red). Errors are ignored
/// because these images are only a debugging aid.
fn save_failure_screens(
    actual: &RgbImage,
    reference: Option<&RgbImage>,
    failures_dir: &Path,
    test: &TestCase,
) {
    let folder = if test.shootout_name.is_some() {
        failures_dir.join("shootout")
    } else {
        failures_dir.to_path_buf()
    };
    let mut path = folder.join(model_label(test.model)).join(&test.rom);
    if let Some(subtest) = test.subtest {
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        path.set_file_name(format!("{stem}-{subtest}"));
    }
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = actual.save(path.with_extension("png"));
    if let Some(reference) = reference {
        let differences = if matches!(
            test.check,
            Check::GrayscaleScreen(_) | Check::Shootout { .. }
        ) {
            screen::highlight_differences_in_grayscale(actual, reference)
        } else {
            screen::highlight_differences(actual, reference)
        };
        let _ = differences.save(path.with_extension("diff.png"));
    }
}

/// The message of a caught panic, if it has one.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&str>()
                .map(std::string::ToString::to_string)
        })
        .unwrap_or_else(|| "unknown panic".to_string())
}
