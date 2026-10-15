//! Tests for keeping and giving back the states rewinding goes through.

use super::*;

/// A state of `length` bytes, with `changes` of them changed by `step`.
fn state(length: usize, changes: &[usize], step: u8) -> Vec<u8> {
    let mut state: Vec<u8> = (0..length).map(|index| (index % 251) as u8).collect();
    for &index in changes {
        state[index] = state[index].wrapping_add(step);
    }
    state
}

#[test]
fn rewinding_gives_back_every_state_newest_first() {
    let states: Vec<Vec<u8>> = (0..10)
        .map(|step| state(5000, &[3, 4, 100, 4999], step))
        .collect();
    let mut history = History::default();
    for state in &states {
        history.push(state.clone());
    }

    for state in states.iter().rev() {
        assert_eq!(history.pop().as_ref(), Some(state));
    }
    assert_eq!(history.pop(), None);
}

#[test]
fn older_states_take_little_memory() {
    let mut history = History::default();
    for step in 0..5 {
        history.push(state(100_000, &[10, 20_000], step));
    }

    assert!(history.bytes < 100_000 + 5 * 64, "{}", history.bytes);
}

#[test]
fn states_of_another_size_are_kept_whole() {
    let mut history = History::default();
    history.push(vec![1; 10]);
    history.push(vec![2; 20]);

    assert_eq!(history.pop(), Some(vec![2; 20]));
    assert_eq!(history.pop(), Some(vec![1; 10]));
}

#[test]
fn the_oldest_states_go_once_there_are_too_many() {
    let mut history = History::default();
    for step in 0..MAX_STATES + 10 {
        history.push(state(64, &[0], step as u8));
    }

    assert_eq!(history.len(), MAX_STATES + 1);
}

#[test]
fn a_state_is_kept_every_few_frames() {
    let mut history = History::default();
    let kept = (0..FRAMES_PER_STATE * 3)
        .filter(|_| history.frame_finished())
        .count();

    assert_eq!(kept, 3);
}
