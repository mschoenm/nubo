//! Tests for checking cheat codes.

use super::*;

#[test]
fn gameshark_codes_are_recognized() {
    let cheat = Cheat::new("Infinite health", "01ff56d0").unwrap();

    assert_eq!(cheat.code, "01FF56D0");
    assert_eq!(cheat.kind(), CheatKind::GameShark);
    assert!(cheat.enabled);
}

#[test]
fn game_genie_codes_are_recognized() {
    let cheat = Cheat::new("", " 00a-17b-c49 ").unwrap();

    assert_eq!(cheat.code, "00A-17B-C49");
    assert_eq!(cheat.kind(), CheatKind::GameGenie);
}

#[test]
fn game_genie_codes_get_their_dashes() {
    assert_eq!(Cheat::new("", "00a17bc49").unwrap().code, "00A-17B-C49");
    assert_eq!(Cheat::new("", "CED 56A").unwrap().code, "CED-56A");
}

#[test]
fn a_cheat_without_a_name_is_named_after_its_code() {
    assert_eq!(Cheat::new("  ", "01FF56D0").unwrap().name, "01FF56D0");
}

#[test]
fn invalid_codes_are_rejected() {
    assert!(Cheat::new("Nothing", "").is_err());
    assert!(Cheat::new("Nothing", "XYZ").is_err());
    assert!(Cheat::new("Nothing", "00A-17").is_err());
    assert!(Cheat::new("Nothing", "01FF0080").is_err());
}

#[test]
fn a_cheat_can_have_several_codes() {
    let cheat = Cheat::new("Max money", "01ff56d0 + 01fe57d0, 00A17BC49").unwrap();

    assert_eq!(cheat.code, "01FF56D0+01FE57D0+00A-17B-C49");
    assert_eq!(
        cheat.codes().collect::<Vec<_>>(),
        ["01FF56D0", "01FE57D0", "00A-17B-C49"]
    );
    assert_eq!(cheat.kind(), CheatKind::GameShark);
}

#[test]
fn codes_can_be_apart_with_spaces() {
    assert_eq!(
        Cheat::new("", "01FF56D0 01FE57D0").unwrap().code,
        "01FF56D0+01FE57D0"
    );
    assert_eq!(Cheat::new("", "CED 56A").unwrap().code, "CED-56A");
}

#[test]
fn one_bad_code_rejects_the_cheat() {
    assert!(Cheat::new("", "01FF56D0 + XYZ").is_err());
    assert!(Cheat::new("", " + ").is_err());
}
