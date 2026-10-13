//! `cargo xtask benchmark-roms`: downloads the free homebrew games the
//! nightly CI measures speed, instructions and battery use with, from the
//! gbdev Homebrew Hub database at pinned commits, and checks their SHA-256
//! checksums. The games aren't part of Nubo; they are only downloaded to
//! run them.

use crate::{Result, target_dir};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A homebrew game to download.
struct BenchmarkRom {
    file: &'static str,
    url: &'static str,
    sha256: &'static str,
}

/// The games, chosen to load the emulator in different ways and licensed so
/// anyone may use them.
///
/// `cargo save-states` keeps save states of these games, and a state only
/// loads into the version of the game it was made with, so a game whose
/// states are kept must stay here at the same version.
const ROMS: [BenchmarkRom; 3] = [
    // Tobu Tobu Girl Deluxe by Tangram Games (MIT, assets CC BY 4.0): a
    // platformer that runs on both the Game Boy and the Game Boy Color.
    BenchmarkRom {
        file: "tobudx.gb",
        url: "https://raw.githubusercontent.com/gbdev/database/732474cd67a26c4260a3ec0be4a1461daf140f59/entries/tobutobugirldeluxe/tobudx.gb",
        sha256: "0a0e8018dbbc8d7f8cd99f05e7cdc7b4cc9e358ecfe9377ebfb2291a84c6e310",
    },
    // µCity by Antonio Niño Díaz (GPL-3.0-or-later): a city builder whose
    // simulation keeps the CPU busy.
    BenchmarkRom {
        file: "ucity.gbc",
        url: "https://raw.githubusercontent.com/gbdev/database/eee749dcd38052be3cb00a47ca03690603eb163e/entries/ucity/ucity.gbc",
        sha256: "0d608db175dd8ba22780be5df5fc17bf135876f6964f58ee141e4f8d81c62947",
    },
    // Geometrix by Antonio Niño Díaz (GPL-3.0-or-later): a puzzle game for
    // the Game Boy Color.
    BenchmarkRom {
        file: "geometrix.gbc",
        url: "https://raw.githubusercontent.com/gbdev/database/eee749dcd38052be3cb00a47ca03690603eb163e/entries/geometrix/geometrix.gbc",
        sha256: "f13575af6bf87003c3eef8c2cab67a49ca6b02aa23ff6a31a38beb96d6eb1795",
    },
];

/// Makes sure every benchmark game is in `target/benchmark-roms` with the
/// right checksum, downloading the missing ones with curl, and returns the
/// folder.
pub fn ensure_benchmark_roms() -> Result<PathBuf> {
    let folder = target_dir().join("benchmark-roms");
    fs::create_dir_all(&folder)
        .map_err(|error| format!("failed to create {}: {error}", folder.display()))?;

    for rom in &ROMS {
        let path = folder.join(rom.file);
        if has_checksum(&path, rom.sha256) {
            continue;
        }
        let status = Command::new("curl")
            .args([
                "--fail",
                "--location",
                "--silent",
                "--show-error",
                "--output",
            ])
            .arg(&path)
            .arg(rom.url)
            .status()
            .map_err(|error| format!("failed to run curl: {error}"))?;
        if !status.success() {
            return Err(format!("failed to download {}", rom.url));
        }
        if !has_checksum(&path, rom.sha256) {
            let _ = fs::remove_file(&path);
            return Err(format!(
                "{} does not match its expected SHA-256 checksum",
                rom.file
            ));
        }
    }

    Ok(folder)
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
