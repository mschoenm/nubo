//! Starts each test where the boot ROM hands over to the cartridge.
//!
//! The boot ROM runs for 1.7 emulated seconds on the DMG, 3.1 on the CGB and
//! 3.5 on the Super Game Boy, far longer than most tests. It only looks at a few header bytes of the ROM
//! ([`Emulator::boot_rom_input`]), so each worker thread runs it once per
//! model and set of those bytes and starts the other ROMs that share them
//! from a copy ([`Emulator::clone_with_rom`]). With `--verify-boot` every
//! test runs the real boot ROM instead and checks that the copy would have
//! been identical, which makes the whole run take about two and a half
//! minutes instead of about 30 seconds. The core test
//! `clone_with_rom_matches_a_real_boot` checks the same for every model, with
//! ROMs that differ everywhere outside the header bytes the boot ROM reads.

use crate::runner::{Progress, STALL_LIMIT};
use crate::suites::Model;
use nubo_core::{Emulator, NoSaves, RtcClock};
use std::collections::HashMap;
use std::time::Instant;

/// The core's audio buffer holds 512 samples, so at this sample rate one
/// buffer is exactly 1/128 of a second of emulated time.
pub(crate) const SAMPLE_RATE: u32 = 65536;
/// Instructions to run between checks for a stalled boot.
const STEPS_BETWEEN_STALL_CHECKS: u32 = 10_000;

/// Cartridge hooks with no saves and a real-time clock that counts emulated
/// time, so every run is the same.
const NO_SAVES: NoSaves = NoSaves {
    rtc_clock: RtcClock::EmulatedTime,
};

/// Why a test couldn't be started where the boot ROM hands over.
pub(crate) enum BootError {
    /// The boot ROM's emulated time stopped advancing.
    TimedOut,
    /// The ROM couldn't be loaded, or a copied boot differs from a real one.
    Failed(String),
}

impl From<String> for BootError {
    fn from(reason: String) -> Self {
        BootError::Failed(reason)
    }
}

/// An emulator that has just finished the boot ROM.
pub(crate) struct Booted {
    pub(crate) emulator: Emulator,
    /// How many audio buffers filled up during the boot ROM. Tests count
    /// time in audio buffers from power-on, so they carry on from here.
    pub(crate) audio_buffers: u32,
}

impl Booted {
    /// A copy with another ROM inserted.
    fn clone_with_rom(&self, rom: &[u8]) -> Result<Booted, String> {
        Ok(Booted {
            emulator: self
                .emulator
                .clone_with_rom(rom, Box::new(NO_SAVES))
                .map_err(|error| format!("failed to load ROM: {error}"))?,
            audio_buffers: self.audio_buffers,
        })
    }
}

/// Emulators that just finished the boot ROM, by model and the bytes of the
/// ROM the boot ROM looked at.
pub(crate) struct BootCache {
    snapshots: HashMap<(Model, Vec<u8>), Booted>,
    verify: bool,
}

impl BootCache {
    /// An empty cache. With `verify`, every boot runs the real boot ROM and
    /// is compared with the copy the cache would have handed out.
    pub(crate) fn new(verify: bool) -> Self {
        BootCache {
            snapshots: HashMap::new(),
            verify,
        }
    }

    /// An emulator for `model` with `rom` inserted that has just finished the
    /// boot ROM, ticking `progress` as it runs. Fails if the ROM can't be
    /// loaded, the boot ROM stalls, or (when verifying) the copy differs from
    /// a real boot.
    pub(crate) fn boot(
        &mut self,
        rom: &[u8],
        model: Model,
        progress: &Progress<'_>,
    ) -> Result<Booted, BootError> {
        let Some(input) = rom.get(Emulator::boot_rom_input(model)) else {
            return run_boot_rom(rom, model, progress);
        };
        let key = (model, input.to_vec());

        if let Some(snapshot) = self.snapshots.get(&key) {
            let copy = snapshot.clone_with_rom(rom)?;
            if !self.verify {
                return Ok(copy);
            }
            let booted = run_boot_rom(rom, model, progress)?;
            check_same_state(&copy, &booted)?;
            return Ok(booted);
        }

        let booted = run_boot_rom(rom, model, progress)?;
        self.snapshots.insert(key, booted.clone_with_rom(rom)?);
        Ok(booted)
    }
}

/// Powers on an emulator for `model` with `rom` inserted and runs it until
/// the boot ROM hands over to the cartridge. Fails if one audio buffer takes
/// longer than `STALL_LIMIT` of real time.
fn run_boot_rom(rom: &[u8], model: Model, progress: &Progress<'_>) -> Result<Booted, BootError> {
    let mut emulator = Emulator::new(model);
    emulator.set_sample_rate(SAMPLE_RATE);
    emulator
        .load_rom(rom, Box::new(NO_SAVES))
        .map_err(|error| format!("failed to load ROM: {error}"))?;

    let mut audio_buffers = 0;
    let mut steps = 0;
    let mut buffer_started = Instant::now();
    while emulator.is_running_boot_rom() {
        if emulator.step_toward_next_audio_buffer(1) {
            audio_buffers += 1;
            buffer_started = Instant::now();
            progress.tick();
        }
        steps += 1;
        if steps % STEPS_BETWEEN_STALL_CHECKS == 0 && buffer_started.elapsed() > STALL_LIMIT {
            return Err(BootError::TimedOut);
        }
    }
    Ok(Booted {
        emulator,
        audio_buffers,
    })
}

/// Fails with the first difference between a copied and a really booted
/// emulator, comparing their complete state as printed by `Debug`.
fn check_same_state(copy: &Booted, booted: &Booted) -> Result<(), String> {
    if copy.audio_buffers != booted.audio_buffers {
        return Err(format!(
            "the copied boot filled {} audio buffers, a real boot {}",
            copy.audio_buffers, booted.audio_buffers
        ));
    }
    let copied = format!("{:?}", copy.emulator);
    let real = format!("{:?}", booted.emulator);
    let Some(position) = copied
        .bytes()
        .zip(real.bytes())
        .position(|(copied, real)| copied != real)
        .or_else(|| (copied.len() != real.len()).then(|| copied.len().min(real.len())))
    else {
        return Ok(());
    };
    let excerpt = |state: &str| {
        let start = state.floor_char_boundary(position.saturating_sub(120));
        let end = state.ceil_char_boundary((position + 60).min(state.len()));
        state[start..end].to_string()
    };
    Err(format!(
        "the copied post-boot state differs from a real boot\n      copy: ...{}\n      boot: ...{}",
        excerpt(&copied),
        excerpt(&real)
    ))
}
