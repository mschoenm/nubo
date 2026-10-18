use super::{AUTOSAVE_INTERVAL, AutosaveTimer};
use nubo_frontend::time::{Duration, Instant};

#[test]
fn a_game_that_never_ran_is_not_saved() {
    let mut timer = AutosaveTimer::default();
    let start = Instant::now();
    assert!(!timer.due(false, start));
    assert!(!timer.due(false, start + AUTOSAVE_INTERVAL * 2));
}

#[test]
fn a_running_game_is_saved_every_interval() {
    let mut timer = AutosaveTimer::default();
    let start = Instant::now();
    assert!(!timer.due(true, start));
    assert!(!timer.due(true, start + AUTOSAVE_INTERVAL - Duration::from_secs(1)));
    assert!(timer.due(true, start + AUTOSAVE_INTERVAL));
    assert!(!timer.due(true, start + AUTOSAVE_INTERVAL + Duration::from_secs(1)));
    assert!(timer.due(true, start + AUTOSAVE_INTERVAL * 2));
}

#[test]
fn a_game_is_saved_once_as_it_stops_running() {
    let mut timer = AutosaveTimer::default();
    let start = Instant::now();
    assert!(!timer.due(true, start));
    assert!(timer.due(false, start + Duration::from_secs(5)));
    assert!(!timer.due(false, start + AUTOSAVE_INTERVAL * 3));
    assert!(!timer.due(true, start + AUTOSAVE_INTERVAL * 3));
    assert!(!timer.due(true, start + AUTOSAVE_INTERVAL * 3 + Duration::from_secs(1)));
    assert!(timer.due(true, start + AUTOSAVE_INTERVAL * 4));
}
