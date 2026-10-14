use super::*;

/// A scoreboard of three tests and three emulators.
const SMALL_SCOREBOARD: &str = "# A comment\n\
commit\t72fd7452fb4febd8832c0ca5b30fab431fde767e\n\
published\t2026-06-04\n\
a.gb\tBeaten Dying Moon,DocBoy,SameBoy\n\
b.gb\tDocBoy,SameBoy\n\
c.gb (DMG)\t\n";

#[test]
fn a_screen_must_hold_for_one_screenshot_interval_at_the_harness_speed() {
    // 0.4 emulated seconds are 51.2 audio buffers.
    assert_eq!(MATCH_HOLD_AUDIO_BUFFERS, 52);
}

#[test]
fn every_test_is_in_the_published_table_once() {
    let scoreboard = Scoreboard::parse(SCOREBOARD).unwrap();
    let published: BTreeSet<&str> = scoreboard
        .tests
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    let mut names = BTreeSet::new();
    for test in &TESTS {
        assert!(
            published.contains(test.name),
            "{} isn't published",
            test.name
        );
        assert!(names.insert(test.name), "{} is listed twice", test.name);
    }
}

#[test]
fn the_scoreboard_reads_the_published_table() {
    let scoreboard = Scoreboard::parse(SCOREBOARD).unwrap();
    assert_eq!(scoreboard.tests.len(), 264);
    assert_eq!(scoreboard.passed_by_emulator()["DocBoy"], 255);
    assert_eq!(scoreboard.passed_by_emulator()["SameBoy"], 250);
}

#[test]
fn ties_share_a_rank() {
    let scoreboard = Scoreboard::parse(SMALL_SCOREBOARD).unwrap();
    let standing = |passing: &[&str]| scoreboard.standing(&passing.iter().copied().collect());

    let nothing = standing(&[]);
    assert_eq!((nothing.passed, nothing.total, nothing.rank), (0, 3, 4));
    assert_eq!(standing(&["a.gb"]).rank, 3);
    assert_eq!(standing(&["a.gb", "b.gb"]).rank, 1);
    let everything = standing(&["a.gb", "b.gb", "c.gb (DMG)"]);
    assert_eq!(
        everything.to_string(),
        "shootout  3/3  rank 1  leader DocBoy 2  published 2026-06-04 72fd745"
    );
}

#[test]
fn a_test_runs_with_the_shootouts_defaults() {
    let case = test("blargg/halt_bug.gb", DMG, 2000).test_case();
    assert_eq!(case.id(), "shootout/blargg/halt_bug.gb");
    assert_eq!(case.suite(), "shootout");
    assert_eq!(case.rom, Path::new("blargg/halt_bug.gb"));
    assert!(matches!(case.source, Source::Shootout));
    assert_eq!(case.deadline, Deadline::MillisecondsFromPowerOn(28_000));
    let Check::Shootout { pass, fail } = case.check else {
        panic!("not judged like the Shootout");
    };
    assert_eq!(pass, [Path::new("blargg/halt_bug.png")]);
    assert!(fail.is_empty());
}

#[test]
fn a_test_can_take_its_rom_from_the_collection_and_name_its_screenshots() {
    let case = test("daid/stop_instr.gb (DMG)", DMG, 500)
        .rom("daid/stop_instr.gb")
        .in_collection("daid/stop_instr_copy.gb")
        .results(&["daid/stop_instr.dmg.png"])
        .fail_results(&["daid/stop_instr.dmg.fail.png"])
        .test_case();
    assert_eq!(case.id(), "shootout/daid/stop_instr.gb (DMG)");
    assert_eq!(case.rom, Path::new("daid/stop_instr_copy.gb"));
    assert!(matches!(case.source, Source::Collection));
    let Check::Shootout { pass, fail } = case.check else {
        panic!("not judged like the Shootout");
    };
    assert_eq!(pass, [Path::new("daid/stop_instr.dmg.png")]);
    assert_eq!(fail, [Path::new("daid/stop_instr.dmg.fail.png")]);
}

#[test]
fn a_filtered_run_has_no_standing() {
    let run_ids = BTreeSet::from([String::from("shootout/blargg/halt_bug.gb")]);
    assert_eq!(standing(&run_ids, &BTreeSet::new()), Ok(None));
}
