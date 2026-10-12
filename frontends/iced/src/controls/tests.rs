//! Tests for the control bindings.

use super::*;

#[test]
fn binding_a_key_takes_it_from_its_old_action() {
    let mut keyboard = default_keyboard();

    keyboard.bind(Action::A, Some(KeyboardKey(Code::KeyX)));

    assert_eq!(keyboard.action(KeyboardKey(Code::KeyX)), Some(Action::A));
    assert_eq!(keyboard.input(Action::B), None);
    assert_eq!(keyboard.action(KeyboardKey(Code::KeyZ)), None);
}

#[test]
fn clearing_a_binding_leaves_the_action_without_a_key() {
    let mut keyboard = default_keyboard();

    keyboard.bind(Action::Start, None);

    assert_eq!(keyboard.input(Action::Start), None);
    assert_eq!(keyboard.action(KeyboardKey(Code::Enter)), None);
}

#[test]
fn controls_survive_a_round_trip_through_toml() {
    let mut controls = Controls::default();
    controls
        .keyboard
        .bind(Action::A, Some(KeyboardKey(Code::Numpad5)));
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
    assert!(KeyboardKey::from_code(Code::Escape).is_none());
    assert!(KeyboardKey::from_code(Code::Backspace).is_none());
    assert!(KeyboardKey::from_code(Code::KeyQ).is_some());
}

#[test]
fn every_controller_button_has_a_name_that_loads_back() {
    for button in ControllerButton::ALL.map(PadButton) {
        assert_eq!(PadButton::try_from(String::from(button)), Ok(button));
    }
}
