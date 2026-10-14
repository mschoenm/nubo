//! Tests for shortening text that doesn't fit and for growing the screen in
//! whole steps.

use super::{truncate, whole_step_factor};
use iced::Size;

#[test]
fn the_screen_grows_by_the_largest_whole_factor_that_fits() {
    assert_eq!(whole_step_factor(Size::new(700.0, 600.0), 160, 144), 4.0);
    assert_eq!(whole_step_factor(Size::new(1000.0, 300.0), 160, 144), 2.0);
}

#[test]
fn a_super_game_boy_picture_with_its_border_grows_by_its_own_size() {
    assert_eq!(whole_step_factor(Size::new(700.0, 600.0), 256, 224), 2.0);
}

#[test]
fn the_screen_never_shrinks_below_its_own_size() {
    assert_eq!(whole_step_factor(Size::new(100.0, 100.0), 160, 144), 1.0);
}

#[test]
fn text_that_fits_is_left_alone() {
    assert_eq!(truncate("Tetris", 200.0, 10.0), "Tetris");
}

#[test]
fn long_text_ends_in_an_ellipsis() {
    // 12 characters of 5 pixels fit in 60 pixels.
    assert_eq!(
        truncate("Pokemon Crystal Version", 60.0, 10.0),
        "Pokemon Cry…"
    );
}

#[test]
fn spaces_before_the_ellipsis_are_dropped() {
    assert_eq!(truncate("Super Mario Land", 60.0, 10.0), "Super Mario…");
    assert_eq!(truncate("Wario Land II", 35.0, 10.0), "Wario…");
}

#[test]
fn no_room_leaves_only_the_ellipsis() {
    assert_eq!(truncate("Tetris", 0.0, 10.0), "…");
}
