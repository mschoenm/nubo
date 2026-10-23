//! Tests for waking the emulator thread when the sound queue runs low.

use super::*;
use std::thread;
use std::time::Instant;

/// A queue holding `length` silent samples that wakes the current thread.
fn queue_waking_this_thread(length: usize) -> SampleQueue {
    let queue = SampleQueue::default();
    queue
        .samples
        .lock()
        .unwrap()
        .extend(std::iter::repeat_n((0.0, 0.0), length));
    let _ = queue.low_waker.set(thread::current());
    queue
}

/// Whether this thread was unparked: parking then returns at once instead of
/// waiting out the timeout.
fn was_woken() -> bool {
    let started = Instant::now();
    thread::park_timeout(Duration::from_millis(200));
    started.elapsed() < Duration::from_millis(100)
}

#[test]
fn wakes_the_thread_when_the_queue_drops_below_the_low_mark() {
    let queue = queue_waking_this_thread(QUEUE_LOW_THRESHOLD + 100);
    let mut buffer = [0.0_f32; 2 * 200];

    fill_output_buffer(&mut buffer, 2, &queue);

    assert!(was_woken());
}

#[test]
fn leaves_the_thread_asleep_while_the_queue_stays_full_enough() {
    let queue = queue_waking_this_thread(QUEUE_LOW_THRESHOLD + 300);
    let mut buffer = [0.0_f32; 2 * 200];

    fill_output_buffer(&mut buffer, 2, &queue);

    assert!(!was_woken());
}

#[test]
fn leaves_the_thread_asleep_while_the_queue_stays_low() {
    let queue = queue_waking_this_thread(QUEUE_LOW_THRESHOLD - 1);
    let mut buffer = [0.0_f32; 2 * 200];

    fill_output_buffer(&mut buffer, 2, &queue);

    assert!(!was_woken());
}
