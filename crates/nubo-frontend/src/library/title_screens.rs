//! Pictures of each game's title screen for the library, made by running
//! the game for a few seconds with no window or sound, which takes about
//! half a second per game. No download or matching is needed, so every ROM
//! gets one, even homebrew.
//!
//! Games show logos and blank screens before their title, so a few moments
//! are tried and the frame with the most detail is kept.

use nubo_core::{Emulator, Model, NoSaves};
use std::sync::atomic::{AtomicBool, Ordering};

/// The moments (in frames, about 60 a second) at which a frame is looked
/// at: 2.5, 5, 7.5 and 10 seconds after the boot ROM hands over to the game.
const SAMPLE_FRAMES: [u32; 4] = [150, 300, 450, 600];
/// Gives up on games that stop drawing frames, such as ones that crash.
const MAX_STEPS: u32 = 40_000_000;
/// The most instructions one piece of a capture runs (see [`Capture::run`]).
/// An instruction takes at least one machine cycle, so that is at least 17
/// frames' worth (half that in a Game Boy Color game's double speed), and
/// usually two or three times as many. The limit lets the browser draw the
/// page between pieces even while a game stops drawing frames.
const MAX_STEPS_AT_ONCE: u32 = 300_000;
/// The sample rate the emulator makes its (unused) sound at.
const SAMPLE_RATE: u32 = 32_768;
/// A frame needs at least this much detail to count as a title screen,
/// so a blank or nearly blank screen is never kept.
const MIN_DETAIL: usize = 200;

/// Whether captures that run a few frames at a time wait (see [`hold`]).
static HELD: AtomicBool = AtomicBool::new(false);

/// Holds captures that run a few frames at a time (in the browser) back
/// between their pieces while `held`, such as while a game runs on the same
/// single thread, so it runs smoothly; they go on once it is let go.
pub fn hold(held: bool) {
    HELD.store(held, Ordering::Relaxed);
}

/// Whether captures are held back (see [`hold`]).
pub fn is_held() -> bool {
    HELD.load(Ordering::Relaxed)
}

/// Runs `rom` and returns the RGBA pixels of its title screen, or `None` if
/// it never showed anything worth a picture. Color games run on a Game Boy
/// Color, the rest on an original Game Boy, so DMG pictures come out gray.
pub fn capture(rom: &[u8]) -> Option<Vec<u8>> {
    let mut capture = Capture::start(rom)?;
    loop {
        if let Some(pixels) = capture.run(u32::MAX) {
            return pixels;
        }
    }
}

/// A title screen being captured, a few frames at a time, so the browser,
/// which has a single thread, can draw the page between them (see
/// [`capture`]).
pub struct Capture {
    emulator: Emulator,
    /// The frame with the most detail so far, and how much it has.
    best: Option<(usize, Vec<u8>)>,
    /// The game's frames so far, after the boot ROM.
    frames: u32,
    /// The instructions run so far.
    steps: u32,
}

impl Capture {
    /// Starts `rom` for its title screen, or returns `None` if it isn't a
    /// game the emulator can start.
    pub fn start(rom: &[u8]) -> Option<Capture> {
        let header = Emulator::read_rom_header(rom).ok()?;
        let mut emulator = Emulator::new(if header.supports_cgb {
            Model::CgbE
        } else {
            Model::Dmg
        });
        emulator.set_sample_rate(SAMPLE_RATE);
        // Running a game for its picture must never touch the player's saves.
        emulator.load_rom(rom, Box::new(NoSaves::default())).ok()?;
        Some(Capture {
            emulator,
            best: None,
            frames: 0,
            steps: 0,
        })
    }

    /// Runs up to `frame_count` more of the game's frames, and no more than
    /// `MAX_STEPS_AT_ONCE` instructions, unless `frame_count` is `u32::MAX`
    /// (the whole capture at once, see [`capture`]). Returns the title
    /// screen once the capture is done (`None` inside if the game showed
    /// nothing worth a picture), or `None` while it isn't.
    pub fn run(&mut self, frame_count: u32) -> Option<Option<Vec<u8>>> {
        let last_sample = SAMPLE_FRAMES[SAMPLE_FRAMES.len() - 1];
        let stop = self.frames.saturating_add(frame_count);
        let step_limit = if frame_count == u32::MAX {
            MAX_STEPS
        } else {
            self.steps.saturating_add(MAX_STEPS_AT_ONCE).min(MAX_STEPS)
        };

        while self.steps < MAX_STEPS {
            if self.steps >= step_limit {
                return None;
            }
            // One instruction, as `Emulator::step` runs it, but dropping the
            // sound once a buffer of it is full, so it can't pile up.
            self.emulator.step_toward_next_audio_buffer(1);
            self.steps += 1;

            // The boot ROM's logo is the same for every game, so the moments
            // count from when the game itself starts.
            let booting = self.emulator.is_running_boot_rom();
            let Some(pixels) = self.emulator.take_frame() else {
                continue;
            };
            if booting {
                continue;
            }
            self.frames += 1;

            if SAMPLE_FRAMES.contains(&self.frames) {
                let score = detail(pixels);
                if score >= MIN_DETAIL && self.best.as_ref().is_none_or(|(best, _)| score > *best) {
                    self.best = Some((score, pixels.to_vec()));
                }
            }

            if self.frames >= last_sample {
                break;
            }
            if self.frames >= stop {
                return None;
            }
        }

        Some(self.best.take().map(|(_, pixels)| pixels))
    }
}

/// How busy a frame looks: the number of pixels whose color differs from
/// the pixel to their right. A blank screen scores 0.
fn detail(pixels: &[u8]) -> usize {
    let width = crate::picture::SCREEN_WIDTH as usize;
    let (rows, _) = pixels.as_chunks::<4>();

    rows.chunks(width)
        .map(|row| row.windows(2).filter(|pair| pair[0] != pair[1]).count())
        .sum()
}

#[cfg(test)]
mod tests;
