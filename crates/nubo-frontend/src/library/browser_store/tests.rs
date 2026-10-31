//! Tests for the library in the browser's storage, which keeps its entries
//! in memory outside a browser (see `storage::indexed_db`), one set per test
//! thread.

use super::*;
use crate::storage::indexed_db;

#[test]
fn entries_are_named_like_the_desktops_files() {
    let rom = Path::new("Pocket Game.gbc");

    assert_eq!(rom_key(rom), "Pocket Game.gbc");
    assert_eq!(
        picture_key(PictureKind::LastScreen, rom),
        "thumbnails/Pocket Game.gbc.png"
    );
    assert_eq!(
        picture_key(PictureKind::TitleScreen, rom),
        "title_screens/Pocket Game.gbc.png"
    );
    assert_eq!(
        picture_key(PictureKind::BoxArt, rom),
        "box_art/Pocket Game.gbc.png"
    );
}

#[test]
fn the_games_read_back_as_saved() {
    let cache = Cache::default();
    let mut game = LibraryEntry::example("Tetris.gb");
    game.last_played = Some(1_000);

    save_entries(&cache, std::slice::from_ref(&game)).unwrap();

    assert_eq!(load_entries(&cache).unwrap(), vec![game]);
    assert!(indexed_db::read("library.toml").is_some());
}

#[test]
fn an_unreadable_library_is_kept_aside_before_a_save_can_replace_it() {
    let cache = Cache::default();
    let from_newer_nubo = b"[[games]]\nmodel = \"Game Boy Advance SP\"\n";
    for _ in 0..2 {
        indexed_db::write("library.toml", from_newer_nubo).unwrap();
        assert!(load_entries(&cache).is_err());
        save_entries(&cache, &[]).unwrap();
    }

    for kept in ["library.kept-1.toml", "library.kept-2.toml"] {
        assert_eq!(&*indexed_db::read(kept).unwrap().bytes, from_newer_nubo);
    }
    assert_eq!(load_entries(&cache), Ok(Vec::new()));
}

#[test]
fn a_new_library_is_empty() {
    assert!(load_entries(&Cache::default()).unwrap().is_empty());
}

#[test]
fn pictures_read_back_and_are_deleted_together() {
    let cache = Cache::default();
    let rom = Path::new("Tetris.gb");
    let pixels = vec![90; (SCREEN_WIDTH * SCREEN_HEIGHT * 4) as usize];

    save_thumbnail(&cache, rom, &pixels).unwrap();
    save_title_screen(&cache, rom, &pixels).unwrap();

    let (width, height, read) = read_picture(&cache, PictureKind::LastScreen, rom).unwrap();
    assert_eq!((width, height), (SCREEN_WIDTH, SCREEN_HEIGHT));
    assert_eq!(read, pixels);
    assert!(current_picture(&cache, PictureKind::TitleScreen, rom, rom).is_some());
    assert!(size(&[]) > 0);

    delete_pictures(rom).unwrap();

    assert!(read_picture(&cache, PictureKind::LastScreen, rom).is_none());
    assert!(read_picture(&cache, PictureKind::TitleScreen, rom).is_none());
}

#[test]
fn box_art_is_kept_as_downloaded() {
    let cache = Cache::default();
    let rom = Path::new("Tetris.gb");

    save_box_art(&cache, rom, b"png").unwrap();

    assert_eq!(
        current_picture(&cache, PictureKind::BoxArt, rom, rom).unwrap(),
        b"png"
    );
}

#[test]
fn a_games_file_is_kept_under_its_file_name() {
    let rom = Path::new("Tetris.gb");

    save_rom(rom, b"rom").unwrap();
    assert!(has_rom(rom));
    assert!(
        futures::executor::block_on(indexed_db::read_rom("Tetris.gb"))
            .unwrap()
            .is_some()
    );

    delete_rom(rom).unwrap();
    assert!(!has_rom(rom));
    assert!(
        futures::executor::block_on(indexed_db::read_rom("Tetris.gb"))
            .unwrap()
            .is_none()
    );
}
