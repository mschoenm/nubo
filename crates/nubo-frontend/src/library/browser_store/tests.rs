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
    assert!(current_picture(&cache, PictureKind::TitleScreen, rom).is_some());
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
        current_picture(&cache, PictureKind::BoxArt, rom).unwrap(),
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
