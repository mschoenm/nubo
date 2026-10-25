//! Tests for reading the version or commit to build and finding its release.

use super::*;

#[test]
fn versions_with_one_to_three_numbers_are_read() {
    assert_eq!(parse_version("0.12"), Some(vec![0, 12]));
    assert_eq!(parse_version("v0.12.1"), Some(vec![0, 12, 1]));
    assert_eq!(parse_version("1"), Some(vec![1]));
}

#[test]
fn anything_else_is_not_a_version() {
    for text in [
        "main",
        "nightly",
        "v",
        "0.12.",
        "0..1",
        "1.2.3.4",
        "2bb52ae",
        "v0.12-rc1",
    ] {
        assert_eq!(parse_version(text), None, "{text}");
    }
}

#[test]
fn a_version_picks_the_newest_matching_release() {
    let tags = [
        "nightly", "v0.9.2", "v0.12.0", "v0.12.2", "v0.12.10", "v0.120.0", "v1.0.0",
    ];
    let newest = |version: &str| newest_release(tags.into_iter(), &parse_version(version).unwrap());
    assert_eq!(newest("0.12").as_deref(), Some("v0.12.10"));
    assert_eq!(newest("0.12.2").as_deref(), Some("v0.12.2"));
    assert_eq!(newest("v0.9").as_deref(), Some("v0.9.2"));
    assert_eq!(newest("0").as_deref(), Some("v0.120.0"));
    assert_eq!(newest("0.13"), None);
}

#[test]
fn a_version_resolves_to_its_release_tag() {
    let tags = "v0.12.0\nv0.12.1\nv1.0.0\n";
    let resolved = |what: &str| resolve(what, tags);
    assert_eq!(
        resolved("0.12").unwrap(),
        ("refs/tags/v0.12.1".to_owned(), "v0.12.1".to_owned())
    );
    assert_eq!(
        resolved("1").unwrap(),
        ("refs/tags/v1.0.0".to_owned(), "v1.0.0".to_owned())
    );
    assert_eq!(
        resolved("main").unwrap(),
        (MAIN.to_owned(), MAIN.to_owned())
    );
    assert!(resolved("0.13").is_err());
}

#[test]
fn a_hash_of_only_digits_is_passed_on_to_git() {
    let tags = "v0.12.0\nv1.0.0\n";
    for what in ["1234567", "v2", "2bb52ae"] {
        assert_eq!(
            resolve(what, tags).unwrap(),
            (what.to_owned(), what.to_owned()),
            "{what}"
        );
    }
}
