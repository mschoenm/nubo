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
    controls.controller.bind(
        Action::QuickSave,
        Some(PadButton(ControllerButton::RightTrigger)),
    );

    let contents = toml::to_string(&controls).unwrap();

    assert!(contents.contains("a = \"Numpad5\""));
    assert_eq!(toml::from_str::<Controls>(&contents).unwrap(), controls);
}

#[test]
fn the_face_buttons_other_than_a_and_b_are_left_for_the_player() {
    let controller = default_controller();

    for button in [ControllerButton::West, ControllerButton::North] {
        assert_eq!(controller.action(PadButton(button)), None);
    }
    assert_eq!(
        controller.input(Action::Rewind),
        Some(PadButton(ControllerButton::RightShoulder))
    );
    assert_eq!(
        controller.input(Action::FastForward),
        Some(PadButton(ControllerButton::RightTrigger))
    );
}

#[test]
fn the_menu_buttons_follow_nintendo_unless_the_player_picks_xbox() {
    let nintendo = Controls::default().menu_buttons;
    assert_eq!(nintendo, MenuButtons::Nintendo);
    assert_eq!(nintendo.action(ControllerButton::East), Some(Action::A));
    assert_eq!(nintendo.action(ControllerButton::South), Some(Action::B));

    let xbox = MenuButtons::Xbox;
    assert_eq!(xbox.action(ControllerButton::South), Some(Action::A));
    assert_eq!(xbox.action(ControllerButton::East), Some(Action::B));

    for buttons in MenuButtons::ALL {
        assert_eq!(buttons.action(ControllerButton::Start), Some(Action::Start));
        assert_eq!(
            buttons.action(ControllerButton::DPadLeft),
            Some(Action::Left)
        );
        assert_eq!(buttons.action(ControllerButton::West), None);
        assert_eq!(buttons.action(ControllerButton::LeftShoulder), None);
    }
}

#[test]
fn resetting_the_controller_puts_back_the_menu_buttons() {
    let mut controls = Controls {
        menu_buttons: MenuButtons::Xbox,
        ..Controls::default()
    };

    controls.reset(Device::Keyboard);
    assert_eq!(controls.menu_buttons, MenuButtons::Xbox);
    controls.reset(Device::Controller);
    assert_eq!(controls.menu_buttons, MenuButtons::Nintendo);
}

#[test]
fn the_menu_buttons_are_saved_only_once_changed() {
    let saved = toml::to_string(&Controls::default()).unwrap();
    assert!(!saved.contains("menu_buttons"), "{saved}");

    let controls = Controls {
        menu_buttons: MenuButtons::Xbox,
        ..Controls::default()
    };
    let saved = toml::to_string(&controls).unwrap();
    assert!(saved.contains("menu_buttons = \"xbox\""), "{saved}");
    assert_eq!(toml::from_str::<Controls>(&saved).unwrap(), controls);
}

#[test]
fn the_pause_menu_keeps_its_controller_button() {
    let mut controls = Controls::default();

    controls.clear(Device::Controller, Action::PauseMenu);
    assert!(controls.controller.input(Action::PauseMenu).is_some());

    controls.clear(Device::Keyboard, Action::Rewind);
    assert_eq!(controls.keyboard.input(Action::Rewind), None);
}

#[test]
fn taking_the_pause_menu_s_button_swaps_the_two() {
    let mut controls = Controls::default();
    let l1 = PadButton(ControllerButton::LeftShoulder);

    controls.bind_button(Action::Rewind, l1);

    assert_eq!(controls.controller.input(Action::Rewind), Some(l1));
    assert_eq!(
        controls.controller.input(Action::PauseMenu),
        Some(PadButton(ControllerButton::RightShoulder))
    );
}

#[test]
fn taking_the_pause_menu_s_button_for_an_unset_action_moves_it_to_a_free_one() {
    let mut controls = Controls::default();
    let l1 = PadButton(ControllerButton::LeftShoulder);

    controls.bind_button(Action::QuickSave, l1);

    assert_eq!(controls.controller.input(Action::QuickSave), Some(l1));
    assert_eq!(
        controls.controller.input(Action::PauseMenu),
        Some(PadButton(ControllerButton::LeftTrigger))
    );
}

#[test]
fn controls_saved_without_a_pause_menu_button_get_a_free_one() {
    let mut controls: Controls =
        toml::from_str("[controller]\nrewind = \"left_shoulder\"\n").unwrap();

    assert!(controls.give_pause_menu_a_button());
    assert_eq!(
        controls.controller.input(Action::PauseMenu),
        Some(PadButton(ControllerButton::RightShoulder))
    );
    assert!(!controls.give_pause_menu_a_button());
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
