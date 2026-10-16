use super::{Shard, parse_seconds, parse_shard};

#[test]
fn parts_are_read_from_their_number_and_count() {
    assert_eq!(
        parse_shard("2/4"),
        Ok(Shard {
            number: 2,
            count: 4
        })
    );
    assert_eq!(
        parse_shard("1/1"),
        Ok(Shard {
            number: 1,
            count: 1
        })
    );
    for invalid in ["0/4", "5/4", "1/0", "2", "a/4", "2/b", ""] {
        assert!(parse_shard(invalid).is_err(), "{invalid} was accepted");
    }
}

#[test]
fn every_rom_is_in_exactly_one_part() {
    for count in 1..=7 {
        for position in 0..50 {
            let parts = (1..=count)
                .filter(|&number| Shard { number, count }.contains(position))
                .count();
            assert_eq!(parts, 1, "ROM {position} is in {parts} of {count} parts");
        }
    }
}

#[test]
fn seconds_must_be_a_finite_number_above_0() {
    assert_eq!(parse_seconds("4"), Ok(4.0));
    assert_eq!(parse_seconds("0.5"), Ok(0.5));
    for invalid in ["0", "-1", "NaN", "inf", "-inf", "a", ""] {
        assert!(parse_seconds(invalid).is_err(), "{invalid} was accepted");
    }
}
