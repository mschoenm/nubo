//! Tests for shortening text that doesn't fit, for growing the screen in
//! whole steps and for naming a bound key in a tooltip.

use super::{truncate, whole_step_factor, whole_step_or_fitting_factor, with_key};
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
fn a_framed_screen_grows_in_whole_steps_but_shrinks_to_fit_a_small_card() {
    assert_eq!(
        whole_step_or_fitting_factor(Size::new(340.0, 300.0), 160, 144),
        2.0
    );
    assert_eq!(
        whole_step_or_fitting_factor(Size::new(180.0, 200.0), 160, 144),
        1.0
    );
    assert_eq!(
        whole_step_or_fitting_factor(Size::new(120.0, 200.0), 160, 144),
        0.75
    );
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

#[test]
fn a_tooltip_names_the_bound_key_and_leaves_out_a_missing_one() {
    assert_eq!(
        with_key("Fullscreen", Some(String::from("F11"))),
        "Fullscreen (F11)"
    );
    assert_eq!(with_key("Fullscreen", None), "Fullscreen");
}
