//! Tests for sizing the window that shows only the game.

use super::screen_only_size;

/// The window's inside in physical pixels, as winit works it out from a
/// logical size: multiplied by the scale factor and rounded.
fn physical(scale: u32, scale_factor: f32) -> (u32, u32) {
    let size = screen_only_size(scale, scale_factor);
    let pixels = |length: f32| (f64::from(length) * f64::from(scale_factor)).round() as u32;
    (pixels(size.width), pixels(size.height))
}

#[test]
fn the_window_is_the_screen_at_its_scale_without_display_scaling() {
    let size = screen_only_size(1, 1.0);
    assert_eq!((size.width, size.height), (160.0, 144.0));
    let size = screen_only_size(3, 1.0);
    assert_eq!((size.width, size.height), (480.0, 432.0));
}

#[test]
fn display_scaling_doesnt_change_the_pixels() {
    for scale_factor in [1.0, 1.25, 1.5, 1.75, 2.0, 2.25, 2.5, 3.0, 3.5] {
        for scale in 1..=8 {
            assert_eq!(
                physical(scale, scale_factor),
                (160 * scale, 144 * scale),
                "{scale}× at {scale_factor}"
            );
        }
    }
}
