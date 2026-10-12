//! Tests for scanning library folders. They write real files in the
//! temporary directory, so they only run in the native build.

use super::*;
use crate::settings::LibraryFolder;
use std::fs;
use std::process;

const TITLE_ADDRESS: usize = 0x134;
const CGB_FLAG_ADDRESS: usize = 0x143;

/// An empty folder in the temporary directory.
fn temporary_folder(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("nubo-library-{}-{}", name, process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    folder
}

/// Writes a 32 KiB ROM-only cartridge with the given title.
fn write_rom(path: &Path, title: &str, supports_cgb: bool) {
    let mut rom = vec![0; 0x8000];
    rom[TITLE_ADDRESS..TITLE_ADDRESS + title.len()].copy_from_slice(title.as_bytes());
    if supports_cgb {
        rom[CGB_FLAG_ADDRESS] = 0x80;
    }
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, rom).unwrap();
}

fn folder(path: &Path, include_subfolders: bool) -> LibraryFolder {
    LibraryFolder {
        path: path.to_path_buf(),
        include_subfolders,
    }
}

fn names(result: &ScanResult) -> Vec<String> {
    result.entries.iter().map(LibraryEntry::name).collect()
}

#[test]
fn finds_roms_and_reads_their_headers() {
    let root = temporary_folder("headers");
    write_rom(&root.join("Tetris.gb"), "TETRIS", false);
    write_rom(&root.join("Zelda.GBC"), "ZELDA", true);
    fs::write(root.join("notes.txt"), "not a rom").unwrap();
    fs::write(root.join("broken.gb"), "too small").unwrap();

    let result = scan::scan(1, &[folder(&root, false)], Vec::new());

    assert_eq!(names(&result), ["Tetris", "Zelda"]);
    assert_eq!(result.entries[0].title, "TETRIS");
    assert!(!result.entries[0].supports_cgb);
    assert!(result.entries[1].supports_cgb);
    assert!(result.entries.iter().all(|entry| entry.in_folder));
}

#[test]
fn looks_in_subfolders_only_when_asked() {
    let root = temporary_folder("subfolders");
    write_rom(&root.join("Top.gb"), "TOP", false);
    write_rom(
        &root.join("nested").join("deeper").join("Deep.gb"),
        "DEEP",
        false,
    );
    write_rom(&root.join(".hidden").join("Hidden.gb"), "HIDDEN", false);

    let flat = scan::scan(1, &[folder(&root, false)], Vec::new());
    let recursive = scan::scan(2, &[folder(&root, true)], Vec::new());

    assert_eq!(names(&flat), ["Top"]);
    let mut found = names(&recursive);
    found.sort();
    assert_eq!(found, ["Deep", "Top"]);
}

#[test]
fn reports_missing_folders() {
    let root = temporary_folder("missing");
    let missing = root.join("gone");

    let result = scan::scan(1, &[folder(&missing, true)], Vec::new());

    assert!(result.entries.is_empty());
    assert_eq!(result.missing_folders, [missing]);
}

#[test]
fn keeps_recently_played_games_outside_the_folders() {
    let root = temporary_folder("recent");
    let elsewhere = root.join("elsewhere").join("Mario.gb");
    write_rom(&elsewhere, "MARIO", false);
    let library_folder = root.join("library");
    fs::create_dir_all(&library_folder).unwrap();

    let mut played =
        scan::scan(1, &[folder(elsewhere.parent().unwrap(), false)], Vec::new()).entries;
    played[0].last_played = Some(1_000);

    let result = scan::scan(2, &[folder(&library_folder, false)], played);

    assert_eq!(names(&result), ["Mario"]);
    assert!(!result.entries[0].in_folder);
    assert_eq!(result.entries[0].last_played, Some(1_000));
}

#[test]
fn search_matches_the_name_or_the_title() {
    let root = temporary_folder("search");
    write_rom(&root.join("Pocket Monsters.gb"), "POKEMON RED", false);

    let entry = &scan::scan(1, &[folder(&root, false)], Vec::new()).entries[0];

    assert!(entry.matches("pocket"));
    assert!(entry.matches("Pokemon"));
    assert!(!entry.matches("zelda"));
}

#[test]
fn a_played_game_whose_file_was_deleted_is_forgotten() {
    let root = temporary_folder("deleted");
    let rom = root.join("elsewhere").join("Mario.gb");
    write_rom(&rom, "MARIO", false);
    let mut known = scan::scan(1, &[folder(rom.parent().unwrap(), false)], Vec::new()).entries;
    known[0].last_played = Some(1_000);
    fs::remove_file(&rom).unwrap();

    let result = scan::scan(2, &[], known.clone());
    let merged = merge_scan(&known, result.entries, &result.checked);

    assert!(merged.is_empty());
    assert!(result.unavailable.is_empty());
}

#[test]
fn games_on_a_folder_that_is_gone_are_kept_but_unavailable() {
    let root = temporary_folder("unplugged");
    let drive = root.join("drive");
    write_rom(&drive.join("Tetris.gb"), "TETRIS", false);
    let folders = [folder(&drive, false)];
    let known = scan::scan(1, &folders, Vec::new()).entries;
    fs::remove_dir_all(&drive).unwrap();

    let result = scan::scan(2, &folders, known);

    assert_eq!(names(&result), ["Tetris"]);
    assert_eq!(result.unavailable, [drive.join("Tetris.gb")]);
    assert_eq!(result.missing_folders, [drive]);
}

#[test]
fn games_started_during_a_scan_are_kept() {
    let started_during_scan = LibraryEntry::example("/elsewhere/New.gb");
    let checked_and_gone = LibraryEntry::example("/games/Old.gb");
    let checked = std::collections::HashSet::from([checked_and_gone.path.clone()]);

    let merged = merge_scan(
        &[started_during_scan.clone(), checked_and_gone],
        Vec::new(),
        &checked,
    );

    assert_eq!(merged, [started_during_scan]);
}

#[test]
fn the_quick_check_tells_deleted_games_from_unreachable_ones() {
    let root = temporary_folder("check");
    let library_folder = root.join("library");
    let drive = root.join("drive");
    write_rom(&library_folder.join("Kept.gb"), "KEPT", false);
    write_rom(&library_folder.join("Deleted.gb"), "DELETED", false);
    write_rom(&drive.join("Unplugged.gb"), "UNPLUGGED", false);
    let folders = [folder(&library_folder, false), folder(&drive, false)];
    let entries = scan::scan(1, &folders, Vec::new()).entries;
    fs::remove_file(library_folder.join("Deleted.gb")).unwrap();
    fs::remove_dir_all(&drive).unwrap();

    let check = scan::check(&entries, &folders);

    assert_eq!(check.deleted, [library_folder.join("Deleted.gb")]);
    assert_eq!(check.unavailable, [drive.join("Unplugged.gb")]);
}

#[test]
fn only_one_scan_runs_and_requests_during_it_lead_to_one_more() {
    let mut library = Library::default();

    assert!(library.start_scan(Vec::new()).is_some());
    assert!(library.start_scan(Vec::new()).is_none());
    assert!(library.start_scan(Vec::new()).is_none());

    assert!(library.take_rescan_wanted());
    assert!(!library.take_rescan_wanted());
}

#[test]
fn a_scan_that_stopped_early_keeps_the_games_and_lets_the_next_scan_start() {
    let mut library = Library {
        entries: vec![LibraryEntry::example("/games/Tetris.gb")],
        ..Library::default()
    };
    assert!(library.start_scan(Vec::new()).is_some());

    let result = library.finish_scan(ScanResult::failed(
        library.scan_generation,
        String::from("it stopped unexpectedly"),
    ));

    assert!(result.is_err());
    assert_eq!(library.entries.len(), 1);
    assert!(library.start_scan(Vec::new()).is_some());
}

#[test]
fn a_file_check_older_than_the_last_scan_is_ignored() {
    let mut library = Library::default();
    let unplugged = PathBuf::from("/drive/Tetris.gb");
    library.finished_scans = 1;

    library
        .finish_check(FileCheck {
            unavailable: vec![unplugged.clone()],
            finished_scans: 0,
            ..FileCheck::default()
        })
        .unwrap();

    assert!(!library.unavailable.contains(&unplugged));
}
