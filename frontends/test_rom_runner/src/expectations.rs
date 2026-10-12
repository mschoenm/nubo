//! Reads and writes `passing.txt`, the list of tests that are expected to
//! pass. One test ID (such as `cgb blargg/halt_bug.gb`) per line.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const HEADER: &str = "# Test ROMs that pass. Update with `cargo test-roms --update`.";

/// The location of `passing.txt` in the runner's source directory.
pub(crate) fn path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("passing.txt")
}

/// Reads the IDs of the tests expected to pass, skipping blank lines and
/// `#` comments. A missing file means no test is expected to pass.
pub(crate) fn load() -> Result<BTreeSet<String>, String> {
    let contents = match fs::read_to_string(path()) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("failed to read {}: {error}", path().display())),
    };

    Ok(contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect())
}

/// Replaces `passing.txt` with the given test IDs, in sorted order.
pub(crate) fn save(passing: &BTreeSet<String>) -> Result<(), String> {
    let mut contents = String::from(HEADER);
    contents.push('\n');
    for id in passing {
        contents.push_str(id);
        contents.push('\n');
    }
    fs::write(path(), contents)
        .map_err(|error| format!("failed to write {}: {error}", path().display()))
}
