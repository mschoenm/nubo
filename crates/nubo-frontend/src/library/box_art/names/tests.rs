//! Tests for finding the names and addresses of box art.

use super::*;

const LIST: &str = r#"clrmamepro (
	name "Nintendo - Game Boy"
)

game (
	name "Tetris (World) (Rev 1)"
	region "World"
	rom ( name "Tetris (World) (Rev 1).gb" size 65536 crc 46DF91AD md5 0 sha1 0 )
)
game (
	name "14 Juillet (World) (Fr)"
	rom ( name "14 Juillet (World) (Fr) (Aftermarket) (Unl).gb" size 1048576 crc 7B66BEE4 md5 0 sha1 0 )
)
"#;

fn games() -> GameList {
    let mut games = GameList::default();
    games.add(LIST);
    games
}

#[test]
fn a_file_named_like_no_intro_needs_no_checksum() {
    let names = games().names_for("Tetris (World) (Rev 1)", || panic!("no checksum needed"));

    assert_eq!(names, ["Tetris (World) (Rev 1)"]);
}

#[test]
fn a_file_name_can_differ_from_the_game_name() {
    let names = games().names_for("14 Juillet (World) (Fr) (Aftermarket) (Unl)", || None);

    assert_eq!(
        names,
        [
            "14 Juillet (World) (Fr) (Aftermarket) (Unl)",
            "14 Juillet (World) (Fr)"
        ]
    );
}

#[test]
fn a_renamed_file_is_found_by_its_checksum() {
    let names = games().names_for("tetris", || Some(0x46DF_91AD));

    assert_eq!(names, ["tetris", "Tetris (World) (Rev 1)"]);
}

#[test]
fn an_unknown_file_only_tries_its_own_name() {
    assert_eq!(games().names_for("homebrew", || Some(1)), ["homebrew"]);
}

#[test]
fn addresses_replace_and_encode_special_characters() {
    assert_eq!(
        box_art_url(
            GAME_BOY,
            "Legend of Zelda, The - Link's Awakening (USA, Europe)"
        ),
        "https://thumbnails.libretro.com/Nintendo%20-%20Game%20Boy/Named_Boxarts/\
         Legend%20of%20Zelda%2C%20The%20-%20Link%27s%20Awakening%20%28USA%2C%20Europe%29.png"
    );
    assert!(box_art_url(GAME_BOY, "Q*bert: The Game").ends_with("/Q_bert_%20The%20Game.png"));
}

#[test]
fn color_games_look_in_the_color_collection_first() {
    assert_eq!(systems(true), [GAME_BOY_COLOR, GAME_BOY]);
    assert_eq!(systems(false), [GAME_BOY, GAME_BOY_COLOR]);
}

#[test]
fn the_browser_looks_in_the_collections_copy_on_github() {
    assert_eq!(
        github_box_art_url(GAME_BOY_COLOR, "Q*bert: The Game (USA)"),
        "https://raw.githubusercontent.com/libretro-thumbnails/Nintendo_-_Game_Boy_Color/master/\
         Named_Boxarts/Q_bert_%20The%20Game%20%28USA%29.png"
    );
}

#[test]
fn game_lists_come_from_the_libretro_database() {
    assert_eq!(
        list_url(GAME_BOY),
        "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/\
         Nintendo%20-%20Game%20Boy.dat"
    );
}
