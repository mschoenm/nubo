//! Tests for turning Iced's key codes into Nubo's.

use super::*;

#[test]
fn every_key_that_can_be_bound_has_an_iced_code() {
    for &key in KeyCode::ALL {
        assert!(
            KEY_CODES.iter().any(|&(_, known)| known == key),
            "{key:?} has no Iced key code"
        );
    }
    assert_eq!(KEY_CODES.len(), KeyCode::ALL.len());
}

#[test]
fn keys_keep_the_names_of_iced_s_codes() {
    // Earlier versions saved keys by the name of Iced's code, so the names
    // must stay the same for their settings to load.
    for &(code, key) in KEY_CODES {
        assert_eq!(format!("{code:?}"), key.name());
        assert_eq!(key_code(code), Some(key));
    }
}

#[test]
fn escape_and_backspace_have_no_code_to_bind() {
    assert_eq!(key_code(Code::Escape), None);
    assert_eq!(key_code(Code::Backspace), None);
    assert_eq!(key_code(Code::KeyQ), Some(KeyCode::KeyQ));
}
