//! Tests for keeping buttons pressed in the menus from the game.

use super::*;

fn pressing(button: ControllerButton) -> ControllerState {
    let mut state = ControllerState::new();
    state.set_button(button, true);
    state
}

fn stick(right: f32, up: f32) -> ControllerState {
    let mut state = ControllerState::new();
    state.set_left_stick(right, up);
    state
}

#[test]
fn a_button_pressed_in_the_menus_is_hidden_until_it_is_released() {
    let mut presses = MenuPresses::default();
    let released = ControllerState::new();
    let held = pressing(ControllerButton::South);

    presses.update(&[released], &[held], true);
    // The game resumes with the button still held.
    presses.update(&[held], &[held], false);
    assert_eq!(presses.hide_from(0, &held), released);

    presses.update(&[held], &[released], false);
    presses.update(&[released], &[held], false);
    assert_eq!(presses.hide_from(0, &held), held);
}

#[test]
fn a_button_held_since_before_the_menus_still_reaches_the_game() {
    let mut presses = MenuPresses::default();
    let held = pressing(ControllerButton::DPadRight);

    presses.update(&[ControllerState::new()], &[held], false);
    presses.update(&[held], &[held], true);

    assert_eq!(presses.hide_from(0, &held), held);
}

#[test]
fn a_stick_direction_pushed_in_the_menus_is_hidden_until_the_stick_lets_go() {
    let mut presses = MenuPresses::default();
    let right_and_up = stick(1.0, 1.0);

    presses.update(&[stick(0.0, 1.0)], &[right_and_up], true);

    assert_eq!(presses.hide_from(0, &right_and_up), stick(0.0, 1.0));

    presses.update(&[right_and_up], &[stick(0.0, 1.0)], false);
    presses.update(&[stick(0.0, 1.0)], &[right_and_up], false);
    assert_eq!(presses.hide_from(0, &right_and_up), right_and_up);
}

#[test]
fn a_button_one_controller_holds_from_the_menus_still_reaches_the_game_from_another() {
    let mut presses = MenuPresses::default();
    let released = ControllerState::new();
    let held = pressing(ControllerButton::South);

    presses.update(&[released, released], &[released, held], true);
    presses.update(&[released, held], &[held, held], false);

    assert_eq!(presses.hide_from(0, &held), held);
    assert_eq!(presses.hide_from(1, &held), released);
}

#[test]
fn a_controller_connected_later_holds_nothing_back() {
    let mut presses = MenuPresses::default();
    let held = pressing(ControllerButton::South);

    presses.update(&[], &[ControllerState::new()], false);
    presses.update(
        &[ControllerState::new()],
        &[ControllerState::new(), held],
        false,
    );

    assert_eq!(presses.hide_from(1, &held), held);
    assert_eq!(presses.hide_from(5, &held), held);
}
