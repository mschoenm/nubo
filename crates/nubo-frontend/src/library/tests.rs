//! Tests for scanning library folders. They write real files in the
//! temporary directory.

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
fn games_taken_off_the_recently_played_during_a_scan_stay_off() {
    let mut played = LibraryEntry::example("/games/Tetris.gb");
    played.last_played = Some(1_000);
    let checked = std::collections::HashSet::from([played.path.clone()]);
    let taken_off = LibraryEntry {
        last_played: None,
        ..played.clone()
    };

    let merged = merge_scan(std::slice::from_ref(&taken_off), vec![played], &checked);

    assert_eq!(merged, [taken_off]);
}

#[test]
fn a_game_the_scan_found_stays_although_the_library_dropped_it_meanwhile() {
    let mut found = LibraryEntry::example("/elsewhere/Mario.gb");
    found.in_folder = false;
    found.last_played = Some(2_000);
    let checked = std::collections::HashSet::from([found.path.clone()]);

    let merged = merge_scan(&[], vec![found.clone()], &checked);

    assert_eq!(merged, [found]);
}

/// A library kept in a new temporary folder, with one game in a library
/// folder and one opened from elsewhere, both played.
#[cfg(feature = "desktop")]
fn library_with_two_played_games(name: &str) -> Library {
    let mut in_folder = LibraryEntry::example("/games/Tetris.gb");
    in_folder.last_played = Some(1_000);
    let mut elsewhere = LibraryEntry::example("/elsewhere/Mario.gb");
    elsewhere.in_folder = false;
    elsewhere.last_played = Some(2_000);

    let mut library = Library {
        cache: Cache::in_folder(temporary_folder(name)),
        ..Library::default()
    };
    library.replace_entries(vec![in_folder, elsewhere]);
    library
}

#[cfg(feature = "desktop")]
#[test]
fn a_game_in_a_library_folder_taken_off_the_recently_played_stays_among_all_games() {
    let mut library = library_with_two_played_games("forget-in-folder");

    library
        .forget_played(Path::new("/games/Tetris.gb"))
        .unwrap();

    let recent: Vec<_> = library
        .recently_played(4)
        .iter()
        .map(|e| e.name())
        .collect();
    let games: Vec<_> = library.games("").map(LibraryEntry::name).collect();
    assert_eq!(recent, ["Mario"]);
    assert_eq!(games, ["Tetris"]);
}

#[cfg(feature = "desktop")]
#[test]
fn a_game_from_elsewhere_taken_off_the_recently_played_is_forgotten_at_the_next_scan() {
    let root = temporary_folder("forget-elsewhere-scan");
    let rom = root.join("elsewhere").join("Mario.gb");
    write_rom(&rom, "MARIO", false);
    let mut library = Library {
        cache: Cache::in_folder(root.join("data")),
        ..Library::default()
    };
    let mut known = scan::scan(1, &[folder(rom.parent().unwrap(), false)], Vec::new()).entries;
    known[0].in_folder = false;
    known[0].last_played = Some(1_000);
    library.replace_entries(known);

    library.forget_played(&rom).unwrap();

    assert!(library.recently_played(4).is_empty());
    let result = scan::scan(2, &[], library.entries.clone());
    assert!(result.entries.is_empty());
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

    assert_eq!(
        check.deleted,
        HashSet::from([library_folder.join("Deleted.gb")])
    );
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

/// The library file that `tests/existing_data_folder.rs` reads was written
/// before the library moved to this crate. Its games are put at new paths,
/// with the size and modification time the file has for them but a header
/// that says something else: a scan that read them again would see that,
/// so the scan must keep what the file knows.
#[cfg(feature = "desktop")]
#[test]
fn a_scan_keeps_what_an_existing_library_file_knows_about_unchanged_games() {
    use std::time::{Duration, UNIX_EPOCH};

    let data_folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/existing_data_folder/data");
    let known = store::load_entries(&Cache::in_folder(data_folder)).unwrap();
    assert_eq!(known.len(), 5);

    let root = temporary_folder("existing");
    let known: Vec<LibraryEntry> = known
        .into_iter()
        .map(|mut entry| {
            let path = root.join(entry.path.file_name().unwrap());
            write_rom(&path, "CHANGED", false);
            assert_eq!(fs::metadata(&path).unwrap().len(), entry.size);
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(UNIX_EPOCH + Duration::from_secs(entry.modified))
                .unwrap();
            entry.path = path;
            entry
        })
        .collect();

    let result = scan::scan(1, &[folder(&root, false)], known.clone());

    let mut expected: Vec<LibraryEntry> = known
        .into_iter()
        .map(|entry| LibraryEntry {
            in_folder: true,
            ..entry
        })
        .collect();
    expected.sort_by(|first, second| first.path.cmp(&second.path));
    assert_eq!(result.entries, expected);
}

#[test]
fn a_model_picked_during_a_scan_stays() {
    let scanned = LibraryEntry::example("/games/Tetris.gb");
    let mut picked = scanned.clone();
    picked.model = Some(ModelPreference::Fixed(Model::Sgb2));

    let merged = merge_scan(
        std::slice::from_ref(&picked),
        vec![scanned],
        &std::collections::HashSet::new(),
    );

    assert_eq!(merged[0].model(), Some(Model::Sgb2));
}

#[cfg(feature = "desktop")]
#[test]
fn a_reset_forgets_the_picked_models_and_the_play_times_each_as_asked() {
    let mut library = library_with_two_played_games("reset-models");
    library
        .set_model(
            Path::new("/games/Tetris.gb"),
            Some(ModelPreference::Fixed(Model::Sgb2)),
        )
        .unwrap();

    library.reset_games(false, true).unwrap();

    assert_eq!(
        library
            .entry(Path::new("/games/Tetris.gb"))
            .unwrap()
            .model(),
        Some(Model::Sgb2)
    );
    assert!(library.recently_played(4).is_empty());
    assert!(!library.any_played());

    library.reset_games(true, false).unwrap();

    assert_eq!(
        library
            .entry(Path::new("/games/Tetris.gb"))
            .unwrap()
            .model(),
        None
    );
}

#[cfg(feature = "desktop")]
#[test]
fn a_game_on_a_drive_that_is_gone_still_counts_as_played() {
    let mut library = library_with_two_played_games("played-unavailable");
    library.unavailable = library
        .entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect();

    assert!(library.recently_played(4).is_empty());
    assert!(library.any_played());
}

#[cfg(feature = "desktop")]
#[test]
fn a_restored_snapshot_brings_back_the_games_and_ignores_the_running_scan() {
    let mut library = library_with_two_played_games("restore");
    library
        .set_model(
            Path::new("/games/Tetris.gb"),
            Some(ModelPreference::Fixed(Model::Sgb2)),
        )
        .unwrap();
    let snapshot = library.snapshot();
    library.reset_games(true, true).unwrap();
    let scan = library.start_scan(Vec::new());
    assert!(scan.is_some());
    let generation = library.scan_generation;

    library.restore(snapshot).unwrap();
    library
        .finish_scan(ScanResult {
            generation,
            entries: Vec::new(),
            missing_folders: Vec::new(),
            unavailable: Vec::new(),
            checked: library
                .entries
                .iter()
                .map(|entry| entry.path.clone())
                .collect(),
            error: None,
        })
        .unwrap();

    assert_eq!(library.recently_played(4).len(), 2);
    assert_eq!(
        library
            .entry(Path::new("/games/Tetris.gb"))
            .unwrap()
            .model(),
        Some(Model::Sgb2)
    );
    assert!(!library.is_scanning());
}

#[test]
fn a_library_that_keeps_its_games_finds_those_whose_file_it_keeps() {
    let mut known = LibraryEntry::example("Tetris.gb");
    known.last_played = Some(1_000);

    let gone = LibraryEntry::example("Refused.gb");

    let result = scan::kept_games(7, vec![known.clone(), gone], |path| {
        path == Path::new("Tetris.gb")
    });

    assert_eq!(result.generation, 7);
    assert_eq!(result.entries, vec![known]);
    assert!(result.checked.contains(Path::new("Tetris.gb")));
    assert!(result.checked.contains(Path::new("Refused.gb")));
    assert!(result.missing_folders.is_empty());
}

#[cfg(feature = "desktop")]
#[test]
fn a_game_set_to_automatic_keeps_automatic_rather_than_the_settings() {
    let mut library = library_with_two_played_games("automatic");
    let path = Path::new("/games/Tetris.gb");
    library
        .set_model(path, Some(ModelPreference::Automatic))
        .unwrap();

    let entry = library.entry(path).unwrap();
    assert_eq!(entry.preference(), Some(ModelPreference::Automatic));
    assert_eq!(entry.model(), None);
}

#[cfg(feature = "desktop")]
#[test]
fn an_unreadable_library_file_is_kept_aside_before_a_save_can_replace_it() {
    let data_folder = temporary_folder("kept");
    let from_newer_nubo = "[[games]]\nmodel = \"Game Boy Advance SP\"\n";
    fs::write(data_folder.join("library.toml"), from_newer_nubo).unwrap();

    let (mut library, error) = Library::load_from(Cache::in_folder(data_folder.clone()));
    let error = error.unwrap();
    assert!(error.contains("library.kept-1.toml"), "{error}");
    library.reset_games(true, true).unwrap();

    assert_eq!(
        fs::read_to_string(data_folder.join("library.kept-1.toml")).unwrap(),
        from_newer_nubo
    );
    assert!(data_folder.join("library.toml").exists());
}
