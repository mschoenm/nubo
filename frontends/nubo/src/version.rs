//! Nubo's version as the app shows it: the version from `Cargo.toml`,
//! and for a build `cargo xtask` made (every release, nightly build and
//! package) the commit it was built from and that commit's date. The About
//! tab shows them together, as `0.10.0 (7c44d39 2026-10-16)`; `--version`
//! puts the commit on a line of its own under the version
//! (`command_line`). A commit followed by `-dirty`, as `git describe
//! --dirty` writes it, was built with changes that weren't committed. A
//! plain `cargo build` knows neither, and shows only `0.10.0`.

use std::sync::LazyLock;

/// The version with the build's commit and date when they are known, as
/// the About tab shows it.
pub static VERSION: LazyLock<String> = LazyLock::new(|| {
    let version = env!("CARGO_PKG_VERSION");
    match &*COMMIT {
        Some(commit) => format!("{version} ({})", commit.summary()),
        None => String::from(version),
    }
});

/// The commit this build was made from, when `cargo xtask` built it.
pub static COMMIT: LazyLock<Option<Commit>> =
    LazyLock::new(|| Commit::read(nubo_core::BUILD_COMMIT, COMMIT_DATE));

/// The date of the commit this build was made from, such as `2026-10-16`,
/// which `cargo xtask` sets through `NUBO_COMMIT_DATE`; empty when it
/// isn't known.
const COMMIT_DATE: &str = match option_env!("NUBO_COMMIT_DATE") {
    Some(date) => date,
    None => "",
};

/// The commit of Nubo's source code a build was made from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// The commit's full hash.
    pub hash: String,
    /// The commit's date, such as `2026-10-16`, or empty when it isn't
    /// known.
    pub date: String,
    /// Whether the source had changes that weren't committed.
    pub dirty: bool,
    /// The commit's page on GitHub.
    pub page: String,
}

impl Commit {
    /// Reads a commit as `nubo_core::BUILD_COMMIT` writes it (the hash,
    /// with `-dirty` after it for changes that weren't committed), with
    /// its date. `None` when `commit` is empty.
    fn read(commit: &str, date: &str) -> Option<Commit> {
        if commit.is_empty() {
            return None;
        }
        let (hash, dirty) = match commit.strip_suffix("-dirty") {
            Some(hash) => (hash, true),
            None => (commit, false),
        };
        Some(Commit {
            hash: hash.to_owned(),
            date: date.to_owned(),
            dirty,
            page: format!("{}/commit/{hash}", env!("CARGO_PKG_REPOSITORY")),
        })
    }

    /// The hash's first seven digits, as GitHub shortens it.
    pub fn short_hash(&self) -> &str {
        self.hash.get(..7).unwrap_or(&self.hash)
    }

    /// The short hash, followed by `-dirty` for a build with changes that
    /// weren't committed, and the date, such as `7c44d39 2026-10-16`, as
    /// the version shows them in brackets.
    fn summary(&self) -> String {
        let mut summary = self.short_hash().to_owned();
        if self.dirty {
            summary.push_str("-dirty");
        }
        if !self.date.is_empty() {
            summary.push(' ');
            summary.push_str(&self.date);
        }
        summary
    }
}

#[cfg(test)]
mod tests;
