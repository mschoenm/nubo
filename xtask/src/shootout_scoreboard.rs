//! `cargo xtask shootout-scoreboard <commit>`: reads the table the GB
//! Emulator Shootout publishes at gbdev.io/GBEmulatorShootout from its
//! `gh-pages` branch at `<commit>` (a full commit hash) and writes it to
//! `tools/nubo-test-rom-runner/shootout_published.txt`, where `cargo
//! test-roms` compares Nubo's results with it (`docs/SHOOTOUT_PLAN.md`,
//! Part 5).
//!
//! The branch has `tests.json`, the tests the table shows, `emulators.json`,
//! the emulators and their result files, and one `<emulator>.json` per
//! emulator with each test's result (`PASS`, `FAIL` or `INFO`) and a
//! screenshot, 6.4 MB in all. The scoreboard keeps only which emulators pass
//! each test, so CI never downloads these files. It is updated in a pull
//! request of its own when the Shootout publishes again.

use crate::{Result, target_dir, workspace_root};
use serde_json::Value;
use std::fmt::Write;
use std::fs;
use std::path::Path;
use std::process::Command;

/// Where the files of the Shootout's branches are downloaded from.
const RAW_URL: &str = "https://raw.githubusercontent.com/gbdev/GBEmulatorShootout";
/// The scoreboard, relative to the workspace root.
const SCOREBOARD: &str = "tools/nubo-test-rom-runner/shootout_published.txt";
const SECONDS_PER_DAY: i64 = 86_400;

/// Runs `shootout-scoreboard <commit>`.
pub fn run(args: &[String]) -> Result {
    let [commit] = args else {
        return Err(String::from(
            "usage: cargo xtask shootout-scoreboard <commit of the Shootout's gh-pages branch>",
        ));
    };
    if commit.len() != 40 || !commit.chars().all(|digit| digit.is_ascii_hexdigit()) {
        return Err(format!(
            "{commit} is not a full commit hash; the scoreboard pins the publication by its 40 \
             hexadecimal digits"
        ));
    }

    let folder = target_dir().join("shootout-scoreboard").join(commit);
    fs::create_dir_all(&folder)
        .map_err(|error| format!("failed to create {}: {error}", folder.display()))?;
    let tests = download_json(commit, "tests.json", &folder)?;
    let emulators = download_json(commit, "emulators.json", &folder)?;
    let files: Vec<&str> = emulators
        .as_object()
        .ok_or("emulators.json is not an object")?
        .values()
        .map(|emulator| {
            emulator
                .get("file")
                .and_then(Value::as_str)
                .ok_or("an emulator in emulators.json has no file")
        })
        .collect::<std::result::Result<_, _>>()?;
    let results = files
        .iter()
        .map(|file| download_json(commit, file, &folder))
        .collect::<Result<Vec<_>>>()?;

    let path = workspace_root().join(SCOREBOARD);
    fs::write(&path, scoreboard(commit, &tests, &results)?)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    println!("Wrote {}", path.display());
    Ok(())
}

/// Downloads `file` from the Shootout at `commit` into `folder` with curl and
/// reads it as JSON.
fn download_json(commit: &str, file: &str, folder: &Path) -> Result<Value> {
    let path = folder.join(file);
    let url = format!("{RAW_URL}/{commit}/{}", percent_encode(file));
    let status = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--output",
        ])
        .arg(&path)
        .arg(&url)
        .status()
        .map_err(|error| format!("failed to run curl: {error}"))?;
    if !status.success() {
        return Err(format!("failed to download {url}"));
    }
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("{file} is not valid JSON: {error}"))
}

/// `text` with every byte outside the URL's unreserved characters written as
/// `%XX`, such as the `$` in `no$gmb.json`.
fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

/// The scoreboard file for the publication at `commit`: comments, a
/// `commit` and a `published` line, then each test in `tests` (the contents
/// of `tests.json`) with the emulators whose `results` (their
/// `<emulator>.json`) say it passes, alphabetically. The publication date is
/// the day, in UTC, of the newest result.
fn scoreboard(commit: &str, tests: &Value, results: &[Value]) -> Result<String> {
    let names: Vec<&str> = tests
        .as_array()
        .ok_or("tests.json is not a list")?
        .iter()
        .map(|test| {
            test.get("name")
                .and_then(Value::as_str)
                .ok_or("a test in tests.json has no name")
        })
        .collect::<std::result::Result<_, _>>()?;
    let mut emulators: Vec<(&str, &Value)> = results
        .iter()
        .map(|result| {
            let name = result
                .get("emulator")
                .and_then(Value::as_str)
                .ok_or("a result file has no emulator name")?;
            let tests = result
                .get("tests")
                .ok_or_else(|| format!("the results of {name} have no tests"))?;
            Ok((name, tests))
        })
        .collect::<Result<_>>()?;
    emulators.sort_by_key(|(name, _)| name.to_lowercase());
    let newest = results
        .iter()
        .filter_map(|result| result.get("date").and_then(Value::as_f64))
        .reduce(f64::max)
        .ok_or("no result file has a date")?;

    let mut text = String::from(
        "# The GB Emulator Shootout's published table (gbdev.io/GBEmulatorShootout), read from its\n\
         # gh-pages branch by `cargo xtask shootout-scoreboard <commit>`: the commit, the day of the\n\
         # newest result, then one line per published test with the emulators that pass it.\n\
         # Derived from the Shootout's results, MIT License, Copyright (c) 2026 daid; see NOTICE.\n",
    );
    let _ = writeln!(text, "commit\t{commit}");
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a date in seconds since 1970 fits an i64"
    )]
    let day = (newest as i64).div_euclid(SECONDS_PER_DAY);
    let _ = writeln!(text, "published\t{}", civil_date(day));
    for name in names {
        let passing: Vec<&str> = emulators
            .iter()
            .filter(|(_, tests)| {
                tests
                    .get(name)
                    .and_then(|test| test.get("result"))
                    .and_then(Value::as_str)
                    == Some("PASS")
            })
            .map(|(emulator, _)| *emulator)
            .collect();
        let _ = writeln!(text, "{name}\t{}", passing.join(","));
    }
    Ok(text)
}

/// The date `days` days after 1970-01-01, as `YYYY-MM-DD` (Howard Hinnant's
/// `civil_from_days`).
fn civil_date(days: i64) -> String {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests;
