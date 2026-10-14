//! Tests for pictures and what they keep to be drawn with.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn two_by_one() -> Picture {
    Picture::new(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8])
}

#[test]
fn a_picture_has_its_size_and_pixels() {
    let picture = two_by_one();

    assert_eq!((picture.width(), picture.height()), (2, 1));
    assert_eq!(picture.pixels(), &[1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(picture.shared_pixels().as_ref(), picture.pixels());
}

#[test]
fn a_picture_is_drawn_once_for_it_and_its_clones() {
    static DRAWS: AtomicUsize = AtomicUsize::new(0);
    let draw = |picture: &Picture| {
        DRAWS.fetch_add(1, Ordering::Relaxed);
        picture.width()
    };
    let picture = two_by_one();
    let clone = picture.clone();

    assert_eq!(picture.drawn(draw), 2);
    assert_eq!(clone.drawn(draw), 2);
    assert_eq!(DRAWS.load(Ordering::Relaxed), 1);
}

#[test]
fn each_new_picture_is_drawn_anew() {
    let first = two_by_one();
    let second = two_by_one();

    assert_eq!(first.drawn(|_| 1_u8), 1);
    assert_eq!(second.drawn(|_| 2_u8), 2);
}

#[test]
fn the_shared_pixels_outlive_the_picture() {
    let picture = two_by_one();
    let shared = picture.shared_pixels();

    drop(picture);

    assert_eq!(shared.as_ref(), &[1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn a_frame_of_an_original_game_boy_is_its_screen() {
    let emulator = nubo_core::Emulator::new(nubo_core::Model::Dmg);
    let frame = Frame::latest(&emulator);
    assert_eq!((frame.width, frame.height), (SCREEN_WIDTH, SCREEN_HEIGHT));
    assert!(!frame.has_border());
    assert!(frame.in_dmg_shades);
    assert_eq!(frame.screen_pixels(), frame.pixels);
}

#[test]
fn a_super_game_boy_frame_has_its_border_around_the_screen() {
    let emulator = nubo_core::Emulator::new(nubo_core::Model::Sgb);
    let mut frame = Frame::latest(&emulator);
    assert_eq!((frame.width, frame.height), (256, 224));
    assert_eq!(frame.screen_position, (48, 40));
    assert!(frame.has_border());
    assert!(!frame.update(&emulator), "an unchanged frame isn't new");

    let shown = frame.shown(DmgPalette::Gray, false);
    assert_eq!(
        (shown.width(), shown.height()),
        (SCREEN_WIDTH, SCREEN_HEIGHT)
    );
    assert_eq!(shown.pixels(), &frame.screen_pixels()[..]);
}

#[test]
fn a_color_game_boy_frame_is_never_repainted() {
    let emulator = nubo_core::Emulator::new(nubo_core::Model::CgbE);
    let frame = Frame::latest(&emulator);
    assert!(!frame.in_dmg_shades);
    assert_eq!(
        frame.shown(DmgPalette::Green, true).pixels(),
        &frame.pixels[..]
    );
}
