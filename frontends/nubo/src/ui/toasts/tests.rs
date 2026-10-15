//! Tests for showing, repeating and taking down toasts.

use super::*;

fn messages(toasts: &Toasts) -> Vec<&str> {
    toasts
        .toasts
        .iter()
        .map(|toast| toast.message.as_str())
        .collect()
}

#[test]
fn a_toast_goes_once_it_has_been_shown_long_enough() {
    let mut toasts = Toasts::default();
    let shown = Instant::now();
    toasts.info("Saved to Slot 1");

    toasts.remove_expired(shown + INFO_DURATION - Duration::from_millis(100));
    assert!(!toasts.is_empty());

    toasts.remove_expired(shown + INFO_DURATION + Duration::from_millis(100));
    assert!(toasts.is_empty());
}

#[test]
fn errors_stay_longer_than_confirmations() {
    let mut toasts = Toasts::default();
    let shown = Instant::now();
    toasts.info("Saved to Slot 1");
    toasts.error("No sound");

    toasts.remove_expired(shown + INFO_DURATION + Duration::from_millis(100));

    assert_eq!(messages(&toasts), ["No sound"]);
}

#[test]
fn a_message_already_on_screen_is_not_repeated() {
    let mut toasts = Toasts::default();

    toasts.error("No sound");
    toasts.error("No sound");
    toasts.info("No sound");

    assert_eq!(messages(&toasts), ["No sound", "No sound"]);
}

#[test]
fn a_repeated_message_becomes_the_newest_toast() {
    let mut toasts = Toasts::default();

    toasts.error("No sound");
    toasts.info("Saved to Slot 1");
    toasts.error("No sound");

    assert_eq!(messages(&toasts), ["Saved to Slot 1", "No sound"]);
    assert_eq!(toasts.toasts[1].repeats, 1);
}

#[test]
fn the_oldest_toasts_make_way_for_new_ones() {
    let mut toasts = Toasts::default();

    for number in 0..=MAX_VISIBLE {
        toasts.info(format!("Toast {number}"));
    }

    assert_eq!(toasts.toasts.len(), MAX_VISIBLE);
    assert_eq!(messages(&toasts)[0], "Toast 1");
}

#[test]
fn dismissing_takes_down_that_toast_only() {
    let mut toasts = Toasts::default();
    toasts.info("First");
    toasts.info("Second");

    toasts.dismiss(0);
    toasts.dismiss(5);

    assert_eq!(messages(&toasts), ["Second"]);
}
