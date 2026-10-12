//! Pictures of each game's title screen for the library, made by running
//! the game for a few seconds with no window or sound. No download or
//! matching is needed, so every ROM gets one, even homebrew.
//!
//! Games show logos and blank screens before their title, so a few moments
//! are tried and the frame with the most detail is kept.

use nubo_core::{Emulator, Model, NoSaves};

/// The moments (in frames, about 60 a second) at which a frame is looked
/// at: 2.5, 5, 7.5 and 10 seconds in.
const SAMPLE_FRAMES: [u32; 4] = [150, 300, 450, 600];
/// Gives up on games that stop drawing frames, such as ones that crash.
const MAX_STEPS: u32 = 40_000_000;
/// The sample rate the emulator makes its (unused) sound at.
const SAMPLE_RATE: u32 = 32_768;
/// A frame needs at least this much detail to count as a title screen,
/// so a blank or nearly blank screen is never kept.
const MIN_DETAIL: usize = 200;

/// Runs `rom` and returns the RGBA pixels of its title screen, or `None` if
/// it never showed anything worth a picture. Color games run on a Game Boy
/// Color, the rest on an original Game Boy, so DMG pictures come out gray.
pub fn capture(rom: &[u8]) -> Option<Vec<u8>> {
    let header = Emulator::read_rom_header(rom).ok()?;
    let mut emulator = Emulator::new(if header.supports_cgb {
        Model::CgbE
    } else {
        Model::Dmg
    });
    emulator.set_sample_rate(SAMPLE_RATE);
    // Running a game for its picture must never touch the player's saves.
    emulator.load_rom(rom, Box::new(NoSaves::default())).ok()?;

    let mut best: Option<(usize, Vec<u8>)> = None;
    let mut frames = 0;
    let last_sample = SAMPLE_FRAMES[SAMPLE_FRAMES.len() - 1];

    for _ in 0..MAX_STEPS {
        emulator.step();

        let Some(pixels) = emulator.take_frame() else {
            continue;
        };
        frames += 1;

        if SAMPLE_FRAMES.contains(&frames) {
            let score = detail(pixels);
            if score >= MIN_DETAIL && best.as_ref().is_none_or(|(best, _)| score > *best) {
                best = Some((score, pixels.to_vec()));
            }
        }

        if frames >= last_sample {
            break;
        }
    }

    best.map(|(_, pixels)| pixels)
}

/// How busy a frame looks: the number of pixels whose color differs from
/// the pixel to their right. A blank screen scores 0.
fn detail(pixels: &[u8]) -> usize {
    let width = crate::app::SCREEN_WIDTH as usize;
    let (rows, _) = pixels.as_chunks::<4>();

    rows.chunks(width)
        .map(|row| row.windows(2).filter(|pair| pair[0] != pair[1]).count())
        .sum()
}

#[cfg(test)]
mod tests;
