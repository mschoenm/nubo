//! `cargo xtask rust-version`: checks that no crate the app or the RetroArch
//! core is built from needs a newer Rust than their `rust-version` in the
//! root `Cargo.toml`, the Rust of the freedesktop SDK that builds the
//! Flatpak (`docs/CI.md`). Resolver 3 keeps `cargo update` within it, but a
//! dependency raised by hand or by Dependabot isn't, and CI builds with the
//! newest Rust, so without this check such an update would fail only the
//! nightly Flatpak build. It reads `cargo metadata` for the Flatpak's two
//! systems, so it takes seconds and needs no other Rust installed. CI runs
//! it on every pull request.

use super::Result;
use super::licenses::{built_from, metadata, string};
use serde_json::Value;
use std::collections::BTreeSet;

/// The packages whose crates are checked: the app and the RetroArch core.
const PACKAGES: [&str; 2] = ["nubo", "nubo-libretro"];

/// The systems the Flatpak is built for.
const TARGETS: [&str; 2] = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"];

/// Checks every crate of [`PACKAGES`] on every one of [`TARGETS`], and fails
/// with the list of those that need a newer Rust.
pub fn check() -> Result {
    let mut problems = BTreeSet::new();
    for target in TARGETS {
        let metadata = metadata(target)?;
        for package in PACKAGES {
            problems.extend(too_new(&metadata, package)?);
        }
    }
    if problems.is_empty() {
        println!(
            "Every crate of {} builds with their rust-version.",
            PACKAGES.join(" and ")
        );
        return Ok(());
    }
    Err(format!(
        "these crates need a newer Rust than the workspace's rust-version, which builds the \
         Flatpak (docs/CI.md):\n  {}",
        problems.into_iter().collect::<Vec<_>>().join("\n  ")
    ))
}

/// The crates `package` is built from, build scripts' dependencies included,
/// whose `rust-version` is newer than `package`'s own, sorted, each one
/// described in a line.
fn too_new(metadata: &Value, package: &str) -> Result<Vec<String>> {
    let crates = built_from(metadata, package, true)?;
    let own = crates
        .iter()
        .find(|krate| string(&krate["name"]) == package && krate["source"].is_null())
        .and_then(|krate| krate["rust_version"].as_str())
        .ok_or_else(|| format!("{package} has no rust-version"))?;
    let own_version = parse(own)?;
    let mut problems = Vec::new();
    for krate in crates {
        let Some(needed) = krate["rust_version"].as_str() else {
            continue;
        };
        if parse(needed)? > own_version {
            problems.push(format!(
                "{} {} needs Rust {needed} for {package}, which has {own}",
                string(&krate["name"]),
                string(&krate["version"]),
            ));
        }
    }
    problems.sort();
    Ok(problems)
}

/// A `rust-version` such as "1.98" or "1.98.1" as major, minor and patch
/// numbers, a missing patch counting as 0.
fn parse(version: &str) -> Result<[u64; 3]> {
    let mut numbers = [0; 3];
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() > numbers.len() {
        return Err(format!("{version:?} is not a Rust version"));
    }
    for (number, part) in numbers.iter_mut().zip(parts) {
        *number = part
            .parse()
            .map_err(|_| format!("{version:?} is not a Rust version"))?;
    }
    Ok(numbers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `cargo metadata`'s output for a workspace whose `nubo` (Rust 1.98)
    /// depends on `helper` and, for its build script, on `generator`, and
    /// whose tests use `tester`, each needing `needs` of Rust.
    fn workspace(needs: &str) -> Value {
        json!({
            "packages": [
                {"id": "nubo", "name": "nubo", "version": "1.0.0", "source": null, "rust_version": "1.98"},
                {"id": "helper", "name": "helper", "version": "2.0.0", "source": "registry", "rust_version": needs},
                {"id": "generator", "name": "generator", "version": "3.0.0", "source": "registry", "rust_version": needs},
                {"id": "tester", "name": "tester", "version": "4.0.0", "source": "registry", "rust_version": "1.200"},
            ],
            "resolve": {"nodes": [
                {"id": "nubo", "deps": [
                    {"pkg": "helper", "dep_kinds": [{"kind": null}]},
                    {"pkg": "generator", "dep_kinds": [{"kind": "build"}]},
                    {"pkg": "tester", "dep_kinds": [{"kind": "dev"}]},
                ]},
                {"id": "helper", "deps": []},
                {"id": "generator", "deps": []},
                {"id": "tester", "deps": []},
            ]},
        })
    }

    #[test]
    fn crates_that_need_no_newer_rust_pass() {
        assert!(too_new(&workspace("1.98"), "nubo").unwrap().is_empty());
        assert!(too_new(&workspace("1.60.2"), "nubo").unwrap().is_empty());
    }

    #[test]
    fn crates_that_need_a_newer_rust_are_named_but_not_tests_ones() {
        assert_eq!(
            too_new(&workspace("1.98.1"), "nubo").unwrap(),
            [
                "generator 3.0.0 needs Rust 1.98.1 for nubo, which has 1.98",
                "helper 2.0.0 needs Rust 1.98.1 for nubo, which has 1.98",
            ]
        );
    }

    #[test]
    fn versions_compare_by_their_numbers() {
        assert!(parse("1.100").unwrap() > parse("1.98.3").unwrap());
        assert_eq!(parse("1.98").unwrap(), parse("1.98.0").unwrap());
        assert!(parse("1.98-beta").is_err());
        assert!(parse("1.2.3.4").is_err());
    }
}
