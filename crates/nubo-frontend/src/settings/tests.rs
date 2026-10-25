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
        original_screen: false,
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
fn a_volume_outside_0_to_1_or_not_a_number_reads_within_range() {
    for (written, read) in [
        ("0.5", 0.5),
        ("2.0", 1.0),
        ("-1.0", 0.0),
        ("nan", 1.0),
        ("inf", 1.0),
        ("-inf", 1.0),
    ] {
        let settings: Settings = toml::from_str(&format!("volume = {written}")).unwrap();
        assert_eq!(settings.volume, read, "volume = {written}");
    }
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
    let loaded = load(&crate::storage::NoStorage, |contents| {
        toml::from_str::<Settings>(contents)
    });

    assert_eq!(loaded.settings, Settings::default());
    assert_eq!(loaded.problem, None);
    assert!(loaded.saving_is_safe);
}

#[cfg(feature = "desktop")]
#[test]
fn unreadable_settings_are_kept_aside_before_the_defaults_can_replace_them() {
    use crate::storage::files::Files;
    use std::fs;

    let folder = std::env::temp_dir().join(format!("nubo-settings-kept-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    let files = Files::in_folders(Some(folder.clone()), None);
    let from_newer_nubo = "model = \"Game Boy Advance SP\"\nmuted = true\n";
    for _ in 0..2 {
        files
            .write(Entry::Settings, from_newer_nubo.as_bytes())
            .unwrap();
        let loaded = load(&files, |contents| toml::from_str::<Settings>(contents));
        assert_eq!(loaded.settings, Settings::default());
        assert!(loaded.saving_is_safe);
        assert!(loaded.problem.unwrap().contains("kept them as"));
        save(&files, &loaded.settings).unwrap();
    }

    for kept in ["settings.kept-1.toml", "settings.kept-2.toml"] {
        assert_eq!(
            fs::read_to_string(folder.join(kept)).unwrap(),
            from_newer_nubo
        );
    }
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn settings_that_cant_be_kept_aside_are_not_safe_to_save_over() {
    struct Unreadable;

    impl Storage for Unreadable {
        fn read(&self, _entry: Entry<'_>) -> Result<Option<Vec<u8>>, String> {
            Ok(Some(b"muted = 3".to_vec()))
        }

        fn write(&self, _entry: Entry<'_>, _contents: &[u8]) -> Result<(), String> {
            Ok(())
        }

        fn location(&self, entry: Entry<'_>) -> String {
            entry.file_name().display().to_string()
        }

        fn delete_battery_saves(&self) -> Result<(), String> {
            Ok(())
        }

        fn keep_aside(&self, _entry: Entry<'_>) -> Result<String, String> {
            Err(String::from("the disk is read-only"))
        }
    }

    let loaded = load(&Unreadable, |contents| toml::from_str::<Settings>(contents));

    assert_eq!(loaded.settings, Settings::default());
    assert!(!loaded.saving_is_safe);
    assert!(loaded.problem.unwrap().contains("the disk is read-only"));
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
    // The file was written while quick load was on the left trigger by
    // default.
    controller.bind(
        Action::QuickLoad,
        Some(PadButton(ControllerButton::LeftTrigger)),
    );
    controller.bind(Action::Fullscreen, Some(PadButton(ControllerButton::North)));
    controller.bind(
        Action::Screenshot,
        Some(PadButton(ControllerButton::LeftStick)),
    );
    controller.bind(Action::Rewind, None);

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
        original_screen: true,
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
        original_screen: false,
        ..Settings::default()
    };
    assert_eq!(plain.screen_look(), ScreenLook::RAW);
}

#[test]
fn a_reset_keeps_only_the_folders_unless_asked_and_the_controls_and_asks_the_questions_again() {
    let folders = vec![LibraryFolder {
        path: PathBuf::from("/games/gb"),
        include_subfolders: false,
    }];
    let controls = Controls {
        menu_buttons: crate::controls::MenuButtons::Xbox,
        ..Controls::default()
    };
    let settings = Settings {
        model: ModelPreference::Fixed(Model::CgbC),
        library_folders: folders.clone(),
        download_box_art: false,
        box_art_asked: true,
        check_for_updates: Some(true),
        volume: 0.5,
        controls: controls.clone(),
        controls_revision: 1,
        dmg_palette: DmgPalette::Pocket,
        ..Settings::default()
    };

    let reset = settings.reset();

    assert_eq!(
        reset,
        Settings {
            library_folders: folders,
            controls,
            controls_revision: 1,
            ..Settings::default()
        }
    );
    assert!(!reset.box_art_asked);
    assert_eq!(reset.check_for_updates, None);
}
