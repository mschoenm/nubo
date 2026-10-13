//! Downloads the CPU tests at a pinned commit and checks them against the
//! SHA-256 checksums in `v2.sha256`.

use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The commit of adtennant/GameboyCPUTests the tests are taken from.
const COMMIT: &str = "d1c8fd48c9e543995397fbd20e55bcbd524d029f";
const BASE_URL: &str = "https://raw.githubusercontent.com/adtennant/GameboyCPUTests";
/// Each test file's SHA-256 checksum and name, one per line, as written by
/// `sha256sum`.
const MANIFEST: &str = include_str!("../v2.sha256");
/// A file written once every test file has been downloaded and checked, so
/// later runs don't check them again.
const COMPLETE_MARKER: &str = ".complete";
const PARALLEL_DOWNLOADS: &str = "16";

/// One line of the manifest.
struct ManifestEntry {
    sha256: &'static str,
    file_name: &'static str,
}

/// Returns the directory with the test files, downloading the ones that are
/// missing or damaged first. Fails if a download fails or a file doesn't
/// match its checksum afterwards.
pub(crate) fn ensure_cpu_tests() -> Result<PathBuf, String> {
    let tests_dir = work_dir().join(&COMMIT[..12]).join("v2");
    if tests_dir.join(COMPLETE_MARKER).exists() {
        return Ok(tests_dir);
    }

    fs::create_dir_all(&tests_dir)
        .map_err(|error| format!("failed to create {}: {}", tests_dir.display(), error))?;

    let entries = manifest();
    let missing: Vec<&ManifestEntry> = entries
        .iter()
        .filter(|entry| !has_expected_checksum(&tests_dir.join(entry.file_name), entry.sha256))
        .collect();

    if !missing.is_empty() {
        download(&tests_dir, &missing)?;
    }

    let corrupt: Vec<&str> = entries
        .iter()
        .filter(|entry| !has_expected_checksum(&tests_dir.join(entry.file_name), entry.sha256))
        .map(|entry| entry.file_name)
        .collect();
    if !corrupt.is_empty() {
        for file_name in &corrupt {
            let _ = fs::remove_file(tests_dir.join(file_name));
        }
        return Err(format!(
            "{} files do not match their expected SHA-256 checksum: {}",
            corrupt.len(),
            corrupt.join(", ")
        ));
    }

    fs::write(tests_dir.join(COMPLETE_MARKER), COMMIT)
        .map_err(|error| format!("failed to write to {}: {}", tests_dir.display(), error))?;
    Ok(tests_dir)
}

/// `json-cpu-tests` in Cargo's target directory.
fn work_dir() -> PathBuf {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the runner lives in tools/nubo-json-test-runner");
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| workspace_root.join("target"), PathBuf::from);
    target_dir.join("json-cpu-tests")
}

/// The entries of `MANIFEST`.
fn manifest() -> Vec<ManifestEntry> {
    MANIFEST
        .lines()
        .filter_map(|line| line.split_once("  "))
        .map(|(sha256, file_name)| ManifestEntry { sha256, file_name })
        .collect()
}

/// Whether the file at `path` exists and has the SHA-256 checksum
/// `expected` (in lowercase hexadecimal).
fn has_expected_checksum(path: &Path, expected: &str) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    let checksum: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    checksum == expected
}

/// Downloads the files of `entries` into `destination` with curl, several at
/// a time.
fn download(destination: &Path, entries: &[&ManifestEntry]) -> Result<(), String> {
    println!(
        "Downloading {} CPU test files from adtennant/GameboyCPUTests@{}",
        entries.len(),
        &COMMIT[..12]
    );

    let mut command = Command::new("curl");
    command.args([
        "--fail",
        "--location",
        "--silent",
        "--show-error",
        "--parallel",
        "--parallel-max",
        PARALLEL_DOWNLOADS,
    ]);
    for entry in entries {
        command
            .arg("--output")
            .arg(destination.join(entry.file_name))
            .arg(format!("{}/{}/v2/{}", BASE_URL, COMMIT, entry.file_name));
    }

    let status = command
        .status()
        .map_err(|error| format!("failed to run curl: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("curl exited with {status}"))
    }
}
