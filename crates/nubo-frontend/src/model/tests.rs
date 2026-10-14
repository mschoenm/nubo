//! Tests for picking a game's model and naming the preference.

use super::*;

/// A header for a game that does or doesn't use Game Boy Color features.
fn header(supports_cgb: bool) -> CartridgeHeader {
    sgb_header(supports_cgb, false)
}

/// A header for a game that does or doesn't use Game Boy Color and Super
/// Game Boy features.
fn sgb_header(supports_cgb: bool, supports_sgb: bool) -> CartridgeHeader {
    CartridgeHeader {
        supports_sgb,
        cartridge_type: 0,
        rom_bank_count: 2,
        ram_bank_count: 0,
        title: String::from("TEST"),
        has_battery: false,
        supports_cgb,
    }
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

#[test]
fn automatic_starts_super_game_boy_games_on_the_game_boy_they_were_made_for() {
    assert_eq!(
        ModelPreference::Automatic.model_for(&sgb_header(false, true)),
        Model::Dmg
    );
    assert_eq!(
        ModelPreference::Automatic.model_for(&sgb_header(true, true)),
        Model::CgbE
    );
}

#[test]
fn preferences_are_named_auto_or_after_the_model() {
    assert_eq!(String::from(ModelPreference::Automatic), "auto");
    assert_eq!(String::from(ModelPreference::Fixed(Model::CgbE)), "CGB-E");
    assert_eq!(
        ModelPreference::try_from(String::from("AUTO")),
        Ok(ModelPreference::Automatic)
    );
    assert_eq!(
        ModelPreference::try_from(String::from("CGB-E")),
        Ok(ModelPreference::Fixed(Model::CgbE))
    );
    assert!(ModelPreference::try_from(String::from("N64")).is_err());
}
