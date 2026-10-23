//! Tests for the size the window opens at.

use super::*;

#[test]
fn a_saved_size_that_is_not_a_finite_number_is_ignored() {
    let default = default_window(None).size;

    assert_eq!(default_window(Some((f32::INFINITY, 600.0))).size, default);
    assert_eq!(default_window(Some((800.0, f32::NAN))).size, default);
    assert_eq!(
        default_window(Some((800.0, 600.0))).size,
        iced::Size::new(800.0, 600.0)
    );
    assert_eq!(default_window(Some((-1.0, 0.0))).size, MINIMUM_SIZE.into());
}

#[test]
fn a_saved_size_too_big_for_any_screen_is_cut_down() {
    assert_eq!(
        default_window(Some((1e30, 600.0))).size,
        iced::Size::new(16_384.0, 600.0)
    );
    assert_eq!(
        default_window(Some((800.0, f32::MAX))).size,
        iced::Size::new(800.0, 16_384.0)
    );
}
