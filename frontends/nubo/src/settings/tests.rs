//! Tests for reading and writing settings as TOML.

use super::*;
use nubo_core::Model;

#[test]
fn settings_survive_a_round_trip() {
    let settings = Settings {
        appearance: Appearance::Light,
        model: ModelPreference::Fixed(Model::CgbC),
        library_folders: vec![LibraryFolder {
            path: PathBuf::from("/games/gb"),
            include_subfolders: true,
        }],
        download_box_art: false,
        resume_games: true,
        fast_forward_speed: FastForwardSpeed(8),
        volume: 0.5,
        muted: true,
        controls: crate::controls::Controls::default(),
        dmg_palette: nubo_frontend::palettes::DmgPalette::Pocket,
        integer_scaling: true,
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
    let settings: Settings = toml::from_str("appearance = \"dark\"").unwrap();

    assert_eq!(settings.appearance, Appearance::Dark);
    assert_eq!(settings.model, ModelPreference::Automatic);
    assert!(settings.library_folders.is_empty());
    assert!(!settings.resume_games);
    assert_eq!(settings.fast_forward_speed.multiplier(), 4);
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

/// A `settings.toml` as Nubo 0.5.0 writes it, with every setting changed
/// from its default. It was written before key bindings stopped using Iced's
/// key codes. Players' files like it must keep reading the same, and be
/// written back unchanged.
const WRITTEN_BY_EARLIER_VERSION: &str = include_str!("tests/settings.toml");

/// The settings in [`WRITTEN_BY_EARLIER_VERSION`].
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
        appearance: Appearance::Dark,
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
        resume_games: true,
        fast_forward_speed: FastForwardSpeed(3),
        volume: 0.65,
        muted: true,
        controls,
        dmg_palette: DmgPalette::Pocket,
        integer_scaling: true,
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

    assert_eq!(
        toml::to_string(&settings).unwrap(),
        WRITTEN_BY_EARLIER_VERSION
    );
}
