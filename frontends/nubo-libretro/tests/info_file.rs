//! Checks the core info file (`nubo_libretro.info`) RetroArch reads to know
//! what the core supports before loading it.

const INFO: &str = include_str!("../nubo_libretro.info");

/// The value of `key` in the info file.
fn value(key: &str) -> Option<&'static str> {
    INFO.lines().find_map(|line| {
        let (name, value) = line.split_once(" = ")?;
        (name.trim() == key).then(|| value.trim().trim_matches('"'))
    })
}

#[test]
fn the_version_is_nubos() {
    assert_eq!(value("display_version"), Some(env!("CARGO_PKG_VERSION")));
}

#[test]
fn the_names_and_files_match_what_the_core_says() {
    assert_eq!(value("corename"), Some("Nubo"));
    assert_eq!(value("supported_extensions"), Some("gb|gbc"));
    assert_eq!(value("needs_fullpath"), Some("false"));
    assert_eq!(value("systemid"), Some("game_boy"));
    assert_eq!(value("firmware_count"), Some("0"));
}

/// RetroArch only offers rewind for `serialized` save states, and run-ahead
/// and netplay for `deterministic` ones.
#[test]
fn save_states_count_as_deterministic_so_run_ahead_and_netplay_work() {
    assert_eq!(value("savestate"), Some("true"));
    assert_eq!(value("savestate_features"), Some("deterministic"));
    assert_eq!(value("memory_descriptors"), Some("true"));
    assert_eq!(value("cheats"), Some("true"));
}

#[test]
fn every_line_is_a_comment_or_a_quoted_value() {
    for line in INFO
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let (key, value) = line.split_once(" = ").unwrap_or_else(|| panic!("{line}"));
        assert!(!key.contains(' '), "{line}");
        let is_number = value.parse::<u32>().is_ok();
        assert!(
            is_number || (value.starts_with('"') && value.ends_with('"')),
            "{line}"
        );
    }
}
