//! The tests of the GB Emulator Shootout (gbdev/GBEmulatorShootout), judged
//! the way its harness judges them, and the table it publishes.
//!
//! The Shootout runs about 260 test ROMs in each emulator's real program at
//! gbdev.io/GBEmulatorShootout. Its harness takes a screenshot of the window
//! every 0.1 seconds and compares it with the test's reference screenshots:
//! a test passes when a pass image matches (in grayscale, no pixel more than
//! 50 off), fails when a `.fail.png` matches, and fails when neither has
//! matched by its deadline. This module runs the same tests headless:
//!
//! - [`TESTS`] lists them as the Shootout's `main` branch has them at the
//!   commit `download.rs` pins, with their IDs `shootout/<name>`. DMG tests run
//!   on the DMG, CGB tests on the CGB-E and SGB tests on the SGB, the models
//!   the harness class starts (`.github/shootout/nubo.py`), and are
//!   compared on the Game Boy screen, which the class crops out of a Super
//!   Game Boy's picture with its border. Tests without a reference screenshot
//!   are left out: they only report what they find.
//! - A match only counts once it has held for [`MATCH_HOLD_AUDIO_BUFFERS`]:
//!   at [`HARNESS_SPEED`], the harness's 0.1 seconds between screenshots are
//!   0.4 emulated seconds, so a screen shown for less than that might never be
//!   seen.
//! - A test fails at the harness's own deadline ([`deadline_milliseconds`]):
//!   its `runtime` at [`HARNESS_SPEED`], plus the startup time and 5 seconds of
//!   real time, which at that speed is the `runtime` plus 26 emulated seconds,
//!   counted from power-on. The `runtime` values are only rough: on Nubo a
//!   test can need up to 8.5 seconds more, on the CGB the boot ROM alone takes
//!   3.1 seconds, and passing tests stop as soon as they pass, so the full
//!   deadline only costs time when a test fails.
//!
//! `shootout_published.txt`, written by `cargo xtask shootout-scoreboard`,
//! holds the published table: which emulators pass each published test. The
//! runner prints Nubo's score on those tests and the rank it would have
//! ([`Standing`]), which the nightly metrics record.
//!
//! [`TESTS`] was generated once from the test lists in the Shootout's
//! `testroms/*.py` and `main.py` at the pinned commit, without running them,
//! and is regenerated when that pin moves. Each `Test(...)` there gives a
//! test's name, model and `runtime`, its ROM (`rom`, or else the name) and
//! its pass screenshots (`result`, or else the ROM's path with `.png`); its
//! fail screenshots are the `.fail.png` next to each pass screenshot, where
//! the Shootout has one (it has none at the pinned commit). A ROM comes from
//! the c-sp collection when the collection's copy has the same SHA-256
//! checksum as the Shootout's (`in_collection`); the other ROMs and all
//! reference screenshots are downloaded from the Shootout and listed in
//! `shootout.sha256`. The collection and the Shootout are both pinned, so
//! the checksums that matched when the table was made still match. Most of
//! the collection's Mooneye, SameSuite and Blargg ROMs are other builds than
//! the Shootout's, so only 53 of the 261 tests run the collection's copy.

use crate::suites::{AUDIO_BUFFERS_PER_SECOND, Check, Deadline, Model, Source, TestCase};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use list::TESTS;

/// The speed the harness class runs Nubo at (`--speed 4` in
/// `.github/shootout/nubo.py`, the draft of the Shootout's `emulators/nubo.py`).
const HARNESS_SPEED: u32 = 4;
/// How often the harness takes a screenshot, in milliseconds of real time.
const SCREENSHOT_INTERVAL_MILLISECONDS: u32 = 100;
/// How many audio buffers (1/128 of an emulated second each) a screen must
/// keep matching before it counts: one screenshot interval at
/// [`HARNESS_SPEED`], rounded up.
pub(crate) const MATCH_HOLD_AUDIO_BUFFERS: u32 =
    (SCREENSHOT_INTERVAL_MILLISECONDS * HARNESS_SPEED * AUDIO_BUFFERS_PER_SECOND).div_ceil(1000);
/// The time the harness gives the emulator to start before its deadline
/// starts counting, in milliseconds of real time: `startup_time` in the
/// harness class (`.github/shootout/nubo.py`).
const HARNESS_STARTUP_MILLISECONDS: u32 = 1500;
/// The time the harness waits after a test's `runtime` and the startup time
/// before the test fails, in milliseconds of real time (`Emulator.run` in the
/// Shootout's `emulator.py`).
const HARNESS_EXTRA_MILLISECONDS: u32 = 5000;
/// The test name prefix that makes a test ID one of the Shootout's.
pub(crate) const ID_PREFIX: &str = "shootout/";
/// The published table, written by `cargo xtask shootout-scoreboard`.
const SCOREBOARD: &str = include_str!("../shootout_published.txt");

/// The Shootout's `DMG` model.
const DMG: Model = Model::Dmg;
/// The Shootout's `CGB` model, without a revision. The harness class starts
/// Nubo with `--model cgb`, which is the CGB-E.
const CGB: Model = Model::CgbE;
/// The Shootout's `SGB` model. The harness class starts Nubo with
/// `--model sgb`.
const SGB: Model = Model::Sgb;

/// One of the Shootout's tests, as its `Test(...)` in `testroms/*.py` gives it.
pub(crate) struct ShootoutTest {
    /// Its name in the Shootout, such as `daid/stop_instr.gb (DMG)`.
    name: &'static str,
    /// The ROM's path under the Shootout's `testroms` folder, when it isn't
    /// the name.
    rom: Option<&'static str>,
    /// The path of the same ROM in the c-sp collection, when the two have the
    /// same SHA-256 checksum.
    collection_rom: Option<&'static str>,
    model: Model,
    /// The Shootout's `runtime`: how long the test needs from power-on, in
    /// emulated milliseconds.
    runtime_milliseconds: u32,
    /// The screenshots that mean it passed, when they aren't the ROM's path
    /// with `.png` in place of its extension.
    results: Option<&'static [&'static str]>,
    /// The screenshots that mean it failed: each pass screenshot's
    /// `.fail.png`, where the Shootout has one.
    fail_results: &'static [&'static str],
}

/// A test named `name` that runs on `model` for `runtime_milliseconds`, with
/// the Shootout's defaults: the ROM is at the name's path and its screenshot
/// next to it.
const fn test(name: &'static str, model: Model, runtime_milliseconds: u32) -> ShootoutTest {
    ShootoutTest {
        name,
        rom: None,
        collection_rom: None,
        model,
        runtime_milliseconds,
        results: None,
        fail_results: &[],
    }
}

impl ShootoutTest {
    /// The same test with its ROM at `rom` in the Shootout.
    const fn rom(self, rom: &'static str) -> Self {
        ShootoutTest {
            rom: Some(rom),
            ..self
        }
    }

    /// The same test, run on the collection's identical copy of the ROM at
    /// `rom`.
    const fn in_collection(self, rom: &'static str) -> Self {
        ShootoutTest {
            collection_rom: Some(rom),
            ..self
        }
    }

    /// The same test with these pass screenshots.
    const fn results(self, results: &'static [&'static str]) -> Self {
        ShootoutTest {
            results: Some(results),
            ..self
        }
    }

    /// The same test with these fail screenshots. The Shootout has none at
    /// the pinned commit; regenerating [`TESTS`] adds them when it does.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no test has a fail screenshot at the pinned commit"
        )
    )]
    const fn fail_results(self, fail_results: &'static [&'static str]) -> Self {
        ShootoutTest {
            fail_results,
            ..self
        }
    }

    /// The test as the runner runs it.
    fn test_case(&self) -> TestCase {
        let shootout_rom = self.rom.unwrap_or(self.name);
        let pass = match self.results {
            Some(results) => results.iter().map(PathBuf::from).collect(),
            None => vec![Path::new(shootout_rom).with_extension("png")],
        };
        let (rom, source) = match self.collection_rom {
            Some(rom) => (rom, Source::Collection),
            None => (shootout_rom, Source::Shootout),
        };
        TestCase {
            rom: PathBuf::from(rom),
            source,
            model: self.model,
            check: Check::Shootout {
                pass,
                fail: self.fail_results.iter().map(PathBuf::from).collect(),
            },
            deadline: Deadline::MillisecondsFromPowerOn(deadline_milliseconds(
                self.runtime_milliseconds,
            )),
            subtest: None,
            input: &[],
            shootout_name: Some(self.name),
        }
    }
}

/// When the harness gives up on a test whose `runtime` is
/// `runtime_milliseconds`, in emulated milliseconds from power-on: after
/// `runtime / speed + startup_time + 5` seconds of real time, which at
/// [`HARNESS_SPEED`] are this many emulated milliseconds.
fn deadline_milliseconds(runtime_milliseconds: u32) -> u32 {
    runtime_milliseconds
        + (HARNESS_STARTUP_MILLISECONDS + HARNESS_EXTRA_MILLISECONDS) * HARNESS_SPEED
}

/// Every test in [`TESTS`].
pub(crate) fn test_cases() -> Vec<TestCase> {
    TESTS.iter().map(ShootoutTest::test_case).collect()
}

/// The table at gbdev.io/GBEmulatorShootout as `shootout_published.txt`
/// records it.
struct Scoreboard {
    /// The commit of the Shootout's `gh-pages` branch it was read from.
    commit: String,
    /// The day the results were published, such as `2026-06-04`.
    published: String,
    /// Each published test's name, with the emulators that pass it.
    tests: Vec<(String, Vec<String>)>,
}

impl Scoreboard {
    /// Reads the scoreboard file: `#` comments, a `commit` and a `published`
    /// line, then one line per test with its name, a tab and the emulators
    /// that pass it, separated by commas.
    fn parse(text: &str) -> Result<Self, String> {
        let mut commit = None;
        let mut published = None;
        let mut tests = Vec::new();
        for line in text.lines().filter(|line| !line.starts_with('#')) {
            let Some((key, value)) = line.split_once('\t') else {
                return Err(format!("the scoreboard line {line:?} has no tab"));
            };
            match key {
                "commit" => commit = Some(value.to_string()),
                "published" => published = Some(value.to_string()),
                name => tests.push((
                    name.to_string(),
                    value
                        .split(',')
                        .filter(|emulator| !emulator.is_empty())
                        .map(str::to_string)
                        .collect(),
                )),
            }
        }
        Ok(Scoreboard {
            commit: commit.ok_or("the scoreboard has no commit line")?,
            published: published.ok_or("the scoreboard has no published line")?,
            tests,
        })
    }

    /// How many published tests each emulator passes.
    fn passed_by_emulator(&self) -> BTreeMap<&str, usize> {
        let mut passed = BTreeMap::new();
        for (_, emulators) in &self.tests {
            for emulator in emulators {
                *passed.entry(emulator.as_str()).or_default() += 1;
            }
        }
        passed
    }

    /// Where Nubo would stand in the table if it passed the tests named in
    /// `passing`.
    fn standing(&self, passing: &BTreeSet<&str>) -> Standing {
        let passed = self
            .tests
            .iter()
            .filter(|(name, _)| passing.contains(name.as_str()))
            .count();
        let by_emulator = self.passed_by_emulator();
        let (leader, leader_passed) = by_emulator
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
            .map_or(("", 0), |(name, passed)| (*name, *passed));
        Standing {
            passed,
            total: self.tests.len(),
            rank: 1 + by_emulator
                .values()
                .filter(|&&other| other > passed)
                .count(),
            leader: leader.to_string(),
            leader_passed,
            published: self.published.clone(),
            commit: self.commit.chars().take(7).collect(),
        }
    }
}

/// Nubo's score on the published tests and the rank it would have, as the
/// runner's last line shows it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Standing {
    /// How many published tests Nubo passes.
    passed: usize,
    /// How many tests the published table has, including the ones that only
    /// report information and the Super Game Boy ones.
    total: usize,
    /// One more than the number of emulators that pass more tests, so a tie
    /// shares the rank.
    rank: usize,
    /// The emulator at the top of the published table, and how many tests it
    /// passes.
    leader: String,
    leader_passed: usize,
    /// When the table was published, and the short commit of the Shootout's
    /// `gh-pages` branch it was read from.
    published: String,
    commit: String,
}

impl fmt::Display for Standing {
    /// The line the nightly run reads, such as `shootout  258/264  rank 1
    /// leader DocBoy 255  published 2026-06-04 72fd745`.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "shootout  {}/{}  rank {}  leader {} {}  published {} {}",
            self.passed,
            self.total,
            self.rank,
            self.leader,
            self.leader_passed,
            self.published,
            self.commit
        )
    }
}

/// Nubo's standing on the published tests, given the IDs of the tests that
/// ran and of those that passed. `None` when not every test in [`TESTS`]
/// ran, since a filtered run can't give the score.
pub(crate) fn standing(
    run_ids: &BTreeSet<String>,
    passing: &BTreeSet<String>,
) -> Result<Option<Standing>, String> {
    let all_ran = TESTS
        .iter()
        .all(|test| run_ids.contains(&format!("{ID_PREFIX}{}", test.name)));
    if !all_ran {
        return Ok(None);
    }
    let passing_names: BTreeSet<&str> = passing
        .iter()
        .filter_map(|id| id.strip_prefix(ID_PREFIX))
        .collect();
    Ok(Some(
        Scoreboard::parse(SCOREBOARD)?.standing(&passing_names),
    ))
}

mod list;
#[cfg(test)]
mod tests;
