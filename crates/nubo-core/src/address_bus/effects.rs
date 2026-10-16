//! The hooks a frontend provides so a cartridge can keep its battery-backed
//! save data and its real-time clock between sessions.

use crate::address_bus::mbc3::RtcState;
use core::fmt::Debug;
use nubo_serializable_derive::Serializable;

/// Lets the emulator core reach the outside world for things a real cartridge
/// does on its own: reading the current time and keeping its save data.
///
/// Every frontend passes one to [`Emulator::load_rom`](crate::Emulator::load_rom);
/// the core never touches files, storage or clocks directly. Each one belongs
/// to one game, so the methods don't say which game they are about. Only
/// [`CartridgeEffects::current_time_millis`] has to be written; the load and
/// save methods keep nothing unless a frontend overrides them, and
/// [`NoSaves`] is ready-made for frontends that keep nothing at all. The RTC
/// (real-time clock) is the clock chip on MBC3 cartridges (MBC = memory bank
/// controller, the chip on the cartridge that switches ROM and RAM banks).
///
/// It is `Send`, so an emulator can move to another thread: an app may
/// start a game on one thread and run it on another, and the RetroArch core
/// keeps its emulator in a `static`.
pub trait CartridgeEffects: Send {
    /// The current wall-clock time in milliseconds. The RTC measures the time
    /// that passed between two calls, so any fixed starting point works, as
    /// long as it is the same across sessions (for example the Unix epoch).
    fn current_time_millis(&self) -> f64;

    /// Which clock the real-time clock follows. The wall clock unless a
    /// frontend overrides this.
    fn rtc_clock(&self) -> RtcClock {
        RtcClock::WallClock
    }

    /// Loads the saved state of the game's RTC, or `None` if there is none.
    fn load_rtc_state(&self) -> Option<RtcState> {
        None
    }

    /// Saves the state of the game's RTC. Called at the end of each
    /// `Emulator` call that ran the CPU if the game wrote the clock, or
    /// latched it after time had passed, during it, so at most once per call.
    fn save_rtc_state(&mut self, _state: &RtcState) {}

    /// Loads the game's saved cartridge RAM, or `None` if there is none.
    /// Called once when the ROM is loaded.
    fn load_ram(&self) -> Option<Vec<u8>> {
        None
    }

    /// Saves the whole cartridge RAM. Called at the end of each `Emulator`
    /// call that ran the CPU (such as `Emulator::step_until_next_audio_buffer`)
    /// if the game wrote battery-backed RAM during it, so at most once per
    /// call.
    fn save_ram(&mut self, _ram: &[u8]) {}
}

/// The clock an MBC3's real-time clock (RTC) counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serializable)]
pub enum RtcClock {
    /// The wall clock from [`CartridgeEffects::current_time_millis`], so the
    /// game's clock keeps up with real time, also while the emulator is
    /// paused or closed.
    #[default]
    WallClock,
    /// The time the emulator has run, counted in Game Boy clock cycles, like
    /// a real cartridge's own clock crystal would. Every run is the same,
    /// which test runners need, and save states keep this time.
    EmulatedTime,
}

/// Cartridge effects that keep nothing: every game starts without save data,
/// and whatever it saves is dropped. The wall clock always reads zero, so
/// with the default [`RtcClock::WallClock`] the real-time clock stands still;
/// test runners and benchmarks pick [`RtcClock::EmulatedTime`] instead, so it
/// runs the same every time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NoSaves {
    /// The clock the cartridge's real-time clock follows.
    pub rtc_clock: RtcClock,
}

impl CartridgeEffects for NoSaves {
    fn current_time_millis(&self) -> f64 {
        0.0
    }

    fn rtc_clock(&self) -> RtcClock {
        self.rtc_clock
    }
}

impl Debug for dyn CartridgeEffects {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "CartridgeEffects")
    }
}

/// A boxed [`NoSaves`] with a clock that stands still, used before a ROM is
/// loaded and in tests.
pub(crate) fn empty_cartridge_effects() -> Box<dyn CartridgeEffects> {
    Box::new(NoSaves::default())
}

/// A value tests share with the cartridge effects they hand the emulator:
/// like `Rc<Cell<T>>`, but `Send`, as cartridge effects must be.
#[cfg(test)]
#[derive(Debug, Clone, Default)]
pub(crate) struct Shared<T>(std::sync::Arc<std::sync::Mutex<T>>);

#[cfg(test)]
impl<T: Copy> Shared<T> {
    pub(crate) fn new(value: T) -> Self {
        Shared(std::sync::Arc::new(std::sync::Mutex::new(value)))
    }

    pub(crate) fn get(&self) -> T {
        *self.0.lock().unwrap()
    }

    pub(crate) fn set(&self, value: T) {
        *self.0.lock().unwrap() = value;
    }
}
