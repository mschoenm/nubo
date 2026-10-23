//! Tests for handing new frames to the app and spreading fast forward's sound.

use super::*;
use crate::lcd::ScreenEffects;
use crate::palettes::DmgPalette;
use crate::picture::{Frame, SCREEN_HEIGHT, SCREEN_WIDTH};
use std::hash::DefaultHasher;

fn shared_frame() -> Arc<Mutex<SharedFrame>> {
    Arc::new(Mutex::new(SharedFrame::default()))
}

/// Publishes a frame of `pixels`, as `Frame::update` would: only if they
/// differ from the frame before.
fn publish_pixels(frame: &Mutex<SharedFrame>, pixels: &[u8]) {
    publish(frame, |frames| {
        frames.push(|newest| newest.pixels = pixels.to_vec())
    });
}

/// Publishes a whole Game Boy Color screen with every channel of every pixel
/// at `level`.
fn publish_level(frame: &Mutex<SharedFrame>, level: u8) {
    publish(frame, |frames| {
        frames.push(|newest| {
            *newest = Frame {
                pixels: vec![level; (SCREEN_WIDTH * SCREEN_HEIGHT * 4) as usize],
                width: SCREEN_WIDTH,
                height: SCREEN_HEIGHT,
                ..Frame::default()
            };
        })
    });
}

fn hash_of(listener: &FrameListener) -> u64 {
    let mut hasher = DefaultHasher::new();
    listener.hash(&mut hasher);
    hasher.finish()
}

/// Listens to `listener` with a callback that counts its calls.
fn count_calls(listener: &FrameListener) -> Arc<AtomicU64> {
    let calls = Arc::new(AtomicU64::new(0));
    let counter = calls.clone();
    listener.listen(move || {
        counter.fetch_add(1, Ordering::Relaxed);
    });
    calls
}

#[test]
fn a_new_listener_is_told_to_look_for_a_frame_straight_away() {
    let listener = FrameListener {
        session_number: 0,
        frame: shared_frame(),
    };

    let calls = count_calls(&listener);

    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[test]
fn the_listener_is_told_about_new_frames_only() {
    let frame = shared_frame();
    let listener = FrameListener {
        session_number: 0,
        frame: frame.clone(),
    };
    let calls = count_calls(&listener);

    publish_pixels(&frame, &[1, 2, 3]);
    publish_pixels(&frame, &[1, 2, 3]);
    publish_pixels(&frame, &[1, 2, 3]);
    publish_pixels(&frame, &[1, 2, 4]);

    assert_eq!(calls.load(Ordering::Relaxed), 4);
}

#[test]
fn only_the_first_frame_like_the_one_before_is_published() {
    let frame = shared_frame();

    publish_pixels(&frame, &[1, 2, 3]);
    publish_pixels(&frame, &[1, 2, 3]);
    assert_eq!(frame.lock().unwrap().sequence, 2);
    publish_pixels(&frame, &[1, 2, 3]);
    publish_pixels(&frame, &[1, 2, 3]);
    assert_eq!(frame.lock().unwrap().sequence, 2);

    publish_pixels(&frame, &[1, 2, 4]);
    assert_eq!(frame.lock().unwrap().sequence, 3);
}

#[test]
fn a_frame_the_app_missed_is_still_the_one_the_newest_blends_with() {
    let shared = shared_frame();
    let mut shown = LatestFrames::default();
    publish_level(&shared, 0x00);
    let sequence = shared.lock().unwrap().copy_since(0, &mut shown).unwrap();

    // Two frames come before the app looks again.
    publish_level(&shared, 0x40);
    publish_level(&shared, 0xC0);
    shared
        .lock()
        .unwrap()
        .copy_since(sequence, &mut shown)
        .unwrap();

    let blending = ScreenEffects {
        correct_colors: false,
        blend_frames: true,
    };
    let picture = blending.show(
        shown.newest().unwrap(),
        shown.before(),
        DmgPalette::Gray,
        true,
    );
    assert!(picture.pixels().iter().all(|&channel| channel == 0x80));
}

#[test]
fn nothing_is_copied_while_the_frame_is_still_the_one_shown() {
    let shared = shared_frame();
    let mut shown = LatestFrames::default();
    publish_level(&shared, 0x40);
    let sequence = shared.lock().unwrap().copy_since(0, &mut shown).unwrap();

    assert_eq!(
        shared.lock().unwrap().copy_since(sequence, &mut shown),
        None
    );
    assert!(
        shown.before().is_none(),
        "the first frame has none before it"
    );
}

#[test]
fn sessions_are_told_apart_by_number_even_when_sharing_an_address() {
    let frame = shared_frame();
    let first = FrameListener {
        session_number: 1,
        frame: frame.clone(),
    };
    let second = FrameListener {
        session_number: 2,
        frame,
    };

    assert_ne!(hash_of(&first), hash_of(&second));
}

#[test]
fn fast_forward_spreads_its_buffers_over_the_played_one() {
    let mut fast_forward = FastForward::new();
    assert_eq!(fast_forward.time_until_next_buffer(), None);

    fast_forward.spread(3, Duration::from_millis(20));

    assert_eq!(fast_forward.interval, Duration::from_millis(5));
    assert!(fast_forward.time_until_next_buffer().unwrap() <= Duration::from_millis(5));
    for _ in 0..3 {
        assert!(fast_forward.take_buffer());
    }
    assert!(!fast_forward.take_buffer());
    assert_eq!(fast_forward.time_until_next_buffer(), None);
}

#[test]
fn clearing_fast_forward_drops_the_buffers_still_due() {
    let mut fast_forward = FastForward::new();
    fast_forward.spread(3, Duration::from_millis(20));

    fast_forward.clear();

    assert!(!fast_forward.take_buffer());
    assert_eq!(fast_forward.time_until_next_buffer(), None);
}
