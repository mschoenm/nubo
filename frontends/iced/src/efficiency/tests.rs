use super::*;

#[test]
fn rates_are_the_counts_between_two_moments_per_second() {
    let start = ActivityCount {
        wakeups: 100,
        buffers: 90,
        frames: 40,
    };
    let end = ActivityCount {
        wakeups: 530,
        buffers: 520,
        frames: 340,
    };

    let measured = rates(start, end, 5.0);

    assert_eq!(measured.wakeups, 86.0);
    assert_eq!(measured.buffers, 86.0);
    assert_eq!(measured.frames, 60.0);
    assert_eq!(
        json(&measured),
        r#"{"wakeups_per_second":86.0,"buffers_per_second":86.0,"frames_per_second":60.0}"#
    );
}
