//! Tests for the nightly metrics log: the regressions it finds and the report
//! it writes.

use super::*;
use serde_json::json;

/// A night with the given speed, instruction count and wake-ups.
fn night(date: &str, speed: f64, instructions: f64, wakeups: f64) -> Value {
    json!({
        "date": date,
        "commit": "abc1234",
        "tests": {"verify_boot": "success", "cpu": "success"},
        "test_roms_passed": 2400,
        "shootout": {"passed": 258, "total": 264, "rank": 1, "leader": "DocBoy",
                     "leader_passed": 255, "published": "2026-06-04", "published_commit": "72fd745"},
        "speed": {"tobudx-dmg": speed},
        "instructions_per_emulated_second": {"tobudx-dmg": instructions},
        "efficiency": {"playing": {"wakeups_per_second": wakeups, "frames_per_second": 60.0}},
    })
}

/// `night` as measured on a machine with the CPU model `cpu`.
fn on_cpu(mut night: Value, cpu: &str) -> Value {
    night["cpu"] = json!(cpu);
    night
}

const SLOW_CPU: &str = "AMD EPYC 7763 64-Core Processor";
const FAST_CPU: &str = "AMD EPYC 9V74 80-Core Processor";

/// Fourteen ordinary nights.
fn ordinary_nights() -> Vec<Value> {
    (1..=14)
        .map(|day| night(&format!("2026-10-{day:02}"), 120.0, 50_000_000.0, 86.0))
        .collect()
}

#[test]
fn an_ordinary_night_has_no_regressions() {
    let earlier = ordinary_nights();
    let latest = night("2026-10-15", 112.0, 50_500_000.0, 88.0);
    assert_eq!(regressions(&latest, &earlier), Vec::<String>::new());
}

#[test]
fn slower_speed_more_instructions_and_more_wakeups_are_regressions() {
    let earlier = ordinary_nights();
    let latest = night("2026-10-15", 100.0, 52_000_000.0, 120.0);
    let found = regressions(&latest, &earlier);
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(found[0].contains("efficiency.playing.wakeups_per_second"));
    assert!(found[1].contains("instructions_per_emulated_second.tobudx-dmg"));
    assert!(found[2].contains("speed.tobudx-dmg"));
}

#[test]
fn more_frames_while_playing_is_not_a_regression() {
    let earlier = ordinary_nights();
    let mut latest = night("2026-10-15", 120.0, 50_000_000.0, 86.0);
    latest["efficiency"]["playing"]["frames_per_second"] = json!(120.0);
    assert!(regressions(&latest, &earlier).is_empty());
}

#[test]
fn a_failed_test_job_and_fewer_passing_test_roms_are_regressions() {
    let earlier = ordinary_nights();
    let mut latest = night("2026-10-15", 120.0, 50_000_000.0, 86.0);
    latest["tests"]["verify_boot"] = json!("failure");
    latest["test_roms_passed"] = json!(2399);
    let found = regressions(&latest, &earlier);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].contains("verify_boot"));
    assert!(found[1].contains("2,400 to 2,399"));
}

/// Fourteen nights: ten on the slow CPU model at 108 times real speed, and
/// four on a twice as fast one among them, as in issue #314.
fn mixed_cpu_nights() -> Vec<Value> {
    (1..=14)
        .map(|day| {
            let date = format!("2026-10-{day:02}");
            if day == 4 || day == 9 || day == 12 || day == 13 {
                on_cpu(night(&date, 210.0, 50_000_000.0, 86.0), FAST_CPU)
            } else {
                on_cpu(night(&date, 108.0, 50_000_000.0, 86.0), SLOW_CPU)
            }
        })
        .collect()
}

#[test]
fn a_slow_night_on_the_slow_cpu_model_is_not_a_regression() {
    let earlier = mixed_cpu_nights();
    let latest = on_cpu(night("2026-10-15", 98.5, 50_000_000.0, 86.0), SLOW_CPU);
    assert_eq!(regressions(&latest, &earlier), Vec::<String>::new());
}

#[test]
fn a_real_drop_on_the_same_cpu_model_is_a_regression() {
    let earlier = mixed_cpu_nights();
    let latest = on_cpu(night("2026-10-15", 90.0, 50_000_000.0, 86.0), SLOW_CPU);
    let found = regressions(&latest, &earlier);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains(
            "`speed.tobudx-dmg`: 90 against a 14-night median on the same CPU model of 108"
        ),
        "{found:?}"
    );
}

#[test]
fn a_slow_night_on_the_fast_cpu_model_is_judged_against_fast_nights() {
    let mut earlier = mixed_cpu_nights();
    earlier.push(on_cpu(
        night("2026-10-15", 205.0, 50_000_000.0, 86.0),
        FAST_CPU,
    ));
    let latest = on_cpu(night("2026-10-16", 180.0, 50_000_000.0, 86.0), FAST_CPU);
    let found = regressions(&latest, &earlier);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("median on the same CPU model of 210"),
        "{found:?}"
    );
}

#[test]
fn speed_is_not_judged_with_too_few_nights_on_the_same_cpu_model() {
    let earlier = ordinary_nights();
    let latest = on_cpu(night("2026-10-15", 50.0, 50_000_000.0, 86.0), SLOW_CPU);
    assert_eq!(regressions(&latest, &earlier), Vec::<String>::new());
    let mut nights = earlier;
    nights.push(latest);
    let page = report(&nights);
    assert!(
        page.contains(
            "Speed isn't judged tonight: only 0 earlier nights ran on AMD EPYC 7763 64-Core Processor, and it takes 5."
        ),
        "{page}"
    );
    assert!(
        page.contains("| speed.tobudx-dmg | 50 |  |  |  |"),
        "{page}"
    );
    assert!(page.contains(&format!("| 2026-10-15 | `abc1234` | {SLOW_CPU} | passed |")));
}

#[test]
fn other_metrics_are_compared_across_cpu_models() {
    let earlier = mixed_cpu_nights();
    let latest = on_cpu(night("2026-10-15", 108.0, 52_000_000.0, 86.0), SLOW_CPU);
    let found = regressions(&latest, &earlier);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("instructions_per_emulated_second.tobudx-dmg"));
}

#[test]
fn medians_take_only_the_latest_nights() {
    let mut earlier = vec![night("2026-09-01", 10.0, 1.0, 1.0); 20];
    earlier.extend(ordinary_nights());
    let earlier: Vec<&Value> = earlier.iter().collect();
    assert_eq!(median_of(&earlier, "speed.tobudx-dmg", 14), Some(120.0));
    assert_eq!(median_of(&earlier, "speed.tobudx-dmg", 90), Some(10.0));
    assert_eq!(median_of(&earlier, "speed.missing", 14), None);
}

#[test]
fn the_report_lists_the_latest_night_and_its_history() {
    let mut nights = ordinary_nights();
    nights.push(night("2026-10-15", 121.5, 50_000_000.0, 86.0));
    let page = report(&nights);
    assert!(page.contains("Latest night: 2026-10-15"));
    assert!(page.contains("None against the 14-night medians."));
    assert!(page.contains("| speed.tobudx-dmg | 121.5 | 120 | 120 | 120 |"));
    assert!(page.contains("| 2026-10-15 | `abc1234` |  | passed | 2,400 | 258 | 121.5 |"));
}

#[test]
fn the_report_starts_with_the_shootout_standing() {
    let mut nights = ordinary_nights();
    nights.push(night("2026-10-15", 121.5, 50_000_000.0, 86.0));
    nights[14]["shootout"]["rank"] = json!(2);
    let page = report(&nights);
    let first_lines: Vec<&str> = page.lines().take(6).collect();
    assert!(
        first_lines.contains(
            &"GB Emulator Shootout: 258 of 264, would rank 2nd (leader: DocBoy, 255; published 2026-06-04)."
        ),
        "{page}"
    );
    assert!(page.contains("| Date | Commit | CPU | Tests | Test ROMs passed | Shootout |"));
}

#[test]
fn a_night_without_the_shootout_leaves_its_line_and_column_empty() {
    let mut latest = night("2026-10-15", 120.0, 50_000_000.0, 86.0);
    latest.as_object_mut().unwrap().remove("shootout");
    let page = report(&[latest]);
    assert!(!page.contains("GB Emulator Shootout"));
    assert!(page.contains("| 2026-10-15 | `abc1234` |  | passed | 2,400 |  | 120 |"));
}

#[test]
fn ranks_are_ordinals() {
    assert_eq!(ordinal(1), "1st");
    assert_eq!(ordinal(2), "2nd");
    assert_eq!(ordinal(3), "3rd");
    assert_eq!(ordinal(4), "4th");
    assert_eq!(ordinal(11), "11th");
    assert_eq!(ordinal(12), "12th");
    assert_eq!(ordinal(13), "13th");
    assert_eq!(ordinal(21), "21st");
    assert_eq!(ordinal(112), "112th");
}

#[test]
fn numbers_are_written_plainly() {
    assert_eq!(format_number(50_000_000.0), "50,000,000");
    assert_eq!(format_number(121.5), "121.5");
    assert_eq!(format_number(86.0), "86");
    assert_eq!(format_number(0.25), "0.25");
}

#[test]
fn a_log_that_cannot_be_read_is_an_error_and_is_kept() {
    let folder =
        std::env::temp_dir().join(format!("nubo-metrics-unreadable-{}", std::process::id()));
    fs::create_dir_all(&folder).unwrap();
    let log = folder.join("metrics.jsonl");
    let night_file = folder.join("night.json");
    fs::write(&log, b"{\"date\": \"\xff\"}\n").unwrap();
    fs::write(&night_file, r#"{"date": "2026-10-23"}"#).unwrap();
    let result = append(&log, &night_file);
    let kept = fs::read(&log);
    fs::remove_dir_all(&folder).unwrap();
    assert!(result.unwrap_err().contains("failed to read"));
    assert_eq!(kept.unwrap(), b"{\"date\": \"\xff\"}\n");
}
