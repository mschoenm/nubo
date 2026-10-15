use super::*;
use std::hash::DefaultHasher;

fn shared_frame() -> Arc<Mutex<SharedFrame>> {
    Arc::new(Mutex::new(SharedFrame::default()))
}

/// Publishes a frame of `pixels`, as `Frame::update` would: only if they
/// differ from the frame before.
fn publish_pixels(frame: &Mutex<SharedFrame>, pixels: &[u8]) {
    publish(frame, |latest| {
        let changed = latest.pixels != pixels;
        latest.pixels = pixels.to_vec();
        changed
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
