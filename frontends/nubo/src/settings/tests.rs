//! Tests for reading and writing settings as TOML.

use super::*;

/// A header for a game that does or doesn't use Game Boy Color features.
fn header(supports_cgb: bool) -> CartridgeHeader {
    CartridgeHeader {
        supports_sgb: false,
        cartridge_type: 0,
        rom_bank_count: 2,
        ram_bank_count: 0,
        title: String::from("TEST"),
        has_battery: false,
        supports_cgb,
    }
}

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
        dmg_palette: crate::palettes::DmgPalette::Pocket,
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

#[test]
fn automatic_picks_the_model_from_the_header() {
    assert_eq!(
        ModelPreference::Automatic.model_for(&header(true)),
        Model::CgbE
    );
    assert_eq!(
        ModelPreference::Automatic.model_for(&header(false)),
        Model::Dmg
    );
    assert_eq!(
        ModelPreference::Fixed(Model::Mgb).model_for(&header(true)),
        Model::Mgb
    );
}
