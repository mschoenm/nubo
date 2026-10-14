//! Tests for picking a title screen.

use super::*;
use nubo_frontend::picture::{SCREEN_HEIGHT, SCREEN_WIDTH};

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
