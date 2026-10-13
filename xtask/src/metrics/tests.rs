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

#[test]
fn medians_take_only_the_latest_nights() {
    let mut earlier = vec![night("2026-09-01", 10.0, 1.0, 1.0); 20];
    earlier.extend(ordinary_nights());
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
    assert!(page.contains("| 2026-10-15 | `abc1234` | passed | 2,400 | 258 | 121.5 |"));
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
    assert!(page.contains("| Date | Commit | Tests | Test ROMs passed | Shootout |"));
}

#[test]
fn a_night_without_the_shootout_leaves_its_line_and_column_empty() {
    let mut latest = night("2026-10-15", 120.0, 50_000_000.0, 86.0);
    latest.as_object_mut().unwrap().remove("shootout");
    let page = report(&[latest]);
    assert!(!page.contains("GB Emulator Shootout"));
    assert!(page.contains("| 2026-10-15 | `abc1234` | passed | 2,400 |  | 120 |"));
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
