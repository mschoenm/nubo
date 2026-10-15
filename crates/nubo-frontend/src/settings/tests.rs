//! Tests for reading and writing settings as TOML.

use super::*;
use nubo_core::Model;

#[test]
fn settings_survive_a_round_trip() {
    let settings = Settings {
        model: ModelPreference::Fixed(Model::CgbC),
        library_folders: vec![LibraryFolder {
            path: PathBuf::from("/games/gb"),
            include_subfolders: true,
        }],
        download_box_art: false,
        box_art_asked: true,
        check_for_updates: Some(true),
        resume_games: true,
        fast_forward_speed: FastForwardSpeed(8),
        volume: 0.5,
        muted: true,
        controls: Controls::default(),
        controls_revision: 1,
        dmg_palette: DmgPalette::Pocket,
        sgb_border: false,
        sgb_own_palette: [0x001F, 0x03E0, 0x7C00, 0x0000],
        sgb_colors: SgbColors::Preset(DmgPalette::Green),
        sgb_nubo_border: true,
        color_correction: false,
        frame_blending: true,
    };

    let contents = toml::to_string(&settings).unwrap();

    assert_eq!(toml::from_str::<Settings>(&contents).unwrap(), settings);
}

#[test]
fn models_are_saved_by_name() {
    let settings = Settings {
        model: ModelPreference::Fixed(Model::Mgb),
        ..Settings::default()
    };

    assert!(
        toml::to_string(&settings)
            .unwrap()
            .contains("model = \"MGB\"")
    );
}

#[test]
fn missing_fields_get_their_defaults() {
    let settings: Settings = toml::from_str("muted = true").unwrap();

    assert!(settings.muted);
    assert_eq!(settings.model, ModelPreference::Automatic);
    assert!(settings.library_folders.is_empty());
    assert!(settings.resume_games);
    assert_eq!(settings.fast_forward_speed.multiplier(), 4);
}

#[test]
fn the_super_game_boy_palette_is_only_saved_once_the_player_changes_it() {
    assert!(
        !toml::to_string(&Settings::default())
            .unwrap()
            .contains("sgb_own_palette")
    );
    let changed = Settings {
        sgb_own_palette: [1, 2, 3, 4],
        ..Settings::default()
    };
    let contents = toml::to_string(&changed).unwrap();
    assert!(contents.contains("sgb_own_palette = [1, 2, 3, 4]"));
    assert_eq!(
        toml::from_str::<Settings>(&contents)
            .unwrap()
            .sgb_own_palette,
        [1, 2, 3, 4]
    );
}

#[test]
fn the_super_game_boy_choices_are_only_saved_once_the_player_makes_them() {
    let defaults = toml::to_string(&Settings::default()).unwrap();
    assert!(!defaults.contains("sgb_colors"));
    assert!(!defaults.contains("sgb_nubo_border"));
    let picked = Settings {
        sgb_colors: SgbColors::Own,
        sgb_nubo_border: true,
        ..Settings::default()
    };
    let contents = toml::to_string(&picked).unwrap();
    assert!(contents.contains("sgb_colors = \"own\""));
    assert!(contents.contains("sgb_nubo_border = true"));
    assert_eq!(toml::from_str::<Settings>(&contents).unwrap(), picked);
}

#[test]
fn the_super_game_boy_border_is_only_saved_when_it_is_off() {
    assert!(
        !toml::to_string(&Settings::default())
            .unwrap()
            .contains("sgb_border")
    );
    let hidden = Settings {
        sgb_border: false,
        ..Settings::default()
    };
    let contents = toml::to_string(&hidden).unwrap();
    assert!(contents.contains("sgb_border = false"));
    assert!(!toml::from_str::<Settings>(&contents).unwrap().sgb_border);
}

#[test]
fn only_offered_fast_forward_speeds_are_accepted() {
    assert!(toml::from_str::<Settings>("fast_forward_speed = 3").is_ok());
    assert!(toml::from_str::<Settings>("fast_forward_speed = 5").is_err());
}

#[test]
fn unknown_models_are_rejected() {
    assert!(toml::from_str::<Settings>("model = \"N64\"").is_err());
}

#[test]
fn without_saved_settings_the_defaults_load_quietly() {
    let (settings, problem) = load(&crate::storage::NoStorage, |contents| {
        toml::from_str::<Settings>(contents)
    });

    assert_eq!(settings, Settings::default());
    assert_eq!(problem, None);
}

/// A `settings.toml` as Nubo 0.5.0 writes it, with every setting changed
/// from its default. It was written before key bindings stopped using Iced's
/// key codes. Players' files like it must keep reading the same, and be
/// written back unchanged. The Iced app's tests keep a copy, which they read
/// with the app's own settings too.
const WRITTEN_BY_EARLIER_VERSION: &str = include_str!("tests/settings.toml");

/// The settings that only the Iced app reads, which [`Settings`] skips.
const ICED_APP_SETTINGS: [&str; 2] = ["appearance = \"dark\"\n", "integer_scaling = true\n"];

/// The shared settings in [`WRITTEN_BY_EARLIER_VERSION`].
fn earlier_settings() -> Settings {
    use crate::controls::{Action, KeyCode, PadButton};
    use nubo_core::ControllerButton;

    let mut controls = Controls::default();
    let keyboard = &mut controls.keyboard;
    keyboard.bind(Action::Up, Some(KeyCode::KeyW));
    keyboard.bind(Action::Down, Some(KeyCode::KeyS));
    keyboard.bind(Action::Left, Some(KeyCode::KeyA));
    keyboard.bind(Action::Right, Some(KeyCode::KeyD));
    keyboard.bind(Action::A, Some(KeyCode::KeyK));
    keyboard.bind(Action::B, Some(KeyCode::KeyJ));
    keyboard.bind(Action::Start, Some(KeyCode::NumpadEnter));
    keyboard.bind(Action::Select, Some(KeyCode::ShiftRight));
    keyboard.bind(Action::FastForward, Some(KeyCode::Backquote));
    keyboard.bind(Action::QuickSave, Some(KeyCode::F1));
    keyboard.bind(Action::QuickLoad, Some(KeyCode::Quote));
    keyboard.bind(Action::Fullscreen, Some(KeyCode::Backslash));
    keyboard.bind(Action::Screenshot, None);
    // The file was written before rewind had a key.
    keyboard.bind(Action::Rewind, None);
    let controller = &mut controls.controller;
    controller.bind(Action::A, Some(PadButton(ControllerButton::South)));
    controller.bind(Action::B, Some(PadButton(ControllerButton::West)));
    controller.bind(
        Action::PauseMenu,
        Some(PadButton(ControllerButton::RightStick)),
    );
    controller.bind(
        Action::FastForward,
        Some(PadButton(ControllerButton::LeftShoulder)),
    );
    controller.bind(Action::QuickSave, None);
    controller.bind(Action::Fullscreen, Some(PadButton(ControllerButton::North)));
    controller.bind(
        Action::Screenshot,
        Some(PadButton(ControllerButton::LeftStick)),
    );

    Settings {
        model: ModelPreference::Fixed(Model::CgbC),
        library_folders: vec![
            LibraryFolder {
                path: PathBuf::from("/home/player/Games/Game Boy"),
                include_subfolders: true,
            },
            LibraryFolder {
                path: PathBuf::from("/media/player/USB stick/roms"),
                include_subfolders: false,
            },
        ],
        download_box_art: false,
        box_art_asked: false,
        check_for_updates: None,
        resume_games: true,
        fast_forward_speed: FastForwardSpeed(3),
        volume: 0.65,
        muted: true,
        controls,
        controls_revision: 0,
        dmg_palette: DmgPalette::Pocket,
        sgb_border: true,
        sgb_own_palette: DEFAULT_OWN_PALETTE,
        sgb_colors: SgbColors::Game,
        sgb_nubo_border: false,
        color_correction: true,
        frame_blending: false,
    }
}

#[test]
fn settings_written_by_an_earlier_version_read_the_same() {
    let settings: Settings = toml::from_str(WRITTEN_BY_EARLIER_VERSION).unwrap();

    assert_eq!(settings, earlier_settings());
}

#[test]
fn settings_written_by_an_earlier_version_write_back_byte_for_byte() {
    let settings: Settings = toml::from_str(WRITTEN_BY_EARLIER_VERSION).unwrap();
    let without_iced_app_settings = ICED_APP_SETTINGS.iter().fold(
        String::from(WRITTEN_BY_EARLIER_VERSION),
        |contents, line| contents.replacen(line, "", 1),
    );

    assert_eq!(
        toml::to_string(&settings).unwrap(),
        without_iced_app_settings
    );
}

#[test]
fn the_raw_screen_is_the_look_with_every_effect_off() {
    let plain = Settings {
        dmg_palette: DmgPalette::Gray,
        color_correction: false,
        frame_blending: false,
        ..Settings::default()
    };
    assert_eq!(plain.screen_look(), ScreenLook::RAW);
}

#[test]
fn a_reset_keeps_only_the_folders_unless_asked_and_asks_the_questions_again() {
    let folders = vec![LibraryFolder {
        path: PathBuf::from("/games/gb"),
        include_subfolders: false,
    }];
    let settings = Settings {
        model: ModelPreference::Fixed(Model::CgbC),
        library_folders: folders.clone(),
        download_box_art: false,
        box_art_asked: true,
        check_for_updates: Some(true),
        volume: 0.5,
        controls_revision: 1,
        dmg_palette: DmgPalette::Pocket,
        ..Settings::default()
    };

    let reset = settings.reset(false);

    assert_eq!(
        reset,
        Settings {
            library_folders: folders,
            controls_revision: 1,
            ..Settings::default()
        }
    );
    assert!(!reset.box_art_asked);
    assert_eq!(reset.check_for_updates, None);
    assert!(settings.reset(true).library_folders.is_empty());
}
