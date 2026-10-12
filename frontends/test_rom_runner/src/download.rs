//! Downloads the test ROMs and checks their SHA-256 checksums: a pinned
//! release of the c-sp/game-boy-test-roms collection, and the test ROMs of
//! the GB Emulator Shootout that the collection doesn't include, at a pinned
//! commit. Each step is skipped when its result is already on disk.

use crate::suites::Source;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const VERSION: &str = "v7.0";
const ARCHIVE_URL: &str =
    "https://github.com/c-sp/game-boy-test-roms/releases/download/v7.0/game-boy-test-roms-v7.0.zip";
const ARCHIVE_SHA256: &str = "b9a9d7a1075aa35a3d07c07c34974048672d8520dca9e07a50178f5860c3832c";
const SHOOTOUT_COMMIT: &str = "38b926bdbc26993d1b4c43e97979ecc66287bf02";
const SHOOTOUT_URL: &str = "https://raw.githubusercontent.com/gbdev/GBEmulatorShootout";
/// The shootout files the runner uses, one `<sha256>  <path>` line each, with
/// paths relative to the repository's `testroms` folder.
const SHOOTOUT_MANIFEST: &str = include_str!("../shootout.sha256");
/// A file written after a download or extraction succeeds, so an interrupted
/// one is redone on the next run.
const COMPLETE_MARKER: &str = ".complete";

/// The folders the test ROMs are in.
pub(crate) struct RomDirs {
    collection: PathBuf,
    shootout: PathBuf,
}

impl RomDirs {
    /// The folder the ROMs from `source` are in.
    pub(crate) fn root(&self, source: Source) -> &Path {
        match source {
            Source::Collection => &self.collection,
            Source::Shootout => &self.shootout,
        }
    }
}

/// `target/test-roms`, where the downloads, the ROMs and failure screens go.
pub(crate) fn work_dir() -> PathBuf {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the runner lives in frontends/test_rom_runner");
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| workspace_root.join("target"), PathBuf::from);
    target_dir.join("test-roms")
}

/// Makes sure all test ROMs are downloaded, and returns where they are.
pub(crate) fn ensure_test_roms() -> Result<RomDirs, String> {
    Ok(RomDirs {
        collection: ensure_collection()?,
        shootout: ensure_shootout()?,
    })
}

/// Makes sure the collection is downloaded and unpacked, and returns the
/// directory that holds it.
fn ensure_collection() -> Result<PathBuf, String> {
    let roms_dir = work_dir().join(VERSION);
    if roms_dir.join(COMPLETE_MARKER).exists() {
        return Ok(roms_dir);
    }

    fs::create_dir_all(work_dir())
        .map_err(|error| format!("failed to create {}: {error}", work_dir().display()))?;

    let archive = work_dir().join(format!("game-boy-test-roms-{VERSION}.zip"));
    if !has_checksum(&archive, ARCHIVE_SHA256) {
        println!("Downloading {ARCHIVE_URL}");
        curl(&[(archive.clone(), ARCHIVE_URL.to_string())])?;
        if !has_checksum(&archive, ARCHIVE_SHA256) {
            let _ = fs::remove_file(&archive);
            return Err(format!(
                "{ARCHIVE_URL} does not match the expected SHA-256 checksum"
            ));
        }
    }

    extract(&archive, &roms_dir)?;
    Ok(roms_dir)
}

/// Makes sure the shootout files in the manifest are downloaded, and returns
/// the directory that holds them.
fn ensure_shootout() -> Result<PathBuf, String> {
    let shootout_dir = work_dir().join(format!("gb-emulator-shootout-{}", &SHOOTOUT_COMMIT[..12]));
    if shootout_dir.join(COMPLETE_MARKER).exists() {
        return Ok(shootout_dir);
    }

    let entries: Vec<(&str, &str)> = SHOOTOUT_MANIFEST
        .lines()
        .filter_map(|line| line.split_once("  "))
        .collect();
    let missing: Vec<(PathBuf, String)> = entries
        .iter()
        .filter(|(sha256, path)| !has_checksum(&shootout_dir.join(path), sha256))
        .map(|(_, path)| {
            (
                shootout_dir.join(path),
                format!("{SHOOTOUT_URL}/{SHOOTOUT_COMMIT}/testroms/{path}"),
            )
        })
        .collect();
    if !missing.is_empty() {
        println!(
            "Downloading {} files from gbdev/GBEmulatorShootout@{}",
            missing.len(),
            &SHOOTOUT_COMMIT[..12]
        );
        for (destination, _) in &missing {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
            }
        }
        curl(&missing)?;
    }

    let corrupt: Vec<&str> = entries
        .iter()
        .filter(|(sha256, path)| !has_checksum(&shootout_dir.join(path), sha256))
        .map(|(_, path)| *path)
        .collect();
    if !corrupt.is_empty() {
        for path in &corrupt {
            let _ = fs::remove_file(shootout_dir.join(path));
        }
        return Err(format!(
            "{} shootout files do not match their expected SHA-256 checksum: {}",
            corrupt.len(),
            corrupt.join(", ")
        ));
    }

    fs::write(shootout_dir.join(COMPLETE_MARKER), SHOOTOUT_COMMIT)
        .map_err(|error| format!("failed to write to {}: {error}", shootout_dir.display()))?;
    Ok(shootout_dir)
}

/// Whether the file at `path` exists and has the SHA-256 checksum `expected`.
fn has_checksum(path: &Path, expected: &str) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    let checksum: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    checksum == expected
}

/// Downloads each URL to its destination with curl, which ships with
/// Windows, macOS and most Linux distributions.
fn curl(downloads: &[(PathBuf, String)]) -> Result<(), String> {
    let mut command = Command::new("curl");
    command.args(["--fail", "--location", "--silent", "--show-error"]);
    if downloads.len() > 1 {
        command.arg("--parallel");
    }
    for (destination, url) in downloads {
        command.arg("--output").arg(destination).arg(url);
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

/// Unpacks the archive into a fresh `destination` and marks it complete.
fn extract(archive: &Path, destination: &Path) -> Result<(), String> {
    let _ = fs::remove_dir_all(destination);

    let file = fs::File::open(archive)
        .map_err(|error| format!("failed to open {}: {error}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|error| format!("failed to read {}: {error}", archive.display()))?;
    zip.extract(destination)
        .map_err(|error| format!("failed to extract {}: {error}", archive.display()))?;

    fs::write(destination.join(COMPLETE_MARKER), VERSION)
        .map_err(|error| format!("failed to write to {}: {error}", destination.display()))
}
