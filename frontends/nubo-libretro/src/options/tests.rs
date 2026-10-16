use super::*;

#[test]
fn the_defaults_are_what_options_start_as() {
    let mut options = Options::default();
    for definition in definitions() {
        assert!(options.set(definition.key, definition.default_value()));
    }
    assert_eq!(options, Options::default());
    assert_eq!(options.model, ModelPreference::Automatic);
    assert_eq!(options.palette, DmgPalette::Gray);
    assert!(options.sgb_border);
    assert_eq!(options.rtc_clock, RtcClock::WallClock);
}

#[test]
fn every_value_the_menu_offers_can_be_set() {
    for definition in definitions() {
        for (value, label) in &definition.values {
            let mut options = Options::default();
            assert!(
                options.set(definition.key, value),
                "{} = {value} ({label})",
                definition.key
            );
        }
    }
}

#[test]
fn every_model_can_be_picked() {
    for model in Model::ALL {
        let mut options = Options::default();
        assert!(options.set(MODEL, model.name()));
        assert_eq!(options.model, ModelPreference::Fixed(model));
    }
}

#[test]
fn the_values_set_the_settings_they_name() {
    let mut options = Options::default();
    assert!(options.set(PALETTE, "pocket"));
    assert!(options.set(RTC, "emulated"));
    assert!(options.set(MODEL, "CGB-C"));
    assert!(options.set(SGB_BORDER, "disabled"));
    assert!(options.set(SGB_COLORS, "green"));
    assert_eq!(
        options,
        Options {
            model: ModelPreference::Fixed(Model::CgbC),
            palette: DmgPalette::Pocket,
            sgb_colors: SgbColors::Preset(DmgPalette::Green),
            sgb_border: false,
            nubo_border: false,
            rtc_clock: RtcClock::EmulatedTime,
        }
    );
}

#[test]
fn the_border_option_picks_the_games_nubos_or_none() {
    let mut options = Options::default();
    assert!(options.set(SGB_BORDER, "nubo"));
    assert!(options.sgb_border && options.nubo_border);
    assert!(options.set(SGB_BORDER, "enabled"));
    assert!(options.sgb_border && !options.nubo_border);
    assert!(options.set(SGB_COLORS, "game"));
    assert_eq!(options.sgb_colors, SgbColors::Game);
}

#[test]
fn unknown_keys_and_values_change_nothing() {
    let mut options = Options::default();
    assert!(!options.set("nubo_unknown", "auto"));
    assert!(!options.set(MODEL, "GBA SP"));
    assert!(!options.set(PALETTE, "purple"));
    assert!(!options.set(RTC, "sometimes"));
    assert!(!options.set(SGB_BORDER, "maybe"));
    assert_eq!(options, Options::default());
}

#[test]
fn the_menu_has_few_enough_values_and_unique_keys() {
    let definitions = definitions();
    for definition in &definitions {
        // RETRO_NUM_CORE_OPTION_VALUES_MAX, less the terminating entry.
        assert!(definition.values.len() < 128);
        assert!(definition.key.starts_with("nubo_"));
    }
    let mut keys: Vec<_> = definitions
        .iter()
        .map(|definition| definition.key)
        .collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), definitions.len());
}
