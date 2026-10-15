//! Tests for the Super Game Boy menu's choices.

use super::*;
use nubo_core::Model;

/// A menu whose player palette is `palette`.
fn menu(palette: Option<[u16; 4]>) -> SgbMenu {
    SgbMenu {
        player_palette: palette,
        palettes_allowed: true,
        border: SgbBorder::Game,
    }
}

#[test]
fn the_menu_shows_which_colors_are_picked() {
    let own = DEFAULT_OWN_PALETTE;
    assert_eq!(menu(None).colors(own), SgbColors::Game);
    for colors in SgbColors::ALL {
        assert_eq!(menu(colors.palette(own)).colors(own), colors);
    }
    assert_eq!(menu(Some([1, 2, 3, 4])).colors(own), SgbColors::Own);
}

#[test]
fn a_preset_the_player_copied_counts_as_their_own() {
    let own = DmgPalette::Green.rgb555();
    assert_eq!(menu(Some(own)).colors(own), SgbColors::Own);
}

#[test]
fn channels_set_only_their_own_bits() {
    let color = 0x7FFF;
    let without_green = ColorChannel::Green.with(color, 0);
    assert_eq!(without_green, 0x7C1F);
    assert_eq!(ColorChannel::Green.of(without_green), 0);
    assert_eq!(ColorChannel::Red.of(without_green), 31);
    assert_eq!(ColorChannel::Blue.with(0, 40), 0x7C00);
}

#[test]
fn colors_convert_both_ways() {
    assert_eq!(rgb555([0xFF, 0x00, 0x00]), 0x001F);
    assert_eq!(rgb(0x7C00), [0x00, 0x00, 0xFF]);
    for color in DEFAULT_OWN_PALETTE {
        assert_eq!(rgb555(rgb(color)), color);
    }
}

#[test]
fn nubos_border_shows_its_day_look_from_seven_to_seven() {
    assert_eq!(border_time_at(0), SgbBorderTime::Night);
    assert_eq!(border_time_at(6), SgbBorderTime::Night);
    assert_eq!(border_time_at(7), SgbBorderTime::Day);
    assert_eq!(border_time_at(12), SgbBorderTime::Day);
    assert_eq!(border_time_at(18), SgbBorderTime::Day);
    assert_eq!(border_time_at(19), SgbBorderTime::Night);
    assert_eq!(border_time_at(23), SgbBorderTime::Night);
}

#[test]
fn the_border_clock_follows_the_clock_on_a_super_game_boy_only() {
    let mut emulator = Emulator::new(Model::Sgb);
    let before = border_time_now();
    BorderClock::start(&mut emulator);
    let after = border_time_now();
    // The hour may turn between the looks at the clock.
    assert!([before, after].contains(&emulator.sgb_border_time()));

    let mut emulator = Emulator::new(Model::Dmg);
    let mut clock = BorderClock::start(&mut emulator);
    for _ in 0..FRAMES_BETWEEN_CHECKS {
        clock.frame_finished(&mut emulator);
    }
    assert_eq!(emulator.sgb_border_time(), SgbBorderTime::Night);
}
