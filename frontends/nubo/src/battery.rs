//! Battery-backed saves. Some cartridges keep their RAM (and, for MBC3
//! cartridges, a real-time clock or RTC) alive with a battery so games can
//! save. This module stores that RAM in `<rom name>.sav` and the clock in
//! `<rom name>.rtc` in the user's data folder (or in localStorage in the
//! browser build).
//!
//! Games write save RAM often, and write a save in several pieces over a
//! few frames, so writes are collected and go to disk once the game has
//! stopped writing for a moment (see `SETTLE_TIME`), when it pauses and
//! when it stops. That way the app ending without stopping the game (see
//! `main.rs`) most likely leaves a whole save on disk, not half of one.

use crate::notices::Notices;
use nubo_core::{CartridgeEffects, RtcState};
use nubo_frontend::time::{Duration, Instant, SystemTime};
use slot::SaveSlot;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

/// How long the game has to stop writing before what it wrote goes to disk.
const SETTLE_TIME: Duration = Duration::from_millis(250);
/// A game that keeps writing is still saved this often: some games use the
/// battery-backed RAM as scratch space, and some read the clock every frame.
const LONGEST_DELAY: Duration = Duration::from_secs(1);
/// Cartridge RAM sizes are multiples of 2 KiB. Other emulators append an RTC
/// footer to `.sav` files, and rounding down to this size drops it.
const RAM_SIZE_GRANULARITY: usize = 0x800;
/// The size of the `.rtc` file written by `encode_rtc`.
const RTC_FILE_SIZE: usize = 17;

/// Save data waiting to be written.
#[derive(Default)]
struct PendingWrites {
    ram: Option<Pending<Vec<u8>>>,
    rtc: Option<Pending<[u8; RTC_FILE_SIZE]>>,
}

/// The data for one save file, waiting to be written, and when.
struct Pending<T> {
    data: T,
    /// When the game will have stopped writing for `SETTLE_TIME`, unless it
    /// writes again.
    settles_at: Instant,
    /// When the data is written even if the game keeps writing.
    deadline: Instant,
}

impl<T> Pending<T> {
    /// Data the game wrote at `now`.
    fn new(data: T, now: Instant) -> Self {
        Pending {
            data,
            settles_at: now + SETTLE_TIME,
            deadline: now + LONGEST_DELAY,
        }
    }

    /// Notes that the game wrote again at `now`.
    fn written_again(&mut self, now: Instant) {
        self.settles_at = now + SETTLE_TIME;
    }

    fn is_due(&self, now: Instant) -> bool {
        now >= self.settles_at || now >= self.deadline
    }

    /// Tries again after `LONGEST_DELAY`, after a write failed at `now`.
    fn retry_later(&mut self, now: Instant) {
        self.settles_at = now + LONGEST_DELAY;
        self.deadline = self.settles_at;
    }
}

/// The save slots for one ROM and the writes waiting to go into them. It is
/// shared between the emulator (which queues writes) and the session (which
/// flushes them).
pub struct BatterySaves {
    ram_slot: SaveSlot,
    rtc_slot: SaveSlot,
    pending: Mutex<PendingWrites>,
    /// Where failed writes are reported.
    notices: Notices,
}

impl BatterySaves {
    /// Creates the saves for the ROM at `rom_path`; the slots are named after
    /// it. Writes that fail are reported to `notices`.
    pub fn for_rom(rom_path: &Path, notices: Notices) -> Arc<BatterySaves> {
        Self::new(
            SaveSlot::for_rom(rom_path, "sav"),
            SaveSlot::for_rom(rom_path, "rtc"),
            notices,
        )
    }

    /// The saves for the ROM at `rom_path`, kept in `directory`, for tests
    /// and measurements that mustn't touch the player's saves.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn in_directory(directory: &Path, rom_path: &Path) -> Arc<BatterySaves> {
        Self::new(
            SaveSlot::in_directory(Some(directory), rom_path, "sav"),
            SaveSlot::in_directory(Some(directory), rom_path, "rtc"),
            Notices::default(),
        )
    }

    fn new(ram_slot: SaveSlot, rtc_slot: SaveSlot, notices: Notices) -> Arc<BatterySaves> {
        Arc::new(BatterySaves {
            ram_slot,
            rtc_slot,
            notices,
            pending: Mutex::new(PendingWrites::default()),
        })
    }

    /// Writes the saves the game has stopped writing to for `SETTLE_TIME`,
    /// or has kept writing to for `LONGEST_DELAY`.
    pub fn flush_when_settled(&self) {
        self.flush_due(Instant::now());
    }

    /// Writes the saves due at `now` (see [`BatterySaves::flush_when_settled`]).
    fn flush_due(&self, now: Instant) {
        let mut pending = self.pending();
        self.write(&self.ram_slot, &mut pending.ram, Some(now));
        self.write(&self.rtc_slot, &mut pending.rtc, Some(now));
    }

    /// Writes pending saves now.
    pub fn flush(&self) {
        let mut pending = self.pending();
        self.write(&self.ram_slot, &mut pending.ram, None);
        self.write(&self.rtc_slot, &mut pending.rtc, None);
    }

    /// Locks the pending writes. A panic while the lock was held doesn't make
    /// the data invalid, so a poisoned lock is used anyway.
    fn pending(&self) -> MutexGuard<'_, PendingWrites> {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Writes the data waiting in `pending` to `slot` if it is due at `now`,
    /// or right away without `now`. Data that fails to write is kept and
    /// tried again later.
    fn write<T: AsRef<[u8]>>(
        &self,
        slot: &SaveSlot,
        pending: &mut Option<Pending<T>>,
        now: Option<Instant>,
    ) {
        let Some(waiting) = pending else {
            return;
        };
        if now.is_some_and(|now| !waiting.is_due(now)) {
            return;
        }

        if self.write_or_report(slot, waiting.data.as_ref()) {
            *pending = None;
        } else {
            waiting.retry_later(now.unwrap_or_else(Instant::now));
        }
    }

    /// Writes `contents` to `slot` and returns whether it worked, reporting
    /// the error if it didn't.
    fn write_or_report(&self, slot: &SaveSlot, contents: &[u8]) -> bool {
        match slot.write(contents) {
            Ok(()) => true,
            Err(error) => {
                self.notices
                    .report(format!("Could not save the game to {slot}: {error}"));
                false
            }
        }
    }

    /// Reads the saved RAM, dropping any footer that isn't part of the RAM.
    fn load_ram(&self) -> Option<Vec<u8>> {
        let mut ram = self.ram_slot.read()?;
        ram.truncate(ram.len() / RAM_SIZE_GRANULARITY * RAM_SIZE_GRANULARITY);
        (!ram.is_empty()).then_some(ram)
    }

    /// Queues RAM the game wrote at `now`, reusing the queued buffer if
    /// possible.
    fn queue_ram(&self, ram: &[u8], now: Instant) {
        let mut pending = self.pending();
        match &mut pending.ram {
            Some(waiting) => {
                if waiting.data.len() == ram.len() {
                    waiting.data.copy_from_slice(ram);
                } else {
                    waiting.data = ram.to_vec();
                }
                waiting.written_again(now);
            }
            None => pending.ram = Some(Pending::new(ram.to_vec(), now)),
        }
    }

    fn load_rtc(&self) -> Option<RtcState> {
        decode_rtc(&self.rtc_slot.read()?)
    }

    /// Queues the clock state the game left at `now`.
    fn queue_rtc(&self, state: &RtcState, now: Instant) {
        let mut pending = self.pending();
        match &mut pending.rtc {
            Some(waiting) => {
                waiting.data = encode_rtc(state);
                waiting.written_again(now);
            }
            None => pending.rtc = Some(Pending::new(encode_rtc(state), now)),
        }
    }
}

/// Connects the emulated cartridge to [`BatterySaves`] and the system clock.
pub struct BatteryCartridgeEffects {
    saves: Arc<BatterySaves>,
}

impl BatteryCartridgeEffects {
    pub fn new(saves: Arc<BatterySaves>) -> Self {
        BatteryCartridgeEffects { saves }
    }
}

impl CartridgeEffects for BatteryCartridgeEffects {
    fn current_time_millis(&self) -> f64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as f64
    }

    fn load_rtc_state(&self) -> Option<RtcState> {
        self.saves.load_rtc()
    }

    fn save_rtc_state(&mut self, state: &RtcState) {
        self.saves.queue_rtc(state, Instant::now());
    }

    fn load_ram(&self) -> Option<Vec<u8>> {
        self.saves.load_ram()
    }

    fn save_ram(&mut self, ram: &[u8]) {
        self.saves.queue_ram(ram, Instant::now());
    }
}

/// Packs the clock state into the little-endian `.rtc` file layout.
fn encode_rtc(state: &RtcState) -> [u8; RTC_FILE_SIZE] {
    let mut bytes = [0; RTC_FILE_SIZE];
    bytes[0..2].copy_from_slice(&state.milliseconds.to_le_bytes());
    bytes[2] = state.seconds;
    bytes[3] = state.minutes;
    bytes[4] = state.hours;
    bytes[5..7].copy_from_slice(&state.days.to_le_bytes());
    bytes[7..15].copy_from_slice(&state.base_timestamp.to_le_bytes());
    bytes[15] = u8::from(state.halted);
    bytes[16] = u8::from(state.day_carry);
    bytes
}

/// Unpacks a `.rtc` file, or returns `None` if it has the wrong size.
fn decode_rtc(bytes: &[u8]) -> Option<RtcState> {
    let bytes: &[u8; RTC_FILE_SIZE] = bytes.try_into().ok()?;

    Some(RtcState {
        milliseconds: u16::from_le_bytes([bytes[0], bytes[1]]),
        seconds: bytes[2],
        minutes: bytes[3],
        hours: bytes[4],
        days: u16::from_le_bytes([bytes[5], bytes[6]]),
        base_timestamp: f64::from_le_bytes(bytes[7..15].try_into().ok()?),
        halted: bytes[15] != 0,
        day_carry: bytes[16] != 0,
    })
}

#[cfg_attr(target_arch = "wasm32", path = "battery/local_storage_slot.rs")]
#[cfg_attr(not(target_arch = "wasm32"), path = "battery/file_slot.rs")]
mod slot;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
