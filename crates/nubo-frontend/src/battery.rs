//! Battery-backed saves. Some cartridges keep their RAM (and, for MBC3
//! cartridges, a real-time clock or RTC) alive with a battery so games can
//! save. This module keeps that RAM ([`Entry::BatteryRam`], `<rom name>.sav`)
//! and the clock ([`Entry::BatteryClock`], `<rom name>.rtc`) in a
//! [`Storage`]: the user's data folder on the desktop, localStorage in the
//! browser.
//!
//! Games write save RAM often, and write a save in several pieces over a
//! few frames, so writes are collected and stored once the game has
//! stopped writing for a moment (see `SETTLE_TIME`), when it pauses and
//! when it stops. That way the app ending without stopping the game most
//! likely leaves a whole save behind, not half of one.

use crate::notices::Notices;
use crate::storage::{self, Entry, Storage};
use crate::time::{Duration, Instant, SystemTime};
use nubo_core::{CartridgeEffects, RtcState};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

/// How long the game has to stop writing before what it wrote is stored.
const SETTLE_TIME: Duration = Duration::from_millis(250);
/// A game that keeps writing is still saved this often: some games use the
/// battery-backed RAM as scratch space, and some read the clock every frame.
const LONGEST_DELAY: Duration = Duration::from_secs(1);
/// Cartridge RAM sizes are multiples of 2 KiB. Other emulators append an RTC
/// footer to `.sav` files, and rounding down to this size drops it.
const RAM_SIZE_GRANULARITY: usize = 0x800;

/// Save data waiting to be written.
#[derive(Default)]
struct PendingWrites {
    ram: Option<Pending<Vec<u8>>>,
    rtc: Option<Pending<[u8; RtcState::BYTES]>>,
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

/// Deletes the battery saves of every game in this build's storage (see
/// [`storage::platform`]), for a player resetting the settings who wants
/// their games to start from the beginning. A running game would write its
/// save again, so apps only offer this while no game runs.
pub fn delete_all() -> Result<(), String> {
    storage::platform()
        .delete_battery_saves()
        .map_err(|error| format!("Couldn't delete the in-game saves: {error}"))
}

/// Where one ROM's saves are kept and the writes waiting to go there. It is
/// shared between the emulator (which queues writes) and the session (which
/// flushes them).
pub struct BatterySaves {
    /// Where the saves are kept.
    storage: Arc<dyn Storage>,
    /// The ROM the saves belong to, which names their entries.
    rom_path: PathBuf,
    pending: Mutex<PendingWrites>,
    /// Where failed writes are reported.
    notices: Notices,
}

impl BatterySaves {
    /// Creates the saves for the ROM at `rom_path` in this build's storage
    /// (see [`storage::platform`]); they are named after the ROM. Writes that
    /// fail are reported to `notices`.
    pub fn for_rom(rom_path: &Path, notices: Notices) -> Arc<BatterySaves> {
        Self::in_storage(storage::platform(), rom_path, notices)
    }

    /// The saves for the ROM at `rom_path`, kept in `saves/` in
    /// `data_folder`, for tests and measurements that mustn't touch the
    /// player's saves.
    #[cfg(feature = "desktop")]
    pub fn in_data_folder(data_folder: &Path, rom_path: &Path) -> Arc<BatterySaves> {
        let files = storage::files::Files::in_folders(None, Some(data_folder.to_path_buf()));
        Self::in_storage(Arc::new(files), rom_path, Notices::default())
    }

    /// The saves for the ROM at `rom_path`, kept in `storage`. Writes that
    /// fail are reported to `notices`.
    pub fn in_storage(
        storage: Arc<dyn Storage>,
        rom_path: &Path,
        notices: Notices,
    ) -> Arc<BatterySaves> {
        Arc::new(BatterySaves {
            storage,
            rom_path: rom_path.to_path_buf(),
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
        self.write(self.ram_entry(), &mut pending.ram, Some(now));
        self.write(self.clock_entry(), &mut pending.rtc, Some(now));
    }

    /// Writes pending saves now.
    pub fn flush(&self) {
        let mut pending = self.pending();
        self.write(self.ram_entry(), &mut pending.ram, None);
        self.write(self.clock_entry(), &mut pending.rtc, None);
    }

    /// Locks the pending writes. A panic while the lock was held doesn't make
    /// the data invalid, so a poisoned lock is used anyway.
    fn pending(&self) -> MutexGuard<'_, PendingWrites> {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Where the RAM is kept.
    fn ram_entry(&self) -> Entry<'_> {
        Entry::BatteryRam(&self.rom_path)
    }

    /// Where the clock is kept.
    fn clock_entry(&self) -> Entry<'_> {
        Entry::BatteryClock(&self.rom_path)
    }

    /// Writes the data waiting in `pending` to `entry` if it is due at `now`,
    /// or right away without `now`. Data that fails to write is kept and
    /// tried again later.
    fn write<T: AsRef<[u8]>>(
        &self,
        entry: Entry<'_>,
        pending: &mut Option<Pending<T>>,
        now: Option<Instant>,
    ) {
        let Some(waiting) = pending else {
            return;
        };
        if now.is_some_and(|now| !waiting.is_due(now)) {
            return;
        }

        if self.write_or_report(entry, waiting.data.as_ref()) {
            *pending = None;
        } else {
            waiting.retry_later(now.unwrap_or_else(Instant::now));
        }
    }

    /// Writes `contents` to `entry` and returns whether it worked, reporting
    /// the error if it didn't.
    fn write_or_report(&self, entry: Entry<'_>, contents: &[u8]) -> bool {
        match self.storage.write(entry, contents) {
            Ok(()) => true,
            Err(error) => {
                let location = self.storage.location(entry);
                self.notices
                    .report(format!("Could not save the game to {location}: {error}"));
                false
            }
        }
    }

    /// Reads what is kept for `entry`, or `None` if nothing is or it can't
    /// be read.
    fn read(&self, entry: Entry<'_>) -> Option<Vec<u8>> {
        self.storage.read(entry).ok().flatten()
    }

    /// Reads the saved RAM, dropping any footer that isn't part of the RAM.
    fn load_ram(&self) -> Option<Vec<u8>> {
        let mut ram = self.read(self.ram_entry())?;
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
        RtcState::from_bytes(&self.read(self.clock_entry())?)
    }

    /// Queues the clock state the game left at `now`.
    fn queue_rtc(&self, state: &RtcState, now: Instant) {
        let mut pending = self.pending();
        match &mut pending.rtc {
            Some(waiting) => {
                waiting.data = state.to_bytes();
                waiting.written_again(now);
            }
            None => pending.rtc = Some(Pending::new(state.to_bytes(), now)),
        }
    }
}

/// Connects the emulated cartridge to [`BatterySaves`] and the system clock.
pub struct BatteryCartridgeEffects {
    saves: Arc<BatterySaves>,
}

impl BatteryCartridgeEffects {
    /// Connects a cartridge to `saves`.
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

#[cfg(all(test, feature = "desktop"))]
mod tests;
