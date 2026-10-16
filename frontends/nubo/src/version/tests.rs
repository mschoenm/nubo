//! Tests for how the version names the build's commit.

use super::*;

const HASH: &str = "7c44d3955370fa02045672a5774f923007919876";

#[test]
fn a_build_without_a_commit_has_none() {
    assert_eq!(Commit::read("", "2026-10-16"), None);
}

#[test]
fn a_commit_shows_its_short_hash_and_date() {
    let commit = Commit::read(HASH, "2026-10-16").unwrap();
    assert!(!commit.dirty);
    assert_eq!(commit.summary(), "7c44d39 2026-10-16");
    assert_eq!(
        commit.page,
        format!("{}/commit/{HASH}", env!("CARGO_PKG_REPOSITORY"))
    );
}

#[test]
fn changes_that_werent_committed_are_dirty() {
    let commit = Commit::read(&format!("{HASH}-dirty"), "2026-10-16").unwrap();
    assert!(commit.dirty);
    assert_eq!(commit.hash, HASH);
    assert_eq!(commit.summary(), "7c44d39-dirty 2026-10-16");
    assert!(commit.page.ends_with(HASH), "{}", commit.page);
}

#[test]
fn a_commit_without_a_date_shows_only_its_hash() {
    assert_eq!(Commit::read(HASH, "").unwrap().summary(), "7c44d39");
}

#[test]
fn the_version_starts_with_the_one_in_cargo_toml() {
    assert!(
        VERSION.starts_with(env!("CARGO_PKG_VERSION")),
        "{}",
        *VERSION
    );
    assert_eq!(COMMIT.is_some(), VERSION.contains('('), "{}", *VERSION);
}
