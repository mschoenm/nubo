//! Tests for the control bindings.

use super::*;

#[test]
fn binding_a_key_takes_it_from_its_old_action() {
    let mut keyboard = default_keyboard();

    keyboard.bind(Action::A, Some(KeyCode::KeyX));

    assert_eq!(keyboard.action(KeyCode::KeyX), Some(Action::A));
    assert_eq!(keyboard.input(Action::B), None);
    assert_eq!(keyboard.action(KeyCode::KeyZ), None);
}

#[test]
fn clearing_a_binding_leaves_the_action_without_a_key() {
    let mut keyboard = default_keyboard();

    keyboard.bind(Action::Start, None);

    assert_eq!(keyboard.input(Action::Start), None);
    assert_eq!(keyboard.action(KeyCode::Enter), None);
}

#[test]
fn controls_survive_a_round_trip_through_toml() {
    let mut controls = Controls::default();
    controls.keyboard.bind(Action::A, Some(KeyCode::Numpad5));
    controls.controller.bind(Action::QuickSave, None);

    let contents = toml::to_string(&controls).unwrap();

    assert!(contents.contains("a = \"Numpad5\""));
    assert_eq!(toml::from_str::<Controls>(&contents).unwrap(), controls);
}

#[test]
fn only_known_keys_and_buttons_load() {
    assert!(toml::from_str::<Controls>("[keyboard]\na = \"Escape\"").is_err());
    assert!(toml::from_str::<Controls>("[controller]\na = \"turbo\"").is_err());
}

#[test]
fn escape_and_backspace_cant_be_bound() {
    assert!(KeyCode::try_from(String::from("Escape")).is_err());
    assert!(KeyCode::try_from(String::from("Backspace")).is_err());
}

#[test]
fn every_key_has_a_name_that_loads_back() {
    for &key in KeyCode::ALL {
        assert_eq!(KeyCode::try_from(String::from(key)), Ok(key));
    }
}

#[test]
fn keys_are_saved_by_the_names_earlier_versions_used() {
    let names: Vec<_> = [
        KeyCode::ArrowUp,
        KeyCode::KeyZ,
        KeyCode::Digit0,
        KeyCode::F12,
        KeyCode::ShiftLeft,
        KeyCode::Backquote,
        KeyCode::NumpadEnter,
    ]
    .map(String::from)
    .into();

    assert_eq!(
        names,
        [
            "ArrowUp",
            "KeyZ",
            "Digit0",
            "F12",
            "ShiftLeft",
            "Backquote",
            "NumpadEnter"
        ]
    );
}

#[test]
fn every_controller_button_has_a_name_that_loads_back() {
    for button in ControllerButton::ALL.map(PadButton) {
        assert_eq!(PadButton::try_from(String::from(button)), Ok(button));
    }
}

#[test]
fn rewind_gets_a_key_once_where_the_player_left_it_free() {
    let mut controls: Controls =
        toml::from_str("[keyboard]\nup = \"ArrowUp\"\n[controller]\nb = \"west\"\n").unwrap();

    assert!(add_new_actions(&mut controls));
    assert_eq!(controls.keyboard.input(Action::Rewind), Some(KeyCode::KeyR));
    // The controller's left button already presses B.
    assert_eq!(controls.controller.input(Action::Rewind), None);
    assert!(!add_new_actions(&mut controls));
}
