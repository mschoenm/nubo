//! Decides when the browser session emulates a frame and how many sound
//! samples it makes per emulated second (see `emulation/web.rs`). Kept apart
//! from the session so it can be tested without a browser.
//!
//! On screens that refresh about 60, 120 or 240 times a second, frames are
//! locked to the refreshes: one frame every one, two or four of them, so
//! every frame stays on screen equally long. The Game Boy draws 59.73 frames
//! a second, so this runs the game about 0.45% fast. The sound keeps up by
//! making that much fewer samples per emulated second, which keeps its pitch
//! within 8 cents, too little to hear. Other screens (75 or 144 Hz, or ones
//! with variable refresh) get frames by the clock instead, and a frame now
//! and then stays up one refresh longer. Either way the audio queue only
//! changes how many samples are made, never when a frame runs, so the
//! queue's jumps by a whole sound buffer don't move frames.

use iced::time::Duration;

/// How long the Game Boy takes to draw a frame: 70,224 clock ticks at
/// 4,194,304 Hz, about 59.73 frames a second.
pub const FRAME_DURATION: Duration = Duration::from_nanos(16_742_706);

/// How many ticks in a row must come one refresh apart before frames lock to
/// the refreshes: about two seconds at 60 Hz.
const STEADY_TICKS_TO_LOCK: u32 = 120;
/// How many ticks in a row that don't fit the measured refresh rate make it
/// measure again, such as after the window moved to another screen.
const ODD_TICKS_TO_REMEASURE: u32 = 10;
/// How far, as a share of a refresh, a tick may come early or late and still
/// count as a whole number of refreshes.
const REFRESH_TOLERANCE: f64 = 0.25;
/// How far a whole number of refreshes may be from a Game Boy frame, as a
/// share, for frames to lock to them. Covers 59.94 to about 60.3 Hz and
/// their multiples.
const LOCK_TOLERANCE: f64 = 0.01;
/// The most refreshes one frame is locked to: 240 Hz screens show each frame
/// four times.
const MOST_REFRESHES_PER_FRAME: f64 = 4.0;

/// How long the audio queue's length is averaged over, in seconds. Long
/// enough to smooth out its jumps by a whole sound buffer.
const QUEUE_AVERAGING_SECONDS: f64 = 0.5;
/// How strongly the sample rate follows the sound missing from the queue:
/// the share it changes by per second of sound missing.
const PROPORTIONAL_GAIN: f64 = 0.6;
/// How strongly the sample rate follows sound that stays missing, per second
/// of sound missing for a second. With `PROPORTIONAL_GAIN` this settles in
/// about ten seconds without overshooting.
const INTEGRAL_GAIN: f64 = 0.09;
/// The most the sample rate changes, as a share, besides making up for a
/// locked game's speed: far more than sound cards' clocks drift from the
/// browser's.
const MOST_ADJUSTMENT: f64 = 0.01;

/// Measures how often the screen refreshes from the time between ticks, and
/// says when frames can be locked to the refreshes.
#[derive(Debug, Default)]
pub struct RefreshRate {
    /// The time the ticks that came a whole number of refreshes apart took
    /// since the rate was last measured, in seconds.
    total_time: f64,
    /// How many refreshes that time held.
    total_refreshes: f64,
    /// Ticks in a row that came one refresh apart.
    steady_ticks: u32,
    /// Ticks in a row that didn't come one refresh apart.
    odd_ticks: u32,
}

impl RefreshRate {
    /// Takes in the time in seconds since the last tick. A tick a whole
    /// number of refreshes later (one the browser skipped) keeps frames
    /// locked; one in between refreshes unlocks them until the ticks are
    /// steady again.
    pub fn observe(&mut self, elapsed: f64) {
        let Some(interval) = self.interval() else {
            if elapsed > 0.0 {
                self.measure_from(elapsed);
            }
            return;
        };

        let refreshes = elapsed / interval;
        let whole = refreshes.round();
        let fits = whole >= 1.0 && (refreshes - whole).abs() < REFRESH_TOLERANCE;
        if fits {
            self.total_time += elapsed;
            self.total_refreshes += whole;
        }
        if fits && whole == 1.0 {
            self.steady_ticks = self.steady_ticks.saturating_add(1);
            self.odd_ticks = 0;
            return;
        }

        if !fits {
            self.steady_ticks = 0;
        }
        self.odd_ticks += 1;
        if self.odd_ticks >= ODD_TICKS_TO_REMEASURE {
            self.measure_from(elapsed);
        }
    }

    /// Starts measuring again, from one tick `elapsed` seconds long.
    fn measure_from(&mut self, elapsed: f64) {
        *self = RefreshRate {
            total_time: elapsed,
            total_refreshes: 1.0,
            ..RefreshRate::default()
        };
    }

    /// The average time between refreshes in seconds, or `None` before the
    /// first tick.
    fn interval(&self) -> Option<f64> {
        (self.total_refreshes > 0.0).then(|| self.total_time / self.total_refreshes)
    }

    /// How frames are locked to the refreshes, or `None` while the screen's
    /// refresh rate isn't steady or not close to a multiple of the Game
    /// Boy's.
    pub fn lock(&self) -> Option<Lock> {
        let interval = self.interval()?;
        if self.steady_ticks < STEADY_TICKS_TO_LOCK {
            return None;
        }
        let refreshes = FRAME_DURATION.as_secs_f64() / interval;
        let whole = refreshes.round();
        ((1.0..=MOST_REFRESHES_PER_FRAME).contains(&whole)
            && (refreshes / whole - 1.0).abs() <= LOCK_TOLERANCE)
            .then_some(Lock {
                refreshes_per_frame: whole,
                refresh_interval: interval,
            })
    }
}

/// Frames locked to the screen's refreshes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lock {
    /// How many refreshes each frame stays on screen.
    refreshes_per_frame: f64,
    /// The average time between refreshes in seconds.
    refresh_interval: f64,
}

impl Lock {
    /// How many frames are due after `elapsed` seconds: one for every
    /// `refreshes_per_frame` refreshes, counted whole, so they come at the
    /// same refreshes however early or late the ticks are.
    pub fn frames_in(self, elapsed: f64) -> f64 {
        (elapsed / self.refresh_interval).round() / self.refreshes_per_frame
    }

    /// How many times faster than a Game Boy the game runs, such as 1.0045
    /// at 60 Hz.
    pub fn speed(self) -> f64 {
        FRAME_DURATION.as_secs_f64() / (self.refreshes_per_frame * self.refresh_interval)
    }
}

/// Keeps the audio queue at its target length by making slightly more or
/// fewer samples per emulated second, instead of emulating more or fewer
/// frames. It follows the queue's average length, so the queue's jumps when
/// the sound card takes a buffer barely change it.
#[derive(Debug, Default)]
pub struct SampleRateControl {
    /// The average number of queued samples, or `None` before the first
    /// update.
    average_queued: Option<f64>,
    /// The sound missing from the queue in seconds, added up over time in
    /// seconds.
    missing_over_time: f64,
}

impl SampleRateControl {
    /// Takes in the `queued` samples after `elapsed` seconds, and returns the
    /// share to make more samples by (fewer when negative), between -1% and
    /// 1%, to bring the queue to `target` samples at `sample_rate`.
    pub fn update(&mut self, elapsed: f64, queued: f64, target: f64, sample_rate: f64) -> f64 {
        let share = 1.0 - (-elapsed / QUEUE_AVERAGING_SECONDS).exp();
        let average = self
            .average_queued
            .map_or(queued, |average| average + (queued - average) * share);
        self.average_queued = Some(average);

        let missing = (target - average) / sample_rate;
        let missing_over_time = self.missing_over_time + missing * elapsed;
        let adjustment = PROPORTIONAL_GAIN * missing + INTEGRAL_GAIN * missing_over_time;
        // While the adjustment is at its limit the missing sound isn't added
        // up, so it doesn't build up a debt that overshoots later.
        if adjustment.abs() <= MOST_ADJUSTMENT {
            self.missing_over_time = missing_over_time;
        }
        adjustment.clamp(-MOST_ADJUSTMENT, MOST_ADJUSTMENT)
    }
}

#[cfg(test)]
mod tests;
