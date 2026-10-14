//! Downloads the CPU tests at a pinned commit and checks them against the
//! SHA-256 checksums in `v2.sha256`.

use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
/// a time. The URLs and destinations go to curl as a config on its standard
/// input rather than as arguments, since hundreds of them would pass Windows'
/// limit on the length of a command line.
fn download(destination: &Path, entries: &[&ManifestEntry]) -> Result<(), String> {
    println!(
        "Downloading {} CPU test files from adtennant/GameboyCPUTests@{}",
        entries.len(),
        &COMMIT[..12]
    );

    let config = curl_config(destination, entries)?;
    let mut child = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--parallel",
            "--parallel-max",
            PARALLEL_DOWNLOADS,
            "--config",
            "-",
        ])
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to run curl: {error}"))?;
    let written = child
        .stdin
        .take()
        .expect("curl's standard input is piped")
        .write_all(config.as_bytes());
    let status = child
        .wait()
        .map_err(|error| format!("failed to run curl: {error}"))?;
    written.map_err(|error| format!("failed to pass the downloads to curl: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("curl exited with {status}"))
    }
}

/// A curl config that downloads each file of `entries` into `destination`:
/// a `url` line and an `output` line per file, with every value quoted.
fn curl_config(destination: &Path, entries: &[&ManifestEntry]) -> Result<String, String> {
    let mut config = String::new();
    for entry in entries {
        let output = destination.join(entry.file_name);
        let output = output
            .to_str()
            .ok_or_else(|| format!("{} is not valid UTF-8", output.display()))?;
        let url = format!("{}/{}/v2/{}", BASE_URL, COMMIT, entry.file_name);
        config.push_str(&format!(
            "url = {}\noutput = {}\n",
            curl_quote(&url),
            curl_quote(output)
        ));
    }
    Ok(config)
}

/// `value` in double quotes for a curl config, where a backslash starts an
/// escape, so the backslashes in Windows paths are doubled.
fn curl_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curl_config_pairs_each_url_with_its_quoted_destination() {
        let entry = ManifestEntry {
            sha256: "",
            file_name: "cb 00.json",
        };
        let config = curl_config(Path::new(r"C:\target"), &[&entry]).unwrap();
        assert_eq!(
            config,
            format!(
                "url = \"{BASE_URL}/{COMMIT}/v2/cb 00.json\"\noutput = \"C:\\\\target{}cb 00.json\"\n",
                if cfg!(windows) { r"\\" } else { "/" }
            )
        );
    }
}
