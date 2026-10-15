//! Tests for reading and writing this app's settings, with the shared ones,
//! as one TOML file.

use super::*;
use nubo_core::Model;
use nubo_frontend::model::ModelPreference;
use nubo_frontend::settings::FastForwardSpeed;

/// A `settings.toml` as Nubo 0.5.0 writes it, with every setting changed
/// from its default. It was written before key bindings stopped using Iced's
/// key codes, and before the shared settings moved to `nubo-frontend`.
/// Players' files like it must keep reading the same, and be written back
/// unchanged. A copy of the one in `nubo-frontend`'s tests, whose own tests
/// check the shared settings in it one by one.
const WRITTEN_BY_EARLIER_VERSION: &str = include_str!("tests/settings.toml");

/// The settings in [`WRITTEN_BY_EARLIER_VERSION`].
fn earlier_settings() -> Settings {
    Settings {
        appearance: Appearance::Dark,
        shared: toml::from_str(WRITTEN_BY_EARLIER_VERSION).unwrap(),
        integer_scaling: true,
        ..Settings::default()
    }
}

#[test]
fn settings_written_by_an_earlier_version_read_the_same() {
    let settings = Settings::from_toml(WRITTEN_BY_EARLIER_VERSION).unwrap();

    assert_eq!(settings, earlier_settings());
    assert_eq!(settings.shared.model, ModelPreference::Fixed(Model::CgbC));
    assert_eq!(settings.shared.fast_forward_speed.multiplier(), 3);
    assert_eq!(settings.shared.library_folders.len(), 2);
}

#[test]
fn settings_written_by_an_earlier_version_write_back_byte_for_byte() {
    let settings = Settings::from_toml(WRITTEN_BY_EARLIER_VERSION).unwrap();

    assert_eq!(
        toml::to_string(&settings).unwrap(),
        WRITTEN_BY_EARLIER_VERSION
    );
}

#[test]
fn settings_survive_a_round_trip() {
    let mut settings = earlier_settings();
    settings.appearance = Appearance::Light;
    settings.shared.volume = 0.5;

    let contents = toml::to_string(&settings).unwrap();

    assert_eq!(Settings::from_toml(&contents).unwrap(), settings);
}

#[test]
fn missing_fields_get_their_defaults() {
    let settings = Settings::from_toml("appearance = \"dark\"").unwrap();

    assert_eq!(settings.appearance, Appearance::Dark);
    assert!(!settings.integer_scaling);
    assert_eq!(
        settings.shared,
        nubo_frontend::settings::Settings::default()
    );
}

#[test]
fn the_defaults_are_the_shared_defaults() {
    let settings = Settings::default();

    assert_eq!(settings.appearance, Appearance::System);
    assert!(settings.shared.download_box_art);
    assert_eq!(
        settings.shared.fast_forward_speed,
        FastForwardSpeed::default()
    );
}

#[test]
fn shared_settings_are_still_checked() {
    assert!(Settings::from_toml("fast_forward_speed = 3").is_ok());
    assert!(Settings::from_toml("fast_forward_speed = 5").is_err());
    assert!(Settings::from_toml("model = \"N64\"").is_err());
    assert!(Settings::from_toml("volume = \"loud\"").is_err());
    assert!(Settings::from_toml("appearance = \"blue\"").is_err());
}

#[test]
fn a_mistake_in_the_shared_settings_is_reported_where_it_is() {
    let error = Settings::from_toml("appearance = \"dark\"\nfast_forward_speed = 5").unwrap_err();

    assert!(error.to_string().contains("line 2, column 22"), "{error}");
}
