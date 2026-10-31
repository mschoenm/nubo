//! Loading ROMs that are packed in zip archives.
//!
//! `Emulator::load_rom` accepts zip files through [`extract_rom`]. The `zip`
//! crate is built with only its pure-Rust deflate backend (zlib-rs), so the
//! core still builds for WebAssembly (wasm32) without C dependencies.

use crate::error::LoadRomError;
use std::borrow::Cow;
use std::fmt::Display;
use std::io::{Cursor, Read};
use zip::ZipArchive;

/// The first bytes of every zip file.
const ZIP_SIGNATURE: &[u8] = b"PK\x03\x04";
const ROM_EXTENSIONS: [&str; 2] = [".gb", ".gbc"];
/// The largest ROM a Game Boy cartridge can hold (MBC5's limit). Stops a
/// malicious archive from unpacking into gigabytes.
const LARGEST_ROM_SIZE: u64 = 8 * 1024 * 1024;

fn invalid_archive(reason: impl Display) -> LoadRomError {
    LoadRomError::InvalidArchive(reason.to_string())
}

/// Whether the archive entry `name` is a ROM: it ends in `.gb` or `.gbc` and
/// is not one of the metadata files macOS Finder adds to the zip archives it
/// makes, which sit under `__MACOSX/` and whose file names start with `._`
/// (`__MACOSX/._Game.gb`, a few hundred bytes next to the real `Game.gb`).
fn is_rom_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let mut components = name.split(['/', '\\']);
    let file_name = components.next_back().unwrap_or_default();
    let is_macos_metadata =
        file_name.starts_with("._") || components.any(|folder| folder == "__macosx");
    !is_macos_metadata
        && ROM_EXTENSIONS
            .iter()
            .any(|extension| file_name.ends_with(extension))
}

/// Returns the ROM inside `data`. Plain ROMs are returned as they are; for a
/// zip archive, the first `.gb` or `.gbc` file in it is unpacked, skipping
/// the `__MACOSX/` and `._` metadata files macOS adds. Frontends use it to
/// look at the ROM itself, for example to take its checksum.
pub fn extract_rom(data: &[u8]) -> Result<Vec<u8>, LoadRomError> {
    extract_rom_borrowed(data, None).map(Cow::into_owned)
}

/// Like [`extract_rom`], but a plain ROM is borrowed from `data` instead of
/// copied, for callers that only look at it (such as reading the header).
/// With `prefix`, at most that many bytes of a zipped ROM are unpacked, for
/// callers that only look at its start; a ROM the archive says is larger
/// than 8 MiB fails all the same.
pub(crate) fn extract_rom_borrowed(
    data: &[u8],
    prefix: Option<u64>,
) -> Result<Cow<'_, [u8]>, LoadRomError> {
    if !data.starts_with(ZIP_SIGNATURE) {
        return Ok(Cow::Borrowed(data));
    }

    let mut archive = ZipArchive::new(Cursor::new(data)).map_err(invalid_archive)?;

    let rom_index = (0..archive.len())
        .find(
            |&index| matches!(archive.name_for_index(index), Some(Ok(name)) if is_rom_name(&name)),
        )
        .ok_or_else(|| invalid_archive("it does not contain a .gb or .gbc ROM"))?;

    let rom_file = archive.by_index(rom_index).map_err(invalid_archive)?;
    let too_large = || invalid_archive("the ROM in it is larger than 8 MiB");
    if rom_file.size() > LARGEST_ROM_SIZE {
        return Err(too_large());
    }
    let mut rom = Vec::new();
    rom_file
        .take(prefix.unwrap_or(LARGEST_ROM_SIZE + 1))
        .read_to_end(&mut rom)
        .map_err(invalid_archive)?;
    if rom.len() as u64 > LARGEST_ROM_SIZE {
        return Err(too_large());
    }

    Ok(Cow::Owned(rom))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    fn build_zip(files: &[(&str, &[u8], CompressionMethod)]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, contents, method) in files {
            writer
                .start_file(
                    *name,
                    SimpleFileOptions::default().compression_method(*method),
                )
                .unwrap();
            writer.write_all(contents).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn returns_plain_rom_unchanged() {
        let rom = vec![0x00, 0xC3, 0x50, 0x01];
        assert_eq!(extract_rom(&rom).unwrap(), rom);
        assert!(
            matches!(extract_rom_borrowed(&rom, None), Ok(Cow::Borrowed(borrowed)) if borrowed == rom)
        );
    }

    #[test]
    fn extracts_deflated_rom_from_zip() {
        let rom = vec![0x42; 0x8000];
        let zip = build_zip(&[("game.gb", &rom, CompressionMethod::Deflated)]);
        assert_eq!(extract_rom(&zip).unwrap(), rom);
    }

    #[test]
    fn unpacks_only_the_start_of_a_zipped_rom_when_asked() {
        let rom: Vec<u8> = (0..0x8000).map(|index| index as u8).collect();
        let zip = build_zip(&[("game.gb", &rom, CompressionMethod::Deflated)]);
        assert_eq!(
            extract_rom_borrowed(&zip, Some(0x150)).unwrap(),
            &rom[..0x150]
        );
    }

    #[test]
    fn a_zip_damaged_past_the_header_fails_only_the_whole_check() {
        let rom = vec![0x00; 0x8000];
        let zip = build_zip(&[("game.gb", &rom, CompressionMethod::Stored)]);
        assert!(crate::Emulator::check_rom(&zip).is_ok());

        let mut damaged = zip;
        let middle_of_the_rom = damaged.len() / 2;
        damaged[middle_of_the_rom] ^= 0xFF;
        assert!(crate::Emulator::read_rom_header(&damaged).is_ok());
        assert!(crate::Emulator::check_rom(&damaged).is_err());
    }

    #[test]
    fn fails_on_a_zipped_rom_over_8_mib_even_when_unpacking_its_start() {
        let rom = vec![0x00; LARGEST_ROM_SIZE as usize + 1];
        let zip = build_zip(&[("game.gb", &rom, CompressionMethod::Deflated)]);
        for prefix in [None, Some(0x150)] {
            let error = extract_rom_borrowed(&zip, prefix).unwrap_err();
            assert!(error.to_string().contains("larger than 8 MiB"));
        }
    }

    #[test]
    fn extracts_stored_rom_from_zip() {
        let rom = vec![0x24; 0x8000];
        let zip = build_zip(&[("game.gbc", &rom, CompressionMethod::Stored)]);
        assert_eq!(extract_rom(&zip).unwrap(), rom);
    }

    #[test]
    fn skips_files_that_are_not_roms() {
        let rom = vec![0x99; 0x8000];
        let zip = build_zip(&[
            ("readme.txt", b"Have fun!", CompressionMethod::Deflated),
            ("Roms/GAME.GBC", &rom, CompressionMethod::Deflated),
        ]);
        assert_eq!(extract_rom(&zip).unwrap(), rom);
    }

    #[test]
    fn skips_macos_metadata_files() {
        let rom = vec![0x77; 0x8000];
        let zip = build_zip(&[
            (
                "__MACOSX/._Game.gb",
                &[0x00; 256],
                CompressionMethod::Deflated,
            ),
            (
                "__MACOSX/Roms/Game.gbc",
                &[0x00; 256],
                CompressionMethod::Deflated,
            ),
            ("._Game.gb", &[0x00; 256], CompressionMethod::Deflated),
            ("Roms/._Game.gbc", &[0x00; 256], CompressionMethod::Deflated),
            ("Game.gb", &rom, CompressionMethod::Deflated),
        ]);
        assert_eq!(extract_rom(&zip).unwrap(), rom);
    }

    #[test]
    fn fails_when_zip_has_only_macos_metadata() {
        let zip = build_zip(&[(
            "__MACOSX/._Game.gb",
            &[0x00; 256],
            CompressionMethod::Deflated,
        )]);
        let error = extract_rom(&zip).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("does not contain a .gb or .gbc ROM")
        );
    }

    #[test]
    fn fails_when_zip_has_no_rom() {
        let zip = build_zip(&[("readme.txt", b"No game here", CompressionMethod::Deflated)]);
        let error = extract_rom(&zip).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("does not contain a .gb or .gbc ROM")
        );
    }

    #[test]
    fn fails_on_corrupt_zip() {
        let mut data = ZIP_SIGNATURE.to_vec();
        data.extend_from_slice(&[0xFF; 64]);
        assert!(matches!(
            extract_rom(&data),
            Err(LoadRomError::InvalidArchive(_))
        ));
    }
}
