//! Clocks and durations that work on the desktop and in the browser:
//! `Instant`, `SystemTime` and `Duration` from the `web-time` crate. On the
//! desktop they are std's own; in the browser, where std's clocks panic, they
//! read the browser's clocks. Code both builds share takes them from here.

pub use web_time::{Duration, Instant, SystemTime};
