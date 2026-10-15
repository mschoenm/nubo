//! Downloads the test ROMs and checks their SHA-256 checksums: a pinned
//! release of the c-sp/game-boy-test-roms collection, and the GB Emulator
//! Shootout's reference screenshots and the test ROMs the collection doesn't
//! have in the same version, at a pinned commit of its `main` branch. Each
//! step is skipped when its result is already on disk.

use crate::suites::Source;
use nubo_download::{download_all, has_checksum, sha256_hex};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

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
        .expect("the runner lives in tools/nubo-test-rom-runner");
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
        download_all(&[(archive.clone(), ARCHIVE_URL.to_string())])?;
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
/// the directory that holds them. The marker of a complete download holds the
/// manifest's checksum, so files added to the manifest are downloaded too.
fn ensure_shootout() -> Result<PathBuf, String> {
    let shootout_dir = work_dir().join(format!("gb-emulator-shootout-{}", &SHOOTOUT_COMMIT[..12]));
    let manifest_checksum = sha256_hex(SHOOTOUT_MANIFEST.as_bytes());
    let marker = shootout_dir.join(COMPLETE_MARKER);
    if fs::read_to_string(&marker).is_ok_and(|contents| contents == manifest_checksum) {
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
        download_all(&missing)?;
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

    fs::write(&marker, manifest_checksum)
        .map_err(|error| format!("failed to write to {}: {error}", shootout_dir.display()))?;
    Ok(shootout_dir)
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
