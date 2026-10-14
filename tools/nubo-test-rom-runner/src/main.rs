//! Runs the Game Boy test ROM suites (Blargg, Mooneye (Gekkio's and Wilbert
//! Pol's), SameSuite, the acid tests, Mealybug Tearoom, Gambatte,
//! GBMicrotest, AGE, rtc3test, Bully, Strikethrough, Scribbltests,
//! TurtleTests, little-things-gb, the MBC3 Tester, and the GB Emulator
//! Shootout's tests, judged the way the Shootout judges them) in parallel and
//! compares the results with `passing.txt`, so regressions, newly passing
//! tests and listed tests that no longer run are noticed. The README lists
//! the tests that aren't run and why.
//!
//! After a count per suite and model and the total, a run that includes every
//! Shootout test prints one more line with Nubo's score on the Shootout's
//! published tests and the rank it would have there, such as
//! `  shootout  261/264  rank 1  leader DocBoy 255  published 2026-06-04 72fd745`,
//! which the nightly run records (see `shootout.rs`).

use clap::Parser;
use runner::Outcome;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::process::ExitCode;
use std::time::Instant;

/// Run the Game Boy test ROM suites and compare the results with the list of passing tests
#[derive(Parser)]
#[command(name = "nubo-test-rom-runner")]
struct Args {
    /// Only run tests whose name contains this text, such as "mooneye" or "cgb blargg"
    filter: Option<String>,

    /// Record the results of this run in the list of passing tests
    #[arg(long)]
    update: bool,

    /// List every failing test with the reason it failed
    #[arg(long)]
    verbose: bool,

    /// Run the boot ROM before every test instead of starting from a copy of an earlier boot,
    /// and check that the copy would have been identical
    #[arg(long)]
    verify_boot: bool,
}

fn main() -> ExitCode {
    match run(&Args::parse()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Runs the selected tests and reports the results. Returns whether they
/// match the list of passing tests (always true with `--update`).
///
/// An unfiltered run also fails when a listed test wasn't run at all (a
/// renamed ROM, a changed model rule, a suite that is no longer collected),
/// listing it under "Listed but not run", and an unfiltered `--update`
/// removes such entries. A filtered run leaves the entries of tests outside
/// the filter alone.
fn run(args: &Args) -> Result<bool, String> {
    let rom_dirs = download::ensure_test_roms()?;
    let failures_dir = download::work_dir().join("failures");
    let _ = fs::remove_dir_all(&failures_dir);

    let tests: Vec<_> = suites::discover(&rom_dirs)
        .into_iter()
        .filter(|test| {
            args.filter
                .as_ref()
                .is_none_or(|filter| test.id().contains(filter.as_str()))
        })
        .collect();
    if tests.is_empty() {
        return Err("no tests match the filter".to_string());
    }

    println!("Running {} test ROMs", tests.len());
    let start = Instant::now();
    let outcomes = runner::run_all(&rom_dirs, &tests, &failures_dir, args.verify_boot)?;
    println!("Finished in {:.1}s\n", start.elapsed().as_secs_f64());

    print_summary(&tests, &outcomes);

    let run_ids: BTreeSet<String> = tests.iter().map(suites::TestCase::id).collect();
    let passing: BTreeSet<String> = tests
        .iter()
        .zip(&outcomes)
        .filter(|(_, outcome)| matches!(outcome, Outcome::Pass))
        .map(|(test, _)| test.id())
        .collect();
    if let Some(standing) = shootout::standing(&run_ids, &passing)? {
        println!("  {standing}");
    }

    if args.verbose {
        println!("\nFailing tests:");
        for (test, outcome) in tests.iter().zip(&outcomes) {
            if let Outcome::Fail(reason) = outcome {
                println!("  {}: {}", test.id(), reason);
            }
        }
    }

    let expected = expectations::load()?;
    // Only an unfiltered run sees every test, so only there does a listed test
    // that didn't run mean a stale entry rather than one the filter left out.
    let unfiltered = args.filter.is_none();
    let listed_but_not_run: BTreeSet<String> = expected.difference(&run_ids).cloned().collect();

    if args.update {
        let kept = if unfiltered {
            print_ids(
                "Removed from the list, since they weren't run:",
                &listed_but_not_run,
            );
            BTreeSet::new()
        } else {
            listed_but_not_run
        };
        let updated: BTreeSet<String> = kept.into_iter().chain(passing).collect();
        expectations::save(&updated)?;
        println!("\nUpdated {}", expectations::path().display());
        return Ok(true);
    }

    let expected_in_run: BTreeSet<String> = expected.intersection(&run_ids).cloned().collect();
    let regressions: Vec<_> = expected_in_run.difference(&passing).collect();
    let improvements: Vec<_> = passing.difference(&expected_in_run).collect();

    if !regressions.is_empty() {
        println!("\nNo longer passing:");
        for (test, outcome) in tests.iter().zip(&outcomes) {
            if let (true, Outcome::Fail(reason)) = (expected_in_run.contains(&test.id()), outcome) {
                println!("  {}: {}", test.id(), reason);
            }
        }
    }
    print_ids(
        "Newly passing (run with --update to record them):",
        improvements.iter().copied(),
    );
    let not_run = if unfiltered {
        listed_but_not_run
    } else {
        BTreeSet::new()
    };
    print_ids(
        "Listed but not run (find out why, then run with --update to remove them):",
        &not_run,
    );
    if failures_dir.exists() {
        println!(
            "\nScreens of failing screenshot tests are in {}",
            failures_dir.display()
        );
    }

    Ok(regressions.is_empty() && improvements.is_empty() && not_run.is_empty())
}

/// Prints the given test IDs under a heading, or nothing when there are none.
fn print_ids<'a>(heading: &str, ids: impl IntoIterator<Item = &'a String>) {
    let mut ids = ids.into_iter().peekable();
    if ids.peek().is_some() {
        println!("\n{heading}");
        for id in ids {
            println!("  {id}");
        }
    }
}

/// Prints how many tests passed per suite and model, and in total.
fn print_summary(tests: &[suites::TestCase], outcomes: &[Outcome]) {
    let mut totals: BTreeMap<(String, String), (usize, usize)> = BTreeMap::new();
    for (test, outcome) in tests.iter().zip(outcomes) {
        let entry = totals
            .entry((test.suite(), suites::model_label(test.model)))
            .or_default();
        entry.1 += 1;
        if matches!(outcome, Outcome::Pass) {
            entry.0 += 1;
        }
    }

    for ((suite, model), (passed, total)) in &totals {
        println!("  {suite:<28} {model:<6} {passed:>3}/{total:<3}");
    }
    let passed: usize = totals.values().map(|(passed, _)| passed).sum();
    println!("  {:<28} {:>8}/{}", "total", passed, tests.len());
}

mod boot;
mod download;
mod expectations;
mod gambatte;
mod runner;
mod screen;
mod shootout;
mod suites;
