//! Tests for reading controllers.

use super::{Controllers, snap};
use nubo_core::{ControllerButton, ControllerState};

#[test]
fn the_stick_is_fully_pushed_or_centered() {
    assert_eq!(snap(0.3), 0.0);
    assert_eq!(snap(-0.49), 0.0);
    assert_eq!(snap(0.5), 1.0);
    assert_eq!(snap(-0.8), -1.0);
}

#[test]
fn combined_controllers_hold_every_button_and_the_first_stick_push() {
    let mut first = ControllerState::new();
    first.set_button(ControllerButton::South, true);
    first.set_left_stick(-1.0, 0.0);
    let mut second = ControllerState::new();
    second.set_button(ControllerButton::Start, true);
    second.set_left_stick(1.0, 1.0);

    let combined = Controllers::from(vec![first, second]).combined();

    assert!(combined.is_pressed(ControllerButton::South));
    assert!(combined.is_pressed(ControllerButton::Start));
    let directions: Vec<_> = combined.stick_directions().collect();
    assert!(directions.contains(&nubo_core::Key::Left));
    assert!(directions.contains(&nubo_core::Key::Up));
}
