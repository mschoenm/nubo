//! Tests for reading ROM files and the names shown for them.

use super::*;

#[test]
fn no_intro_tags_are_dropped() {
    assert_eq!(game_name("Tetris (World) (Rev 1)"), "Tetris");
    assert_eq!(
        game_name("Pokemon - Red Version (USA, Europe) (SGB Enhanced)"),
        "Pokemon - Red Version"
    );
    assert_eq!(game_name("Mario [!] (Japan)"), "Mario");
}

#[test]
fn articles_moved_to_the_end_go_back_in_front() {
    assert_eq!(
        game_name("Legend of Zelda, The - Link's Awakening DX (USA, Europe) (Rev 2)"),
        "The Legend of Zelda - Link's Awakening DX"
    );
    assert_eq!(
        game_name("Bugs Bunny Crazy Castle, The"),
        "The Bugs Bunny Crazy Castle"
    );
    assert_eq!(game_name("Boy and His Blob, A"), "A Boy and His Blob");
}

#[test]
fn underscores_become_spaces() {
    assert_eq!(game_name("super_mario_land"), "super mario land");
    assert_eq!(game_name("m3_lcdc_bg_en_change"), "m3 lcdc bg en change");
}

#[test]
fn plain_names_stay_as_they_are() {
    assert_eq!(game_name("tobudx"), "tobudx");
    assert_eq!(game_name("cgb-acid2"), "cgb-acid2");
}

#[test]
fn a_name_that_would_be_empty_stays_as_it_was() {
    assert_eq!(game_name("(Demo)"), "(Demo)");
}

#[test]
fn a_file_too_big_to_be_a_game_isnt_read() {
    let folder = std::env::temp_dir().join(format!("nubo-rom-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("Disc image.gb");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(MAX_FILE_SIZE).unwrap();
    assert_eq!(Rom::read(&path).unwrap().bytes.len() as u64, MAX_FILE_SIZE);

    file.set_len(MAX_FILE_SIZE + 1).unwrap();
    let error = Rom::read(&path).unwrap_err();

    assert!(error.contains("too big"), "{error}");
    std::fs::remove_dir_all(&folder).unwrap();
}
