//! Tests for reading family names from the desktop's font settings.

use super::*;

#[test]
fn gnome_names_drop_the_size_and_then_a_word_at_a_time() {
    assert_eq!(family_names("Cantarell 11"), ["Cantarell"]);
    assert_eq!(
        family_names("Noto Sans Medium 10.5"),
        ["Noto Sans Medium", "Noto Sans", "Noto"]
    );
    assert_eq!(
        family_names("Adwaita Sans 11px"),
        ["Adwaita Sans", "Adwaita"]
    );
    assert_eq!(family_names("Cantarell, Sans 11"), ["Cantarell"]);
}

#[test]
fn kde_names_end_at_the_first_comma() {
    assert_eq!(
        family_names("Noto Sans,10,-1,5,50,0,0,0,0,0"),
        ["Noto Sans", "Noto"]
    );
}

#[test]
fn an_empty_setting_names_nothing() {
    assert!(family_names("").is_empty());
    assert!(family_names("11").is_empty());
}
