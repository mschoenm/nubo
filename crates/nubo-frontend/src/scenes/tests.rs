use super::*;

const GAME_SCENE: &str = r#"
[game]
file = "tobudx.gb"
sha256 = "0a0e8018dbbc8d7f8cd99f05e7cdc7b4cc9e358ecfe9377ebfb2291a84c6e310"
model = "SGB2"
frames = 2

[window]
width = 1000.0
height = 780.0

[view]
pause_menu = "main"

[settings]
appearance = "dark"
dmg_palette = "pocket"
"#;

#[test]
fn a_scene_of_a_game_reads_and_writes_back_the_same() {
    let scene = Scene::from_toml(GAME_SCENE).unwrap();
    let game = scene.game.as_ref().unwrap();
    assert_eq!(game.model(), Ok(Model::Sgb2));
    assert_eq!(scene.view.screen, Screen::Game);
    assert_eq!(scene.view.pause_menu.as_deref(), Some("main"));
    assert_eq!(scene.settings["appearance"].as_str(), Some("dark"));

    assert_eq!(Scene::from_toml(&scene.to_toml()), Ok(scene));
}

#[test]
fn a_scene_of_the_library_needs_its_games_and_no_game() {
    let scene = Scene::from_toml(
        r#"
        view = { screen = "library" }
        [library]
        games = ["moki.gbc", "big2small.gbc"]
        played = [{ file = "moki.gbc", minutes_ago = 90 }]
        "#,
    )
    .unwrap();
    assert_eq!(scene.game, None);
    assert_eq!(scene.library.unwrap().played[0].minutes_ago, 90);

    assert!(Scene::from_toml(r#"view = { screen = "library" }"#).is_err());
    assert!(Scene::from_toml("[view]").is_err());
}

#[test]
fn unknown_keys_are_mistakes() {
    let misspelled = GAME_SCENE.replace("frames = 2", "frame = 2");
    assert!(Scene::from_toml(&misspelled).is_err());
}

#[test]
fn settings_change_by_key_with_values_read_as_toml_or_text() {
    let mut settings: toml::Table = toml::from_str("appearance = \"light\"\nvolume = 1.0").unwrap();
    let changes = [
        parse_change("appearance=dark").unwrap(),
        parse_change("volume = 0.5").unwrap(),
        parse_change("frame_blending=true").unwrap(),
        parse_change("controls.keyboard.a=\"KeyK\"").unwrap(),
    ];
    change_settings(&mut settings, &changes).unwrap();

    assert_eq!(settings["appearance"].as_str(), Some("dark"));
    assert_eq!(settings["volume"].as_float(), Some(0.5));
    assert_eq!(settings["frame_blending"].as_bool(), Some(true));
    assert_eq!(settings["controls"]["keyboard"]["a"].as_str(), Some("KeyK"));
}

#[test]
fn changes_that_name_no_setting_are_mistakes() {
    let mut settings: toml::Table = toml::from_str("volume = 1.0").unwrap();
    assert!(parse_change("volume").is_err());
    assert!(
        change_settings(
            &mut settings,
            &[(String::from("volume.loud"), String::from("1"))]
        )
        .is_err()
    );
    assert!(change_settings(&mut settings, &[(String::from("a."), String::from("1"))]).is_err());
}

#[cfg(feature = "desktop")]
#[test]
fn checksums_are_lowercase_hexadecimal_sha256() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn a_scene_folder_holds_the_scene_the_state_and_the_preview() {
    let folder = std::env::temp_dir().join(format!("nubo-scene-test-{}", std::process::id()));
    let scene = Scene::from_toml(GAME_SCENE).unwrap();
    scene.write(&folder, Some(b"state"), Some(b"png")).unwrap();

    assert_eq!(Scene::read(&folder), Ok(scene));
    assert_eq!(std::fs::read(folder.join(STATE_FILE)).unwrap(), b"state");
    assert_eq!(std::fs::read(folder.join(PREVIEW_FILE)).unwrap(), b"png");
    std::fs::remove_dir_all(folder).unwrap();
}
