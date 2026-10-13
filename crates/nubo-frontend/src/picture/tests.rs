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
