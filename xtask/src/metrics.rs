//! `cargo xtask metrics`: the nightly metrics log, one JSON object per line
//! (`metrics.jsonl` in the repository's wiki, see `docs/NIGHTLY_REVIEW.md`).
//! `append` adds a night to it, and `report` writes the "Nightly metrics"
//! wiki page: the latest night against the medians of the nights before it,
//! the values that regressed, and the last 30 nights.
//!
//! A night looks like `{"date": "2026-10-14", "commit": "90d6ef3", "tests":
//! {"verify_boot": "success", ...}, "test_roms_passed": 2412, "speed":
//! {"tobudx-dmg": 118.1, ...}, ...}`: every number in it, however deep, is
//! a metric named by its path, such as `speed.tobudx-dmg`.
//!
//! Its `shootout` holds Nubo's score on the GB Emulator Shootout's published
//! tests and the rank it would have there, with the publication it was
//! counted against: `{"passed": 258, "total": 264, "rank": 1, "leader":
//! "DocBoy", "leader_passed": 255, "published": "2026-06-04",
//! "published_commit": "72fd745"}`. The page's first lines show it.

use crate::Result;
use clap::Subcommand;
use serde_json::Value;
use std::fmt::Write;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// How many nights before the latest one each median covers, shortest
/// window first.
const MEDIAN_NIGHTS: [usize; 3] = [14, 90, 365];
/// How many nights the history table shows.
const HISTORY_NIGHTS: usize = 30;
/// The metrics the history table shows, if a night has them.
const HISTORY_COLUMNS: [(&str, &str); 7] = [
    ("test_roms_passed", "Test ROMs passed"),
    ("shootout.passed", "Shootout"),
    ("speed.tobudx-dmg", "Speed DMG"),
    ("speed.tobudx-cgb", "Speed CGB"),
    (
        "instructions_per_emulated_second.tobudx-dmg",
        "Instructions/s DMG",
    ),
    ("efficiency.playing.wakeups_per_second", "Wake-ups/s"),
    ("size_bytes.wasm", "Wasm size"),
];

/// Which way a metric gets worse, and how far it may move from its 14-night
/// median before it counts as a regression.
enum Threshold {
    /// Worse when lower, by more than this share.
    Lower(f64),
    /// Worse when higher, by more than this share and by more than one.
    Higher(f64),
    /// Not judged: it changes with the game, not with the code's quality.
    None,
}

/// The threshold for the metric named `key`. The test ROM counts, the
/// Shootout's among them, have none: a test that stops passing already fails
/// CI, and the history shows the trend.
fn threshold(key: &str) -> Threshold {
    if key.starts_with("speed.") {
        // Shared cloud machines vary by 10% from night to night.
        Threshold::Lower(0.10)
    } else if key.starts_with("instructions_per_emulated_second.") {
        Threshold::Higher(0.02)
    } else if key.starts_with("size_bytes.") {
        Threshold::Higher(0.05)
    } else if key.ends_with("wakeups_per_second")
        || key.ends_with("buffers_per_second")
        || key == "efficiency.still_screen.frames_per_second"
        || key == "efficiency.paused.frames_per_second"
    {
        Threshold::Higher(0.10)
    } else {
        Threshold::None
    }
}

/// What `cargo xtask metrics` can do. The doc comments are the help that
/// `--help` prints.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Add a night to the nightly metrics log
    Append {
        /// The log, one night of JSON per line
        #[arg(value_name = "METRICS.JSONL")]
        log: PathBuf,
        /// The night to add, as JSON with its date
        #[arg(value_name = "NIGHT.JSON")]
        night: PathBuf,
    },
    /// Print the Nightly metrics wiki page, made from the log, as Markdown
    Report {
        /// The log, one night of JSON per line
        #[arg(value_name = "METRICS.JSONL")]
        log: PathBuf,
    },
}

/// Runs `append <log> <night.json>` or `report <log>`.
pub fn run(command: Command) -> Result {
    match command {
        Command::Append { log, night } => append(&log, &night),
        Command::Report { log } => {
            print!("{}", report(&read_log(&log)?));
            Ok(())
        }
    }
}

/// Adds the night in `night` to the log at `log` as one line. A missing log
/// is started; one that can't be read is an error, so it is never replaced.
fn append(log: &Path, night: &Path) -> Result {
    let text = fs::read_to_string(night)
        .map_err(|error| format!("failed to read {}: {error}", night.display()))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not valid JSON: {error}", night.display()))?;
    if value.get("date").and_then(Value::as_str).is_none() {
        return Err(format!("{} has no date", night.display()));
    }

    let mut lines = match fs::read_to_string(log) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("failed to read {}: {error}", log.display())),
    };
    if !lines.is_empty() && !lines.ends_with('\n') {
        lines.push('\n');
    }
    lines.push_str(&value.to_string());
    lines.push('\n');
    fs::write(log, lines).map_err(|error| format!("failed to write {}: {error}", log.display()))
}

/// The nights in the log at `log`, oldest first.
fn read_log(log: &Path) -> Result<Vec<Value>> {
    let text = fs::read_to_string(log)
        .map_err(|error| format!("failed to read {}: {error}", log.display()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
        .map(|(index, line)| {
            serde_json::from_str(line).map_err(|error| {
                format!(
                    "line {} of {} is not valid JSON: {error}",
                    index + 1,
                    log.display()
                )
            })
        })
        .collect()
}

/// The "Nightly metrics" page for `nights`, oldest first, in Markdown.
fn report(nights: &[Value]) -> String {
    let mut page = String::from("# Nightly metrics\n\n");
    let Some((latest, earlier)) = nights.split_last() else {
        page.push_str("No nights recorded yet.\n");
        return page;
    };

    let _ = writeln!(
        page,
        "Latest night: {} at commit `{}`. Written by the nightly CI; see `docs/NIGHTLY_REVIEW.md` in the repository.\n",
        text(latest, "date"),
        text(latest, "commit"),
    );
    if let Some(line) = shootout_line(latest) {
        let _ = writeln!(page, "{line}\n");
    }

    let regressions = regressions(latest, earlier);
    page.push_str("## Regressions\n\n");
    if regressions.is_empty() {
        let _ = writeln!(
            page,
            "None against the {}-night medians.\n",
            MEDIAN_NIGHTS[0]
        );
    } else {
        for regression in &regressions {
            let _ = writeln!(page, "- {regression}");
        }
        page.push('\n');
    }

    page.push_str("## Latest night\n\n| Metric | Latest |");
    for nights in MEDIAN_NIGHTS {
        let _ = write!(page, " {nights}-night median |");
    }
    page.push_str("\n| --- | --- |");
    for _ in MEDIAN_NIGHTS {
        page.push_str(" --- |");
    }
    page.push('\n');
    for (key, value) in numbers(latest) {
        let _ = write!(page, "| {key} | {} |", format_number(value));
        for nights in MEDIAN_NIGHTS {
            let median = median_of(earlier, &key, nights);
            let _ = write!(page, " {} |", median.map_or(String::new(), format_number));
        }
        page.push('\n');
    }
    let no_medians = " |".repeat(MEDIAN_NIGHTS.len());
    for (key, result) in test_results(latest) {
        let _ = writeln!(page, "| {key} | {result} |{no_medians}");
    }

    let _ = write!(
        page,
        "\n## Last {HISTORY_NIGHTS} nights\n\n| Date | Commit | Tests |"
    );
    for (_, title) in HISTORY_COLUMNS {
        let _ = write!(page, " {title} |");
    }
    page.push_str("\n| --- | --- | --- |");
    for _ in HISTORY_COLUMNS {
        page.push_str(" --- |");
    }
    page.push('\n');
    for night in nights.iter().rev().take(HISTORY_NIGHTS) {
        let failed = test_results(night)
            .into_iter()
            .filter(|(_, result)| result != "success")
            .count();
        let tests = if failed == 0 {
            String::from("passed")
        } else {
            format!("{failed} failed")
        };
        let _ = write!(
            page,
            "| {} | `{}` | {tests} |",
            text(night, "date"),
            text(night, "commit")
        );
        for (key, _) in HISTORY_COLUMNS {
            let value = number(night, key);
            let _ = write!(page, " {} |", value.map_or(String::new(), format_number));
        }
        page.push('\n');
    }
    page
}

/// Nubo's standing in the GB Emulator Shootout on `night`, such as "GB
/// Emulator Shootout: 258 of 264, would rank 1st (leader: DocBoy, 255;
/// published 2026-06-04).", or `None` if the night has none.
fn shootout_line(night: &Value) -> Option<String> {
    let shootout = night.get("shootout")?;
    let count = |key: &str| shootout.get(key).and_then(Value::as_u64);
    Some(format!(
        "GB Emulator Shootout: {} of {}, would rank {} (leader: {}, {}; published {}).",
        count("passed")?,
        count("total")?,
        ordinal(count("rank")?),
        text(shootout, "leader"),
        count("leader_passed")?,
        text(shootout, "published"),
    ))
}

/// `number` as an English ordinal, such as "1st", "2nd", "11th" or "23rd".
fn ordinal(number: u64) -> String {
    let suffix = match (number % 10, number % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{number}{suffix}")
}

/// What regressed in `latest` against the nights before it, as sentences.
fn regressions(latest: &Value, earlier: &[Value]) -> Vec<String> {
    let mut found = Vec::new();
    for (key, result) in test_results(latest) {
        if result != "success" {
            found.push(format!("Test job `{key}`: {result}"));
        }
    }
    if let (Some(passed), Some(before)) = (
        number(latest, "test_roms_passed"),
        earlier
            .last()
            .and_then(|night| number(night, "test_roms_passed")),
    ) && passed < before
    {
        found.push(format!(
            "Test ROMs passed fell from {} to {}",
            format_number(before),
            format_number(passed)
        ));
    }
    for (key, value) in numbers(latest) {
        let Some(median) = median_of(earlier, &key, MEDIAN_NIGHTS[0]) else {
            continue;
        };
        let worse = match threshold(&key) {
            Threshold::Lower(share) => value < median * (1.0 - share),
            Threshold::Higher(share) => value > median * (1.0 + share) && value > median + 1.0,
            Threshold::None => false,
        };
        if worse {
            found.push(format!(
                "`{key}`: {} against a {}-night median of {}",
                format_number(value),
                MEDIAN_NIGHTS[0],
                format_number(median)
            ));
        }
    }
    found
}

/// The median of the metric `key` over the last `nights` of `earlier`, or
/// `None` if none of them has it.
fn median_of(earlier: &[Value], key: &str, nights: usize) -> Option<f64> {
    let mut values: Vec<f64> = earlier
        .iter()
        .rev()
        .take(nights)
        .filter_map(|night| number(night, key))
        .collect();
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        f64::midpoint(values[middle - 1], values[middle])
    } else {
        values[middle]
    })
}

/// Every number in `night` with its path, such as `speed.tobudx-dmg`, in a
/// fixed order.
fn numbers(night: &Value) -> Vec<(String, f64)> {
    let mut found = Vec::new();
    collect_numbers(night, "", &mut found);
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// Adds every number under `value` to `found`, named by its dotted path
/// from `path`.
fn collect_numbers(value: &Value, path: &str, found: &mut Vec<(String, f64)>) {
    match value {
        Value::Number(number) => {
            if let Some(number) = number.as_f64() {
                found.push((path.to_string(), number));
            }
        }
        Value::Object(map) => {
            for (key, child) in map {
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                collect_numbers(child, &child_path, found);
            }
        }
        _ => {}
    }
}

/// The results of the test jobs in `night`, such as `("verify_boot",
/// "success")`.
fn test_results(night: &Value) -> Vec<(String, String)> {
    night
        .get("tests")
        .and_then(Value::as_object)
        .map(|tests| {
            tests
                .iter()
                .map(|(key, result)| {
                    (
                        key.clone(),
                        result.as_str().unwrap_or("unknown").to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The number at the dotted path `key` in `night`.
fn number(night: &Value, key: &str) -> Option<f64> {
    key.split('.')
        .try_fold(night, |value, part| value.get(part))
        .and_then(Value::as_f64)
}

/// The text at `key` in `night`, or an empty string.
fn text<'a>(night: &'a Value, key: &str) -> &'a str {
    night.get(key).and_then(Value::as_str).unwrap_or("")
}

/// `value` without trailing zeros, grouped in thousands when it is large.
fn format_number(value: f64) -> String {
    if value.abs() >= 1000.0 {
        let whole = value.round() as i64;
        let digits = whole.unsigned_abs().to_string();
        let mut grouped = String::new();
        for (index, digit) in digits.chars().enumerate() {
            if index > 0 && (digits.len() - index).is_multiple_of(3) {
                grouped.push(',');
            }
            grouped.push(digit);
        }
        if whole < 0 {
            grouped.insert(0, '-');
        }
        grouped
    } else {
        let text = format!("{value:.2}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests;
