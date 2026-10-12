use super::*;

/// The times between ticks that come `hz` times a second, each seen up to
/// `jitter` seconds early or late, told apart from run to run by `seed`.
fn ticks(hz: f64, jitter: f64, count: usize, seed: u64) -> impl Iterator<Item = f64> {
    let mut state = seed;
    let mut last_seen = 0.0;
    (1..=count).map(move |tick| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let random = (state >> 11) as f64 / (1u64 << 53) as f64;
        let seen = tick as f64 / hz + (random * 2.0 - 1.0) * jitter;
        let elapsed = seen - last_seen;
        last_seen = seen;
        elapsed
    })
}

/// A refresh rate that has seen `count` ticks at `hz`.
fn measured(hz: f64, jitter: f64, count: usize) -> RefreshRate {
    let mut rate = RefreshRate::default();
    for elapsed in ticks(hz, jitter, count, 1) {
        rate.observe(elapsed);
    }
    rate
}

#[test]
fn frames_lock_to_screens_near_a_multiple_of_the_game_boys_rate() {
    for (hz, refreshes_per_frame) in [(60.0, 1.0), (59.94, 1.0), (120.0, 2.0), (240.0, 4.0)] {
        let lock = measured(hz, 0.001 / (hz / 60.0), 300).lock();
        assert_eq!(
            lock.map(|lock| lock.refreshes_per_frame),
            Some(refreshes_per_frame),
            "{hz} Hz"
        );
    }
}

#[test]
fn frames_go_by_the_clock_on_other_screens() {
    for hz in [50.0, 75.0, 90.0, 144.0, 165.0] {
        assert_eq!(measured(hz, 0.0005, 300).lock(), None, "{hz} Hz");
    }
}

#[test]
fn frames_go_by_the_clock_while_ticks_are_irregular() {
    let mut rate = measured(60.0, 0.001, 300);
    for elapsed in [0.012, 0.020].repeat(20) {
        rate.observe(elapsed);
    }
    assert_eq!(rate.lock(), None);
}

#[test]
fn frames_lock_only_once_the_ticks_are_steady() {
    assert_eq!(measured(60.0, 0.001, 60).lock(), None);
}

#[test]
fn a_locked_game_runs_as_fast_as_the_screen() {
    let lock = measured(60.0, 0.001, 300).lock().unwrap();
    assert!((lock.speed() - 1.0045).abs() < 0.0005, "{}", lock.speed());
}

#[test]
fn a_skipped_refresh_still_makes_its_frame_due() {
    let mut rate = measured(60.0, 0.001, 300);
    rate.observe(2.0 / 60.0);
    let lock = rate.lock().unwrap();
    assert_eq!(lock.frames_in(2.0 / 60.0), 2.0);
    assert_eq!(lock.frames_in(1.0 / 60.0 + 0.003), 1.0);
}

#[test]
fn the_rate_is_measured_again_when_the_screen_changes() {
    let mut rate = measured(60.0, 0.001, 300);
    for elapsed in ticks(120.0, 0.0005, 300, 2) {
        rate.observe(elapsed);
    }
    assert_eq!(rate.lock().map(|lock| lock.refreshes_per_frame), Some(2.0));

    for elapsed in ticks(60.0, 0.001, 300, 3) {
        rate.observe(elapsed);
    }
    assert_eq!(rate.lock().map(|lock| lock.refreshes_per_frame), Some(1.0));
}

#[test]
fn moving_to_a_screen_with_another_refresh_rate_unlocks_frames_at_once() {
    for (from_hz, to_hz) in [(60.0, 75.0), (60.0, 50.0), (120.0, 144.0)] {
        let mut rate = measured(from_hz, 0.0005, 20_000);
        rate.observe(1.0 / to_hz);
        assert_eq!(rate.lock(), None, "{from_hz} to {to_hz} Hz");

        for elapsed in ticks(to_hz, 0.0005, 300, 4) {
            rate.observe(elapsed);
        }
        let interval = rate.interval().unwrap();
        assert!(
            (interval * to_hz - 1.0).abs() < 0.001,
            "{from_hz} to {to_hz} Hz: measured {} Hz",
            1.0 / interval
        );
    }
}

#[test]
fn ticks_rounded_to_the_millisecond_still_lock_at_120_hz() {
    let mut rate = RefreshRate::default();
    let mut last = 0.0;
    for tick in 1..=300 {
        let seen = (f64::from(tick) * 1000.0 / 120.0).round() / 1000.0;
        rate.observe(seen - last);
        last = seen;
    }
    assert_eq!(rate.lock().map(|lock| lock.refreshes_per_frame), Some(2.0));
}

/// What the browser's sound did in [`simulate`].
struct Sound {
    /// The fewest samples left in the queue right before the sound card took
    /// a buffer, after the first five seconds.
    fewest_before_take: f64,
    /// The average queue length over the last ten seconds, measured where
    /// the session does: before each tick's frames add their samples.
    average_at_end: f64,
}

/// Plays `seconds` of a game locked to a `hz` screen, `refreshes_per_frame`
/// refreshes a frame, into a sound card that takes 2,048 samples at a time
/// at 48 kHz on a clock running `drift` fast (or slow, when negative), with
/// the sample rate set by [`SampleRateControl`].
fn simulate(hz: f64, refreshes_per_frame: f64, drift: f64, seconds: f64) -> Sound {
    const SAMPLE_RATE: f64 = 48_000.0;
    const BUFFER: f64 = 2048.0;
    const TARGET: f64 = BUFFER * 1.25;
    let lock = Lock {
        refreshes_per_frame,
        refresh_interval: 1.0 / hz,
    };

    let mut control = SampleRateControl::default();
    let mut queued = TARGET;
    let mut frames_due = 0.0;
    let mut takes = 0.0;
    let mut time = 0.0;
    let mut fewest_before_take = f64::MAX;
    let (mut queued_at_end, mut ticks_at_end) = (0.0, 0.0);

    while time < seconds {
        time += 1.0 / hz;
        while takes < (time * SAMPLE_RATE * (1.0 + drift) / BUFFER).floor() {
            if time > 5.0 {
                fewest_before_take = fewest_before_take.min(queued);
            }
            queued = (queued - BUFFER).max(0.0);
            takes += 1.0;
        }

        if time > seconds - 10.0 {
            queued_at_end += queued;
            ticks_at_end += 1.0;
        }

        let adjustment = control.update(1.0 / hz, queued, TARGET, SAMPLE_RATE);
        let sample_rate = SAMPLE_RATE * (1.0 + adjustment) / lock.speed();
        frames_due += lock.frames_in(1.0 / hz);
        while frames_due >= 1.0 {
            frames_due -= 1.0;
            queued += FRAME_DURATION.as_secs_f64() * sample_rate;
        }
    }

    Sound {
        fewest_before_take,
        average_at_end: queued_at_end / ticks_at_end,
    }
}

#[test]
fn the_sample_rate_keeps_the_sound_queue_filled_on_a_locked_screen() {
    for (hz, refreshes_per_frame, drift) in [
        (60.0, 1.0, 0.0),
        (60.0, 1.0, 0.001),
        (59.94, 1.0, -0.001),
        (120.0, 2.0, 0.002),
        (120.0, 2.0, -0.002),
    ] {
        let sound = simulate(hz, refreshes_per_frame, drift, 60.0);
        assert!(
            sound.fewest_before_take >= 2048.0,
            "{hz} Hz, drift {drift}: the queue ran down to {}",
            sound.fewest_before_take
        );
        assert!(
            (sound.average_at_end - 2560.0).abs() < 200.0,
            "{hz} Hz, drift {drift}: the queue settled at {}",
            sound.average_at_end
        );
    }
}
