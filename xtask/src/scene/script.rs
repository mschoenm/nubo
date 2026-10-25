//! The input scripts `cargo xtask scene explore` plays a game with: steps
//! separated by `;` or new lines, with `#` starting a comment.
//!
//! - `wait <time>`: let the game run, such as `wait 4s`, `wait 500ms` or
//!   `wait 30f` (frames);
//! - `press <buttons> [<time>]`: press buttons and let go after the time
//!   (6 frames unless given), such as `press start` or `press a+right 1s`;
//! - `hold <buttons>`: press buttons until a `release`;
//! - `release [<buttons>]`: let go of buttons, or of all of them;
//! - `mark`: save a candidate scene here.
//!
//! Buttons are `up`, `down`, `left`, `right`, `a`, `b`, `start` and
//! `select`, joined by `+`.

use crate::Result;
use nubo_core::{DOTS_PER_FRAME, Key};

/// How many frames a `press` lasts unless it says: long enough for every
/// game to see the button, which some only read once a frame or two.
const PRESS_FRAMES: u32 = 6;
/// The Game Boy's dot clock: 4,194,304 dots each second.
const DOTS_PER_SECOND: f64 = 4_194_304.0;

/// One step of a script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Let the game run this many frames.
    Wait(u32),
    /// Press these buttons.
    Press(Vec<Key>),
    /// Let go of these buttons, or of all of them when empty.
    Release(Vec<Key>),
    /// Save a candidate scene.
    Mark,
}

/// Reads a script into its steps, each `press` with a time becoming a
/// press, a wait and a release.
pub fn parse(script: &str) -> Result<Vec<Step>> {
    let mut steps = Vec::new();
    for part in script
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default())
        .flat_map(|line| line.split(';'))
    {
        let words: Vec<&str> = part.split_whitespace().collect();
        let mistake = || format!("'{}' isn't a step of an input script", part.trim());
        match words.as_slice() {
            [] => {}
            ["wait", time] => steps.push(Step::Wait(parse_frames(time)?)),
            ["press", buttons] | ["press", buttons, _] => {
                let keys = parse_buttons(buttons)?;
                let frames = match words.get(2) {
                    Some(time) => parse_frames(time)?,
                    None => PRESS_FRAMES,
                };
                steps.push(Step::Press(keys.clone()));
                steps.push(Step::Wait(frames));
                steps.push(Step::Release(keys));
            }
            ["hold", buttons] => steps.push(Step::Press(parse_buttons(buttons)?)),
            ["release"] => steps.push(Step::Release(Vec::new())),
            ["release", buttons] => steps.push(Step::Release(parse_buttons(buttons)?)),
            ["mark"] => steps.push(Step::Mark),
            _ => return Err(mistake()),
        }
    }
    Ok(steps)
}

/// Reads a time, such as `4s`, `0.5s`, `500ms` or `30f`, as a number of
/// frames, rounded to the nearest.
pub fn parse_frames(time: &str) -> Result<u32> {
    let mistake = || format!("'{time}' isn't a time such as 4s, 500ms or 30f");
    if let Some(frames) = time.strip_suffix('f') {
        return frames.parse().map_err(|_| mistake());
    }
    let seconds: f64 = if let Some(milliseconds) = time.strip_suffix("ms") {
        milliseconds.parse::<f64>().map_err(|_| mistake())? / 1000.0
    } else if let Some(seconds) = time.strip_suffix('s') {
        seconds.parse().map_err(|_| mistake())?
    } else {
        return Err(mistake());
    };
    if !(0.0..=3600.0).contains(&seconds) {
        return Err(mistake());
    }
    let frames = (seconds * DOTS_PER_SECOND / DOTS_PER_FRAME as f64).round();
    // An hour at most, so a few hundred thousand frames.
    Ok(frames as u32)
}

/// How many seconds of the Game Boy's time `frames` frames take.
pub fn seconds(frames: u32) -> f64 {
    f64::from(frames) * DOTS_PER_FRAME as f64 / DOTS_PER_SECOND
}

/// Reads buttons joined by `+`, such as `a+right`.
fn parse_buttons(buttons: &str) -> Result<Vec<Key>> {
    buttons
        .split('+')
        .map(|button| match button {
            "up" => Ok(Key::Up),
            "down" => Ok(Key::Down),
            "left" => Ok(Key::Left),
            "right" => Ok(Key::Right),
            "a" => Ok(Key::A),
            "b" => Ok(Key::B),
            "start" => Ok(Key::Start),
            "select" => Ok(Key::Select),
            _ => Err(format!("'{button}' isn't a Game Boy button")),
        })
        .collect()
}
