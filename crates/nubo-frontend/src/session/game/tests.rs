//! Tests for telling the emulator how to play only when that changes, and
//! for showing frames in the chosen palette.

use super::*;
use crate::settings::FastForwardSpeed;

/// A whole frame in the lightest of `palette`'s colors.
fn frame_in(palette: DmgPalette) -> Vec<u8> {
    let [red, green, blue] = palette.colors()[0];
    [red, green, blue, 0xFF].repeat((SCREEN_WIDTH * SCREEN_HEIGHT) as usize)
}

#[test]
fn an_original_game_boy_frame_is_shown_in_the_palette() {
    let mut screen = Screen::new(true);
    screen.set_palette(DmgPalette::Green);

    screen.show(frame_in(DmgPalette::Gray));

    assert_eq!(screen.pixels(), Some(&frame_in(DmgPalette::Green)[..]));
}

#[test]
fn changing_the_palette_repaints_the_frame_on_screen() {
    let mut screen = Screen::new(true);
    screen.show(frame_in(DmgPalette::Gray));

    screen.set_palette(DmgPalette::Green);
    assert_eq!(screen.pixels(), Some(&frame_in(DmgPalette::Green)[..]));
    screen.set_palette(DmgPalette::Pocket);
    assert_eq!(screen.pixels(), Some(&frame_in(DmgPalette::Pocket)[..]));
    screen.set_palette(DmgPalette::Gray);
    assert_eq!(screen.pixels(), Some(&frame_in(DmgPalette::Gray)[..]));
}

#[test]
fn a_color_game_keeps_its_colors_and_no_second_copy() {
    let mut screen = Screen::new(false);
    screen.set_palette(DmgPalette::Green);

    screen.show(frame_in(DmgPalette::Gray));

    assert_eq!(screen.pixels(), Some(&frame_in(DmgPalette::Gray)[..]));
    assert!(screen.raw.is_none());
}

#[test]
fn nothing_is_sent_while_nothing_changes() {
    let settings = Settings::default();
    let mut control = PlaybackControl::new(true, None, &settings);

    assert_eq!(control.change(true, &settings), None);
}

#[test]
fn pausing_is_sent_once() {
    let settings = Settings::default();
    let mut control = PlaybackControl::new(true, None, &settings);

    let paused = control.change(false, &settings).unwrap();

    assert!(!paused.running);
    assert_eq!(control.change(false, &settings), None);
}

#[test]
fn fast_forward_runs_at_the_speed_the_settings_pick() {
    let settings = Settings {
        fast_forward_speed: FastForwardSpeed::ALL[0],
        ..Settings::default()
    };
    let mut control = PlaybackControl::new(true, None, &settings);

    control.fast_forward_held = true;
    let fast = control.change(true, &settings).unwrap();
    control.fast_forward_held = false;
    let normal = control.change(true, &settings).unwrap();

    assert_eq!(fast.fast_forward, Some(FastForwardSpeed::ALL[0]));
    assert_eq!(normal.fast_forward, None);
}

#[test]
fn a_game_started_fast_forwarding_keeps_its_own_speed() {
    let settings = Settings {
        fast_forward_speed: FastForwardSpeed::ALL[0],
        ..Settings::default()
    };
    let mut control = PlaybackControl::new(true, Some(FastForwardSpeed::ALL[3]), &settings);

    // The emulator starts at that speed, so there's nothing more to send.
    assert_eq!(control.sent.fast_forward, Some(FastForwardSpeed::ALL[3]));
    assert_eq!(control.change(true, &settings), None);

    control.fast_forward_on = false;
    assert_eq!(control.change(true, &settings).unwrap().fast_forward, None);
    control.fast_forward_held = true;
    let held = control.change(true, &settings).unwrap();
    assert_eq!(held.fast_forward, Some(FastForwardSpeed::ALL[3]));
}

#[test]
fn a_new_game_starts_muted_when_the_player_muted_the_sound() {
    let settings = Settings {
        muted: true,
        ..Settings::default()
    };

    let control = PlaybackControl::new(false, None, &settings);

    assert_eq!(control.sent.volume, 0.0);
    assert!(!control.sent.running);
}
