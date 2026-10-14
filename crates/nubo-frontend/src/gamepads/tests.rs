//! Tests for reading controllers.

use super::snap;

#[test]
fn the_stick_is_fully_pushed_or_centered() {
    assert_eq!(snap(0.3), 0.0);
    assert_eq!(snap(-0.49), 0.0);
    assert_eq!(snap(0.5), 1.0);
    assert_eq!(snap(-0.8), -1.0);
}
