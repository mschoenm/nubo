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

#[test]
fn cheats_that_share_a_code_keep_their_own_ids() {
    let mut emulator = Emulator::new(nubo_core::Model::Dmg);
    // Both make 0xD056 read 0x5A.
    let both = Cheat::new("", "015A56D0 + 01FE57D0").unwrap();
    let mut one = Cheat::new("", "015A56D0").unwrap();
    both.apply_to(&mut emulator);
    one.apply_to(&mut emulator);

    one.enabled = false;
    one.apply_to(&mut emulator);

    assert_eq!(emulator.read_memory(0xD056), 0x5A);
}

#[cfg(feature = "desktop")]
#[test]
fn an_unreadable_list_is_kept_aside_before_a_new_one_can_replace_it() {
    use crate::storage::files::Files;
    use std::fs;

    let folder = std::env::temp_dir().join(format!("nubo-cheats-kept-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    let files = Files::in_folders(None, Some(folder.clone()));
    let rom_path = Path::new("roms/Game.gb");
    let from_newer_nubo = "[[cheats]]\nname = \"Health\"\ncode = \"01FF56D0\"\n";
    files
        .write(Entry::Cheats(rom_path), from_newer_nubo.as_bytes())
        .unwrap();

    let error = load_from(&files, rom_path).unwrap_err();
    assert!(error.problem.contains("Game.gb.kept-1.toml"), "{error}");
    assert!(error.saving_is_safe);
    assert_eq!(load_from(&files, rom_path), Ok(Vec::new()));
    let cheats = vec![Cheat::new("", "01ff56d0").unwrap()];
    save_to(&files, rom_path, &cheats).unwrap();

    assert_eq!(load_from(&files, rom_path), Ok(cheats));
    assert_eq!(
        fs::read_to_string(folder.join("cheats/Game.gb.kept-1.toml")).unwrap(),
        from_newer_nubo
    );
    let _ = fs::remove_dir_all(&folder);
}

/// A storage whose cheats list is `contents`, or can't be read when that is
/// `None`, and can't be kept aside.
struct Unkeepable {
    contents: Option<&'static [u8]>,
}

impl Storage for Unkeepable {
    fn read(&self, _entry: Entry<'_>) -> Result<Option<Vec<u8>>, String> {
        self.contents
            .map(|contents| Some(contents.to_vec()))
            .ok_or_else(|| String::from("permission denied"))
    }

    fn write(&self, _entry: Entry<'_>, _contents: &[u8]) -> Result<(), String> {
        Ok(())
    }

    fn location(&self, entry: Entry<'_>) -> String {
        entry.file_name().display().to_string()
    }

    fn delete_battery_saves(&self) -> Result<(), String> {
        Ok(())
    }

    fn keep_aside(&self, _entry: Entry<'_>) -> Result<String, String> {
        Err(String::from("the file is in use"))
    }
}

#[test]
fn a_list_that_cant_be_kept_aside_is_not_safe_to_save_over() {
    let storage = Unkeepable {
        contents: Some(b"cheats = 3"),
    };

    let error = load_from(&storage, Path::new("Game.gb")).unwrap_err();

    assert!(!error.saving_is_safe);
    assert!(error.problem.contains("the file is in use"), "{error}");
}

#[test]
fn a_list_that_cant_be_read_is_not_safe_to_save_over() {
    let storage = Unkeepable { contents: None };

    let error = load_from(&storage, Path::new("Game.gb")).unwrap_err();

    assert!(!error.saving_is_safe);
    assert!(error.problem.contains("permission denied"), "{error}");
}

#[test]
fn cheats_read_from_a_file_are_checked_and_put_in_form() {
    let file = br#"
[[cheats]]
name = "Lowercase"
code = "01ff56d0"
enabled = true

[[cheats]]
name = "No dashes"
code = "00a17bc49 cED56a"
enabled = true

[[cheats]]
name = "Too short"
code = "01FF56"
enabled = true
"#;

    let cheats = parse_list(file.to_vec()).unwrap();

    assert_eq!(cheats[0].code, "01FF56D0");
    assert!(cheats[0].enabled);
    assert_eq!(cheats[1].code, "00A-17B-C49+CED-56A");
    assert!(cheats[1].enabled);
    assert_eq!(cheats[2].code, "01FF56");
    assert!(!cheats[2].enabled, "a code that isn't one stays off");
    assert!(cheats[2].check().is_err());
}
