//! Tests for which game owns which stored name. They write real ROM files
//! and records in the temporary directory.

use super::*;
use crate::storage::Entry;
use crate::storage::Storage;
use crate::storage::files::Files;

/// An empty folder in the temporary directory, with `roms/` for the games
/// and `data/` for what Nubo keeps.
fn temporary_folder(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("nubo-names-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    folder
}

/// Writes a ROM file at `path`, making its folder.
fn write_rom(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"rom").unwrap();
}

/// No folder is out of reach.
fn reachable(_path: &Path) -> bool {
    false
}

#[test]
fn the_hash_of_a_folder_never_changes() {
    // Players' saves are named after it: these values must stay.
    assert_eq!(hashed_name("game.gb", "", 0), "game (811c9dc5).gb");
    assert_eq!(hashed_name("game.gb", "a", 0), "game (e40c292c).gb");
    assert_eq!(
        hashed_name("Pocket Game.gbc", "/home/player/Jam B", 0),
        format!(
            "Pocket Game ({:08x}).gbc",
            crate::storage::fnv1a(b"/home/player/Jam B")
        )
    );
    assert_eq!(
        hashed_name("game.gb", "a", 1),
        format!("game ({:08x}).gb", crate::storage::fnv1a(b"a\n1"))
    );
    assert_eq!(hashed_name("game", "a", 0), "game (e40c292c)");
    assert_eq!(
        crate::rom::display_name(Path::new(&hashed_name("Tetris.gb", "/roms", 0))),
        "Tetris"
    );
}

#[test]
fn the_first_game_keeps_the_plain_name_and_another_gets_its_own() {
    let folder = temporary_folder("first");
    let first = folder.join("Jam A").join("game.gb");
    let second = folder.join("Jam B").join("game.gb");
    write_rom(&first);
    write_rom(&second);
    let mut names = Names::default();

    assert_eq!(names.stored_path(&second, false), second);
    assert_eq!(
        names.claim(&first, "GAME", reachable),
        (first.clone(), How::Plain)
    );
    assert_eq!(
        names.claim(&first, "GAME", reachable),
        (first.clone(), How::Owned)
    );

    let own = second.with_file_name(hashed_name(
        "game.gb",
        &folder.join("Jam B").to_string_lossy(),
        0,
    ));
    assert_eq!(names.stored_path(&second, false), own);
    assert_eq!(
        names.claim(&second, "OTHER", reachable),
        (own.clone(), How::OwnName)
    );
    assert_eq!(
        names.claim(&second, "OTHER", reachable),
        (own.clone(), How::Owned)
    );
    assert_eq!(names.stored_path(&first, false), first);
    assert_eq!(names.stored_path(&second, false), own);
}

#[test]
fn a_copy_gets_names_of_its_own() {
    let folder = temporary_folder("copy");
    let original = folder.join("game.gb");
    let copy = folder.join("Backup").join("game.gb");
    write_rom(&original);
    let mut names = Names::default();
    names.claim(&original, "GAME", reachable);
    fs::create_dir_all(copy.parent().unwrap()).unwrap();
    fs::copy(&original, &copy).unwrap();

    let (stored, how) = names.claim(&copy, "GAME", reachable);

    assert_eq!(how, How::OwnName);
    assert_ne!(stored, copy);
    assert_eq!(names.stored_path(&original, false), original);
}

#[test]
fn a_game_moved_in_or_out_of_the_library_keeps_its_names() {
    let folder = temporary_folder("moved");
    let library = folder.join("Library");
    let before = library.join("Jam A").join("game.gb");
    let inside = library.join("Favorites").join("game.gb");
    let outside = folder.join("Downloads").join("game.gb");
    let other = library.join("Jam B").join("game.gb");
    write_rom(&before);
    write_rom(&other);
    let mut names = Names::default();
    names.claim(&before, "GAME", reachable);
    let (others_name, _) = names.claim(&other, "OTHER", reachable);

    fs::create_dir_all(inside.parent().unwrap()).unwrap();
    fs::rename(&before, &inside).unwrap();
    assert_eq!(
        names.claim(&inside, "GAME", reachable),
        (inside.clone(), How::TookOver)
    );

    fs::create_dir_all(outside.parent().unwrap()).unwrap();
    fs::rename(&inside, &outside).unwrap();
    assert_eq!(
        names.claim(&outside, "GAME", reachable),
        (outside.clone(), How::TookOver)
    );
    assert_eq!(names.stored_path(&outside, false), outside);

    // The other game with its own name keeps it when it moves too.
    let moved_other = folder.join("Elsewhere").join("game.gb");
    fs::create_dir_all(moved_other.parent().unwrap()).unwrap();
    fs::rename(&other, &moved_other).unwrap();
    assert_eq!(
        names.claim(&moved_other, "OTHER", reachable),
        (
            moved_other.with_file_name(others_name.file_name().unwrap()),
            How::TookOver
        )
    );
}

#[test]
fn a_moved_game_takes_the_name_of_its_own_title_when_several_are_gone() {
    let folder = temporary_folder("several-gone");
    let first = folder.join("A").join("game.gb");
    let second = folder.join("B").join("game.gb");
    write_rom(&first);
    write_rom(&second);
    let mut names = Names::default();
    names.claim(&first, "FIRST", reachable);
    let (second_name, _) = names.claim(&second, "SECOND", reachable);
    fs::remove_file(&first).unwrap();
    fs::remove_file(&second).unwrap();

    let moved = folder.join("C").join("game.gb");
    write_rom(&moved);

    assert_eq!(
        names.claim(&moved, "SECOND", reachable).0.file_name(),
        second_name.file_name()
    );
}

#[test]
fn a_game_on_an_unreachable_folder_keeps_its_name() {
    let folder = temporary_folder("unreachable");
    let unplugged = folder.join("USB stick");
    let owner = unplugged.join("game.gb");
    let other = folder.join("Disk").join("game.gb");
    write_rom(&owner);
    write_rom(&other);
    let mut names = Names::default();
    names.claim(&owner, "GAME", reachable);
    fs::remove_dir_all(&unplugged).unwrap();

    let on_unplugged = |path: &Path| path.starts_with(&unplugged);
    let (stored, how) = names.claim(&other, "GAME", on_unplugged);

    assert_eq!(how, How::OwnName);
    assert_ne!(stored, other);
    // The drive comes back: the game finds its name again.
    write_rom(&owner);
    assert_eq!(
        names.claim(&owner, "GAME", reachable),
        (owner.clone(), How::Owned)
    );
}

#[test]
fn looking_up_never_hands_on_a_name() {
    let folder = temporary_folder("look-up");
    let owner = folder.join("A").join("game.gb");
    let other = folder.join("B").join("game.gb");
    write_rom(&owner);
    write_rom(&other);
    let mut names = Names::default();
    names.claim(&owner, "GAME", reachable);
    fs::remove_file(&owner).unwrap();

    assert_ne!(names.stored_path(&other, false), other);
    // Before anyone claims it, a game that shares its file name in the
    // library doesn't use a plain name for its pictures.
    let unclaimed = folder.join("C").join("solo.gb");
    assert_eq!(Names::default().stored_path(&unclaimed, false), unclaimed);
    assert_ne!(Names::default().stored_path(&unclaimed, true), unclaimed);
}

#[test]
fn the_record_is_saved_and_read_back() {
    let folder = temporary_folder("record");
    let data = folder.join("data");
    let first = folder.join("A").join("game.gb");
    let second = folder.join("B").join("game.gb");
    write_rom(&first);
    write_rom(&second);

    assert_eq!(claim(&data, &first, "GAME", reachable).stored, first);
    let own = claim(&data, &second, "OTHER", reachable);
    assert_eq!(own.how, How::OwnName);
    assert_eq!(own.problem, None);
    forget_record(&data);

    assert_eq!(stored_path(&data, &second), own.stored);
    let names = Names::load(&data);
    assert_eq!(names.problem, None);
    assert_eq!(names.owners.len(), 2);
    let text = fs::read_to_string(data.join("names.toml")).unwrap();
    assert!(text.contains("[[names]]"), "{text}");
    assert!(text.contains("name = \"game.gb\""), "{text}");
    assert!(text.contains("title = \"OTHER\""), "{text}");
}

#[test]
fn a_record_that_can_t_be_parsed_is_kept_aside() {
    let folder = temporary_folder("unparsable");
    let data = folder.join("data");
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("names.toml"), "names = 3").unwrap();

    let names = Names::load(&data);

    assert!(names.owners.is_empty());
    assert!(!names.read_only);
    assert!(names.problem.unwrap().contains("names.kept-1.toml"));
    assert_eq!(
        fs::read_to_string(data.join("names.kept-1.toml")).unwrap(),
        "names = 3"
    );
}

#[test]
fn battery_saves_and_cheats_go_by_the_name_the_game_owns() {
    let folder = temporary_folder("files");
    let data = folder.join("data");
    let first = folder.join("A").join("game.gb");
    let second = folder.join("B").join("game.gb");
    write_rom(&first);
    write_rom(&second);
    let files = Files::in_folders(None, Some(data.clone()));

    claim(&data, &first, "GAME", reachable);
    let own = claim(&data, &second, "OTHER", reachable).stored;
    files.write(Entry::BatteryRam(&first), b"first").unwrap();
    files.write(Entry::BatteryRam(&second), b"second").unwrap();
    files.write(Entry::Cheats(&second), b"cheats").unwrap();

    let stem = own.file_stem().unwrap().to_string_lossy().into_owned();
    let saves = data.join("saves");
    assert_eq!(fs::read(saves.join("game.sav")).unwrap(), b"first");
    assert_eq!(
        fs::read(saves.join(format!("{stem}.sav"))).unwrap(),
        b"second"
    );
    assert!(
        data.join("cheats")
            .join(format!("{stem}.gb.toml"))
            .is_file()
    );
}

#[test]
fn a_gone_game_s_name_isn_t_taken_over_by_another_title() {
    let folder = temporary_folder("other-title");
    let deleted = folder.join("jam1").join("game.gb");
    let other = folder.join("jam2").join("game.gb");
    write_rom(&deleted);
    let mut names = Names::default();
    names.claim(&deleted, "FIRST", reachable);
    fs::remove_dir_all(deleted.parent().unwrap()).unwrap();
    write_rom(&other);

    let (stored, how) = names.claim(&other, "SECOND", reachable);

    assert_eq!(how, How::OwnName);
    assert_ne!(stored, other);
    // The deleted game comes back with its own title: it takes its name over.
    let back = folder.join("jam3").join("game.gb");
    write_rom(&back);
    assert_eq!(
        names.claim(&back, "FIRST", reachable),
        (back.clone(), How::TookOver)
    );
}

#[test]
fn names_that_differ_only_in_case_or_extension_are_taken() {
    let folder = temporary_folder("case");
    let first = folder.join("A").join("Tetris.gb");
    let upper = folder.join("B").join("TETRIS.GB");
    let color = folder.join("C").join("tetris.gbc");
    let accented = folder.join("D").join("Été.gb");
    let accented_upper = folder.join("E").join("ÉTÉ.gb");
    for path in [&first, &upper, &color, &accented, &accented_upper] {
        write_rom(path);
    }
    let mut names = Names::default();
    names.claim(&first, "TETRIS", reachable);
    names.claim(&accented, "ETE", reachable);

    for path in [&upper, &color, &accented_upper] {
        let (stored, how) = names.claim(path, "OTHER", reachable);
        assert_eq!(how, How::OwnName, "{}", path.display());
        assert_ne!(&stored, path);
    }
    // Two hashed names from one folder that differ only in case don't share
    // a battery save either. File systems that ignore case (APFS, NTFS) keep
    // the two names as one file, which rightly keeps one name.
    let same_folder = folder.join("B").join("tetris.gb");
    write_rom(&same_folder);
    let stem = |path: &Path| {
        let stored = names.stored_path(path, false);
        stored.file_stem().unwrap().to_string_lossy().to_lowercase()
    };
    if fs::read_dir(folder.join("B")).unwrap().count() == 2 {
        assert_ne!(stem(&same_folder), stem(&upper));
    } else {
        assert_eq!(stem(&same_folder), stem(&upper));
    }
}

#[test]
fn a_game_whose_hashed_name_has_data_claims_it_after_the_record_is_lost() {
    let folder = temporary_folder("lost-record");
    let data = folder.join("data");
    let first = folder.join("A").join("game.gb");
    let second = folder.join("B").join("game.gb");
    write_rom(&first);
    write_rom(&second);
    let files = Files::in_folders(None, Some(data.clone()));
    claim(&data, &first, "FIRST", reachable);
    let own = claim(&data, &second, "SECOND", reachable).stored;
    files.write(Entry::BatteryRam(&first), b"first").unwrap();
    files.write(Entry::BatteryRam(&second), b"second").unwrap();

    fs::write(data.join("names.toml"), "names = 3").unwrap();
    forget_record(&data);
    // The library looks pictures up as it starts, before any game claims.
    let _ = picture_path(&data, &second, true);

    let second_claim = claim(&data, &second, "SECOND", reachable);
    assert_eq!(second_claim.stored, own);
    assert_eq!(second_claim.how, How::OwnName);
    assert!(
        second_claim
            .problem
            .is_some_and(|problem| problem.contains("names.kept-1.toml"))
    );
    assert_eq!(
        files.read(Entry::BatteryRam(&second)).unwrap().unwrap(),
        b"second"
    );

    let first_claim = claim(&data, &first, "FIRST", reachable);
    assert_eq!(
        first_claim,
        Claim {
            stored: first.clone(),
            how: How::Plain,
            problem: None
        }
    );
    assert_eq!(
        files.read(Entry::BatteryRam(&first)).unwrap().unwrap(),
        b"first"
    );
}

#[test]
fn a_read_problem_and_a_save_problem_are_both_reported() {
    let folder = temporary_folder("both-problems");
    let data = folder.join("data");
    let game = folder.join("A").join("game.gb");
    write_rom(&game);
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("names.toml"), "names = 3").unwrap();
    forget_record(&data);
    // A lookup reads the record and keeps it aside, but doesn't report it.
    assert_eq!(stored_path(&data, &game), game);
    // A folder where the record goes makes saving it fail.
    fs::create_dir_all(data.join("names.toml").join("in the way")).unwrap();

    let problem = claim(&data, &game, "GAME", reachable).problem.unwrap();

    assert!(problem.contains("names.kept-1.toml"), "{problem}");
    assert!(problem.contains("couldn't be saved"), "{problem}");
    // Both are reported once: the next claim of the same game changes nothing.
    assert_eq!(claim(&data, &game, "GAME", reachable).problem, None);
}

#[cfg(unix)]
#[test]
fn a_path_that_isn_t_utf_8_gets_the_name_a_lookup_gives() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let folder = temporary_folder("not-utf-8");
    let owner = folder.join("A").join("game.gb");
    let odd = folder.join(OsStr::from_bytes(b"B \xff")).join("game.gb");
    write_rom(&owner);
    let mut names = Names::default();
    names.claim(&owner, "GAME", reachable);

    let (stored, how) = names.claim(&odd, "OTHER", reachable);

    assert_eq!(how, How::Unrecorded);
    assert_ne!(stored, odd);
    assert_eq!(stored, names.stored_path(&odd, false));
    assert_eq!(names.owners.len(), 1);
}
