//! Tests for telling the emulator how to play only when that changes, and
//! for showing frames in the chosen palette.

use super::*;
use crate::picture::{SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::settings::FastForwardSpeed;

/// A whole Game Boy screen in the lightest of `palette`'s colors.
fn pixels_in(palette: DmgPalette) -> Vec<u8> {
    let [red, green, blue] = palette.colors()[0];
    [red, green, blue, 0xFF].repeat((SCREEN_WIDTH * SCREEN_HEIGHT) as usize)
}

/// A frame of the Game Boy screen in the lightest gray, in the emulator's
/// grays or not.
fn screen_frame(in_dmg_shades: bool) -> Frame {
    Frame {
        pixels: pixels_in(DmgPalette::Gray),
        width: SCREEN_WIDTH,
        height: SCREEN_HEIGHT,
        screen_position: (0, 0),
        in_dmg_shades,
    }
}

/// A Super Game Boy frame: a black border around a screen in the lightest
/// gray.
fn bordered_frame() -> Frame {
    let (width, height) = (256, 224);
    let mut pixels = [0, 0, 0, 0xFF].repeat((width * height) as usize);
    for row in 40..40 + SCREEN_HEIGHT as usize {
        let start = (row * width as usize + 48) * 4;
        pixels[start..start + SCREEN_WIDTH as usize * 4].fill(0xFF);
    }
    Frame {
        pixels,
        width,
        height,
        screen_position: (48, 40),
        in_dmg_shades: true,
    }
}

impl Screen {
    fn pixels(&self) -> Option<&[u8]> {
        self.picture.as_ref().map(Picture::pixels)
    }
}

#[test]
fn an_original_game_boy_frame_is_shown_in_the_palette() {
    let mut screen = Screen::new();
    screen.set_palette(DmgPalette::Green);

    screen.show(screen_frame(true));

    assert_eq!(screen.pixels(), Some(&pixels_in(DmgPalette::Green)[..]));
}

#[test]
fn changing_the_palette_repaints_the_frame_on_screen() {
    let mut screen = Screen::new();
    screen.show(screen_frame(true));

    screen.set_palette(DmgPalette::Green);
    assert_eq!(screen.pixels(), Some(&pixels_in(DmgPalette::Green)[..]));
    screen.set_palette(DmgPalette::Pocket);
    assert_eq!(screen.pixels(), Some(&pixels_in(DmgPalette::Pocket)[..]));
    screen.set_palette(DmgPalette::Gray);
    assert_eq!(screen.pixels(), Some(&pixels_in(DmgPalette::Gray)[..]));
}

#[test]
fn a_color_game_keeps_its_colors() {
    let mut screen = Screen::new();
    screen.set_palette(DmgPalette::Green);

    screen.show(screen_frame(false));

    assert_eq!(screen.pixels(), Some(&pixels_in(DmgPalette::Gray)[..]));
}

#[test]
fn a_super_game_boy_border_shows_unless_it_is_switched_off() {
    let mut screen = Screen::new();
    screen.show(bordered_frame());
    let picture = screen.picture.clone().unwrap();
    assert_eq!((picture.width(), picture.height()), (256, 224));

    screen.set_border(false);
    let picture = screen.picture.clone().unwrap();
    assert_eq!(
        (picture.width(), picture.height()),
        (SCREEN_WIDTH, SCREEN_HEIGHT)
    );
    assert_eq!(picture.pixels(), &pixels_in(DmgPalette::Gray)[..]);
}

#[test]
fn the_palette_repaints_only_the_screen_inside_a_border() {
    let mut screen = Screen::new();
    screen.set_palette(DmgPalette::Green);
    screen.show(bordered_frame());

    let pixels = screen.pixels().unwrap();
    assert_eq!(&pixels[..4], &[0, 0, 0, 0xFF]);
    let inside = (40 * 256 + 48) * 4;
    assert_eq!(
        &pixels[inside..inside + 4],
        &pixels_in(DmgPalette::Green)[..4]
    );
    assert_eq!(
        screen.screen_pixels().unwrap(),
        pixels_in(DmgPalette::Green)
    );
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
