//! Tests for the entries kept in memory. Reaching IndexedDB needs a browser;
//! its names are checked here, since they must stay the same so players'
//! data in the browser keeps loading.

use super::*;

#[test]
fn the_database_keeps_its_names() {
    assert_eq!(DATABASE, "nubo");
    assert_eq!(FILES, "files");
    assert_eq!(ROMS, "roms");
}

#[test]
fn an_entry_reads_back_as_written() {
    write("states/game.gb/quick.state", b"state").unwrap();

    let record = read("states/game.gb/quick.state").unwrap();

    assert_eq!(&record.bytes[..], b"state");
    assert!(record.modified <= SystemTime::now());
}

#[test]
fn entries_are_listed_and_removed_by_prefix() {
    for key in [
        "states/a.gb/quick.state",
        "states/a.gb/quick.png",
        "states/b.gb/quick.state",
        "thumbnails/a.gb.png",
    ] {
        write(key, b"x").unwrap();
    }

    assert_eq!(
        keys("states/a.gb/"),
        ["states/a.gb/quick.png", "states/a.gb/quick.state"]
    );
    assert_eq!(size("states/"), 3);

    remove_all("states/").unwrap();

    assert!(keys("states/").is_empty());
    assert!(read("thumbnails/a.gb.png").is_some());
}

#[test]
fn renaming_moves_the_entry() {
    write("old", b"contents").unwrap();

    assert!(rename("old", "new").unwrap());
    assert!(!rename("old", "newer").unwrap());

    assert!(read("old").is_none());
    assert_eq!(&read("new").unwrap().bytes[..], b"contents");
}

#[test]
fn roms_are_kept_apart_from_the_files() {
    write_rom("game.gb", b"rom").unwrap();

    let rom = futures::executor::block_on(read_rom("game.gb")).unwrap();

    assert_eq!(rom.as_deref(), Some(&b"rom"[..]));
    assert!(read("game.gb").is_none());

    remove_rom("game.gb").unwrap();
    assert!(
        futures::executor::block_on(read_rom("game.gb"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_refused_write_keeps_the_name_a_later_write_kept() {
    let mut names = RomNames::default();

    names.start("game.gb", true);
    names.start("game.gb", true);
    names.finish("game.gb", true, false);
    assert!(names.contains("game.gb"));
    names.finish("game.gb", true, true);

    assert!(names.contains("game.gb"));
    assert!(names.pending.is_empty());
}

#[test]
fn a_name_whose_writes_were_all_refused_is_dropped() {
    let mut names = RomNames::default();

    names.start("game.gb", true);
    names.start("game.gb", true);
    assert!(names.contains("game.gb"));
    names.finish("game.gb", true, false);
    names.finish("game.gb", true, false);

    assert!(!names.contains("game.gb"));
}

#[test]
fn a_refused_replacement_keeps_the_rom_the_database_has() {
    let mut names = RomNames::default();
    names.stored.insert(String::from("game.gb"));

    names.start("game.gb", true);
    names.finish("game.gb", true, false);

    assert!(names.contains("game.gb"));
}

#[test]
fn a_write_refused_after_a_removal_leaves_the_name_removed() {
    let mut names = RomNames::default();
    names.stored.insert(String::from("game.gb"));

    names.start("game.gb", false);
    names.start("game.gb", true);
    assert!(names.contains("game.gb"));
    names.finish("game.gb", false, true);
    names.finish("game.gb", true, false);

    assert!(!names.contains("game.gb"));
}

#[test]
fn a_refused_removal_keeps_the_rom_the_database_still_has() {
    let mut names = RomNames::default();
    names.stored.insert(String::from("game.gb"));

    names.start("game.gb", false);
    assert!(!names.contains("game.gb"));
    names.finish("game.gb", false, false);

    assert!(names.contains("game.gb"));
}
