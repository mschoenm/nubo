//! Tests for picking a title screen.

use super::*;
use crate::picture::{SCREEN_HEIGHT, SCREEN_WIDTH};

const FRAME_SIZE: usize = (SCREEN_WIDTH * SCREEN_HEIGHT * 4) as usize;

#[test]
fn a_blank_frame_has_no_detail() {
    assert_eq!(detail(&vec![0xFF; FRAME_SIZE]), 0);
}

#[test]
fn stripes_have_detail_on_every_edge() {
    let mut pixels = vec![0xFF; FRAME_SIZE];
    for (index, pixel) in pixels.chunks_mut(4).enumerate() {
        if index % 2 == 1 {
            pixel[..3].fill(0);
        }
    }

    let edges_per_row = SCREEN_WIDTH as usize - 1;
    assert_eq!(detail(&pixels), edges_per_row * SCREEN_HEIGHT as usize);
}

#[test]
fn a_game_that_shows_nothing_gets_no_picture() {
    // A cartridge of NOPs never turns on anything worth a picture.
    let rom = vec![0; 0x8000];

    assert!(capture(&rom).is_none());
}

#[test]
fn files_that_are_no_roms_get_no_picture() {
    assert!(capture(b"not a rom").is_none());
}

#[test]
fn a_piece_of_a_capture_stays_short_without_frames_that_count() {
    // The boot ROM's frames don't count, so the first piece ends at its
    // limit on instructions rather than after ten frames.
    let rom = vec![0; 0x8000];
    let mut capture = Capture::start(&rom).unwrap();

    assert!(capture.run(10).is_none());
    assert!(capture.steps <= MAX_STEPS_AT_ONCE);
    assert!(capture.steps > 0);
}

#[test]
fn a_long_capture_keeps_no_more_than_a_buffer_of_sound() {
    let rom = vec![0; 0x8000];
    let mut capture = Capture::start(&rom).unwrap();

    for _ in 0..3 {
        assert!(capture.run(10).is_none());
    }
    // About a second of sound at the capture's rate, of which only the last
    // buffer (512 samples) may be left.
    let (left, right) = capture.emulator.audio_samples().unwrap_or_default();
    assert!(left.len() < 1024 && right.len() < 1024);
}
