use super::*;
use serde_json::json;

#[test]
fn the_scoreboard_lists_who_passes_each_published_test() {
    let tests = json!([
        {"name": "acid/which.gb (DMG)", "description": null, "url": null},
        {"name": "mooneye/acceptance/div_timing.gb", "description": null, "url": null},
    ]);
    let results = [
        json!({"emulator": "SameBoy", "date": 1_780_589_810.5, "tests": {
            "acid/which.gb (DMG)": {"result": "INFO"},
            "mooneye/acceptance/div_timing.gb": {"result": "PASS"},
        }}),
        json!({"emulator": "bgb", "date": 1_780_500_000.0, "tests": {
            "mooneye/acceptance/div_timing.gb": {"result": "PASS"},
            "an/unpublished/test.gb": {"result": "PASS"},
        }}),
        json!({"emulator": "Emmy", "date": 1_780_000_000.0, "tests": {
            "mooneye/acceptance/div_timing.gb": {"result": "FAIL"},
        }}),
    ];
    let text = scoreboard("72fd7452fb4febd8832c0ca5b30fab431fde767e", &tests, &results).unwrap();
    let lines: Vec<&str> = text.lines().filter(|line| !line.starts_with('#')).collect();
    assert_eq!(
        lines,
        [
            "commit\t72fd7452fb4febd8832c0ca5b30fab431fde767e",
            "published\t2026-06-04",
            "acid/which.gb (DMG)\t",
            "mooneye/acceptance/div_timing.gb\tbgb,SameBoy",
        ]
    );
}

#[test]
fn days_since_1970_become_dates() {
    assert_eq!(civil_date(0), "1970-01-01");
    assert_eq!(civil_date(20_608), "2026-06-04");
    assert_eq!(civil_date(11_016), "2000-02-29");
    assert_eq!(civil_date(-1), "1969-12-31");
}

#[test]
fn file_names_are_percent_encoded() {
    assert_eq!(percent_encode("no$gmb.json"), "no%24gmb.json");
    assert_eq!(
        percent_encode("visualboyadvance-m.json"),
        "visualboyadvance-m.json"
    );
}
