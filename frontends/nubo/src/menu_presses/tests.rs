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

/// One controller doing what `state` says.
fn one(state: ControllerState) -> Controllers {
    Controllers::from(vec![state])
}

/// The identity of the controller at `index` of `controllers`.
fn id(controllers: &Controllers, index: usize) -> ControllerId {
    controllers.ids()[index]
}

#[test]
fn a_button_pressed_in_the_menus_is_hidden_until_it_is_released() {
    let mut presses = MenuPresses::default();
    let released = one(ControllerState::new());
    let held = one(pressing(ControllerButton::South));
    let first = id(&held, 0);

    presses.update(&released, &held, true);
    // The game resumes with the button still held.
    presses.update(&held, &held, false);
    assert_eq!(
        presses.hide_from(first, &held.each()[0]),
        ControllerState::new()
    );

    presses.update(&held, &released, false);
    presses.update(&released, &held, false);
    assert_eq!(presses.hide_from(first, &held.each()[0]), held.each()[0]);
}

#[test]
fn a_button_held_since_before_the_menus_still_reaches_the_game() {
    let mut presses = MenuPresses::default();
    let held = one(pressing(ControllerButton::DPadRight));

    presses.update(&one(ControllerState::new()), &held, false);
    presses.update(&held, &held, true);

    assert_eq!(
        presses.hide_from(id(&held, 0), &held.each()[0]),
        held.each()[0]
    );
}

#[test]
fn a_stick_direction_pushed_in_the_menus_is_hidden_until_the_stick_lets_go() {
    let mut presses = MenuPresses::default();
    let up = one(stick(0.0, 1.0));
    let right_and_up = one(stick(1.0, 1.0));
    let first = id(&up, 0);

    presses.update(&up, &right_and_up, true);

    assert_eq!(presses.hide_from(first, &stick(1.0, 1.0)), stick(0.0, 1.0));

    presses.update(&right_and_up, &up, false);
    presses.update(&up, &right_and_up, false);
    assert_eq!(presses.hide_from(first, &stick(1.0, 1.0)), stick(1.0, 1.0));
}

#[test]
fn a_button_one_controller_holds_from_the_menus_still_reaches_the_game_from_another() {
    let mut presses = MenuPresses::default();
    let released = ControllerState::new();
    let held = pressing(ControllerButton::South);
    let before = Controllers::from(vec![released, released]);
    let second_presses = Controllers::from(vec![released, held]);
    let both_hold = Controllers::from(vec![held, held]);

    presses.update(&before, &second_presses, true);
    presses.update(&second_presses, &both_hold, false);

    assert_eq!(presses.hide_from(id(&both_hold, 0), &held), held);
    assert_eq!(presses.hide_from(id(&both_hold, 1), &held), released);
}

#[test]
fn what_a_controller_holds_back_stays_with_it_when_one_before_it_disconnects() {
    let mut presses = MenuPresses::default();
    let released = ControllerState::new();
    let held = pressing(ControllerButton::South);
    let (first, second) = (ControllerId(7), ControllerId(9));
    let before = Controllers::from(vec![(first, released), (second, released)]);
    let second_presses = Controllers::from(vec![(first, released), (second, held)]);
    presses.update(&before, &second_presses, true);

    // The first controller disconnects, leaving the second first in the
    // list, still holding the press from the menus.
    let only_second = Controllers::from(vec![(second, held)]);
    presses.update(&second_presses, &only_second, false);
    assert_eq!(presses.hide_from(second, &held), released);

    // A controller connected in its place holds nothing back.
    let new_first = Controllers::from(vec![(second, held), (ControllerId(10), held)]);
    presses.update(&only_second, &new_first, false);
    assert_eq!(presses.hide_from(ControllerId(10), &held), held);
    assert_eq!(presses.hide_from(second, &held), released);
}

#[test]
fn a_controller_connected_while_the_game_runs_holds_nothing_back() {
    let mut presses = MenuPresses::default();
    let held = pressing(ControllerButton::South);
    let later = Controllers::from(vec![ControllerState::new(), held]);

    presses.update(&one(ControllerState::new()), &later, false);

    assert_eq!(presses.hide_from(id(&later, 1), &held), held);
}
