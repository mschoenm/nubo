//! Tests for the app's small helpers.

use super::cycle;

const OPTIONS: [u8; 3] = [1, 2, 3];

#[test]
fn cycling_stops_at_the_ends_without_wrapping() {
    assert_eq!(cycle(&OPTIONS, 1, -1, false), 1);
    assert_eq!(cycle(&OPTIONS, 2, 1, false), 3);
    assert_eq!(cycle(&OPTIONS, 3, 1, false), 3);
}

#[test]
fn cycling_starts_over_with_wrapping() {
    assert_eq!(cycle(&OPTIONS, 3, 1, true), 1);
    assert_eq!(cycle(&OPTIONS, 1, -1, true), 3);
}
