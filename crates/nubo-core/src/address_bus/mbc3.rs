//! The MBC3 memory bank controller, which can also have a real-time clock
//! (RTC) that keeps counting while the console is off, powered by the
//! cartridge battery, and the MBC30, an MBC3 for up to 4 MiB of ROM and 64
//! KiB of RAM (Nubo treats an MBC3 cartridge with more as one, like SameBoy).
//!
//! The clock has five registers: seconds, minutes, hours, and a 9-bit day
//! counter split over DL (its lower 8 bits) and DH (its top bit, plus a halt
//! flag and a day carry flag). A game selects a register by writing 0x08-0x0C
//! where it would otherwise select a RAM bank, then writes it at
//! 0xA000-0xBFFF. Reads there show a copy of the registers, which any write to
//! 0x6000-0x7FFF latches (games write 0 and then 1): that is when Nubo brings
//! the clock up to date with the frontend's clock (or, with
//! [`RtcClock::EmulatedTime`], with the time the emulator has run). The bank
//! and latch behavior follows SameBoy (<https://github.com/LIJI32/SameBoy>,
//! Expat License, see the notice in `apu.rs`) and
//! CasualPokePlayer's latch-rtc-test and rtc-invalid-banks-test.

use crate::address_bus::bank_utils::{banked_read, banked_write};
use crate::address_bus::cartridge::lacks_ram;
use crate::address_bus::constants::{
    CARTRIDGE_TYPE_MBC3_TIMER_BATTERY, CARTRIDGE_TYPE_MBC3_TIMER_RAM_BATTERY, RAM_BANK_SIZE,
};
use crate::address_bus::effects::{CartridgeEffects, RtcClock};
use nubo_serializable_derive::Serializable;

/// The state of an MBC3 real-time clock. Frontends store it next to the
/// game's save so the clock keeps counting while the emulator is closed,
/// for example in the 17-byte layout of [`RtcState::to_bytes`].
#[derive(Debug, Default, Clone, PartialEq, Serializable)]
pub struct RtcState {
    /// Milliseconds into the current second, 0-999.
    pub milliseconds: u16,
    /// Seconds, normally 0-59. Games can write values up to 63.
    pub seconds: u8,
    /// Minutes, normally 0-59. Games can write values up to 63.
    pub minutes: u8,
    /// Hours, normally 0-23. Games can write values up to 31.
    pub hours: u8,
    /// Days, 0-511.
    pub days: u16,
    /// The time, in milliseconds, that the registers above were last brought
    /// up to date with, on the clock the cartridge follows ([`RtcClock`]):
    /// [`CartridgeEffects::current_time_millis`] on the wall clock, the time
    /// the emulator has run on emulated time.
    ///
    /// [`CartridgeEffects::current_time_millis`]: crate::CartridgeEffects::current_time_millis
    pub base_timestamp: f64,
    /// Whether the clock is stopped (bit 6 of DH).
    pub halted: bool,
    /// Whether the day counter went past 511 (bit 7 of DH). It stays set until
    /// the game clears it.
    pub day_carry: bool,
}

impl RtcState {
    /// The length of [`RtcState::to_bytes`]: the `.rtc` file the Nubo app
    /// keeps next to a battery save, and RetroArch's RTC memory.
    pub const BYTES: usize = 17;

    /// Packs the state into the little-endian `.rtc` layout: milliseconds
    /// (2 bytes), seconds, minutes, hours, days (2 bytes),
    /// `base_timestamp` (an `f64`, 8 bytes), `halted` and `day_carry`.
    pub fn to_bytes(&self) -> [u8; Self::BYTES] {
        let mut bytes = [0; Self::BYTES];
        bytes[0..2].copy_from_slice(&self.milliseconds.to_le_bytes());
        bytes[2] = self.seconds;
        bytes[3] = self.minutes;
        bytes[4] = self.hours;
        bytes[5..7].copy_from_slice(&self.days.to_le_bytes());
        bytes[7..15].copy_from_slice(&self.base_timestamp.to_le_bytes());
        bytes[15] = u8::from(self.halted);
        bytes[16] = u8::from(self.day_carry);
        bytes
    }

    /// Unpacks the layout of [`RtcState::to_bytes`], or returns `None` if
    /// `bytes` has another length.
    pub fn from_bytes(bytes: &[u8]) -> Option<RtcState> {
        let bytes: &[u8; Self::BYTES] = bytes.try_into().ok()?;
        let [timestamp @ .., halted, day_carry] = *bytes.last_chunk::<10>()?;
        Some(RtcState {
            milliseconds: u16::from_le_bytes([bytes[0], bytes[1]]),
            seconds: bytes[2],
            minutes: bytes[3],
            hours: bytes[4],
            days: u16::from_le_bytes([bytes[5], bytes[6]]),
            base_timestamp: f64::from_le_bytes(timestamp),
            halted: halted != 0,
            day_carry: day_carry != 0,
        })
    }

    /// The state with each register cut to the bits it has, as a game's
    /// writes are, and the milliseconds to under a second, so a damaged or
    /// hand-edited `.rtc` file can't make a register hold a value the
    /// hardware can't, such as 255 hours.
    pub(super) fn within_register_widths(mut self) -> Self {
        self.milliseconds %= 1000;
        self.seconds &= SECONDS_REGISTER_MAX;
        self.minutes &= MINUTES_REGISTER_MAX;
        self.hours &= HOURS_REGISTER_MAX;
        self.days &= DAYS_REGISTER_MAX;
        self
    }

    /// Moves the clock forward by `elapsed_milliseconds`. A register that holds
    /// an invalid value (such as 61 seconds) doesn't carry into the next one
    /// when it passes its normal limit: it keeps counting until it overflows
    /// its bits and wraps to 0, without a carry, and from there counts as
    /// usual, so 62 seconds plus 10 reads 8 seconds, as on SameBoy, which
    /// ticks once a second.
    fn advance(&mut self, elapsed_milliseconds: f64) {
        let total_milliseconds =
            u64::from(self.milliseconds).saturating_add(elapsed_milliseconds as u64);
        let (seconds, minutes_carry) = count_register(
            self.seconds,
            total_milliseconds / 1000,
            60,
            SECONDS_REGISTER_MAX,
        );
        let (minutes, hours_carry) =
            count_register(self.minutes, minutes_carry, 60, MINUTES_REGISTER_MAX);
        let (hours, days_carry) = count_register(self.hours, hours_carry, 24, HOURS_REGISTER_MAX);

        let days = u64::from(self.days) + days_carry;
        if days > u64::from(DAYS_REGISTER_MAX) {
            self.day_carry = true;
        }

        self.milliseconds = (total_milliseconds % 1000) as u16;
        self.seconds = seconds;
        self.minutes = minutes;
        self.hours = hours;
        self.days = (days % (u64::from(DAYS_REGISTER_MAX) + 1)) as u16;
    }

    /// Counts the whole milliseconds that passed since the clock was last
    /// brought up to date. The fraction of a millisecond left over stays in
    /// `base_timestamp`, so a game that latches the clock very often doesn't
    /// lose it. A halted clock doesn't count. Returns whether the clock
    /// changed, so the frontend saves it.
    ///
    /// A `base_timestamp` that isn't a number, as a damaged `.rtc` file or
    /// save state can hold, starts counting from `current_time` instead, and
    /// so does one in the future: the computer's clock went back, or the
    /// `.rtc` file came from a computer whose clock was ahead. Waiting for
    /// the old time to come back would stand the clock still for as long.
    fn catch_up(&mut self, current_time: f64) -> bool {
        if !self.base_timestamp.is_finite() {
            self.base_timestamp = current_time;
            return false;
        }
        if self.halted {
            return false;
        }
        if current_time < self.base_timestamp {
            self.base_timestamp = current_time;
            return true;
        }
        let elapsed_milliseconds = (current_time - self.base_timestamp).floor();
        if elapsed_milliseconds < 1.0 {
            return false;
        }
        self.advance(elapsed_milliseconds);
        self.base_timestamp += elapsed_milliseconds;
        true
    }

    /// Moves the clock over to another time source without changing what it
    /// shows: it counts the time that passed on the old source up to
    /// `old_now`, then goes on from `new_now`, the same moment on the new
    /// source, keeping the fraction of a millisecond it had counted.
    /// `base_timestamp` is a time on the source the clock follows, so a
    /// state read on the other one would otherwise stand still (wall-clock
    /// time read as emulated time) or jump decades ahead.
    pub(crate) fn rebase(&mut self, old_now: f64, new_now: f64) {
        let mut fraction = 0.0;
        if !self.halted && old_now >= self.base_timestamp {
            self.catch_up(old_now);
            fraction = old_now - self.base_timestamp;
        }
        self.base_timestamp = new_now - fraction;
    }
}

/// The registers and clock of an MBC3 or MBC30, with or without RAM and a
/// clock.
#[derive(Debug, Serializable)]
pub(super) struct Mbc3CartridgeMapper {
    /// The ROM bank visible at 0x4000-0x7FFF: the seven lower bits written to
    /// 0x2000-0x3FFF (all eight on an MBC30), where 0 selects bank 1.
    rom_bank_number: u8,
    /// Whether the game may access the RAM and the clock registers.
    ram_rtc_enabled: bool,
    /// What 0xA000-0xBFFF shows: the four lower bits written to 0x4000-0x5FFF.
    /// Bit 3 selects a clock register (0x08-0x0C, while 0x0D-0x0F read 0xFF)
    /// on a cartridge with the clock, and bits 0-2 the RAM bank otherwise.
    /// On an MBC3 with the clock (but not an MBC30), RAM banks 4-7 read 0xFF.
    ram_bank_or_rtc_register: u8,
    /// The clock, which keeps counting.
    rtc: RtcState,
    /// What reads of the clock registers show: the registers as they were at
    /// the last latch.
    latched_rtc: LatchedRtc,
    /// The clock `rtc.base_timestamp` is a time on. A state made on the other
    /// clock is moved over to this emulator's when it loads
    /// (`Mbc3CartridgeMapper::finish_loading_save_state`). A state from before this field
    /// lacks it and keeps the value the load starts from: the power-on
    /// state, already moved over to this emulator's clock, so its clock
    /// loads as it is, as it did before.
    rtc_clock: RtcClock,
    /// Whether the cartridge type has RAM, which the game can then enable.
    #[serializable(skip)]
    has_ram: bool,
    /// Whether the cartridge type has the clock.
    #[serializable(skip)]
    has_timer: bool,
    /// Whether it is an MBC30: an 8-bit ROM bank number and eight RAM banks.
    #[serializable(skip)]
    is_mbc30: bool,
    /// Set when the clock changed, until it is passed to the frontend to save
    /// (see `Cartridge::report_save_changes`). Like the cartridge's
    /// `ram_changed`, loading a save state leaves it alone.
    #[serializable(skip)]
    rtc_changed: bool,
}

/// The ROM size above which an MBC3 is an MBC30 (2 MiB, 128 banks).
const MBC3_MAX_ROM_SIZE: usize = 0x20_0000;
/// The RAM size above which an MBC3 is an MBC30 (32 KiB, 4 banks).
const MBC3_MAX_RAM_SIZE: usize = 0x8000;

/// PPU dots in a millisecond: the clock runs at 4,194,304 Hz.
const DOTS_PER_MILLISECOND: f64 = 4_194.304;
/// The seconds register has six bits, so it can hold values up to 63.
const SECONDS_REGISTER_MAX: u8 = 63;
/// The minutes register has six bits, so it can hold values up to 63.
const MINUTES_REGISTER_MAX: u8 = 63;
/// The hours register has five bits, so it can hold values up to 31.
const HOURS_REGISTER_MAX: u8 = 31;
/// The day counter has nine bits (DL and bit 0 of DH), so it can hold values
/// up to 511.
const DAYS_REGISTER_MAX: u16 = 0x1FF;

/// Counts a clock register holding `value` on by `ticks`. A register below
/// `period` (60 for seconds and minutes, 24 for hours) counts as usual and
/// carries into the next register each time it goes past `period - 1`. One
/// holding an invalid value counts up to `register_max`, wraps to 0 without
/// a carry and counts as usual from there. Returns the new value and the
/// carries into the next register.
fn count_register(value: u8, ticks: u64, period: u8, register_max: u8) -> (u8, u64) {
    let mut ticks = ticks;
    if value >= period {
        let to_wrap = u64::from(register_max.saturating_sub(value)) + 1;
        if ticks < to_wrap {
            return (value + ticks as u8, 0);
        }
        ticks -= to_wrap;
    } else {
        ticks += u64::from(value);
    }
    let period = u64::from(period);
    ((ticks % period) as u8, ticks / period)
}

const RTC_SECONDS: u8 = 0x08;
const RTC_MINUTES: u8 = 0x09;
const RTC_HOURS: u8 = 0x0A;
/// DL: the lower 8 bits of the day counter.
const RTC_DAY_LOW: u8 = 0x0B;
/// DH: bit 0 is bit 8 of the day counter, bit 6 halts the clock and bit 7 is
/// the day carry flag.
const RTC_DAY_HIGH: u8 = 0x0C;

/// The time an MBC3 clock follows, in milliseconds: the frontend's wall
/// clock, or with [`RtcClock::EmulatedTime`] the `elapsed_dots` the emulator
/// has run since power-on.
pub(super) fn rtc_time(effects: &dyn CartridgeEffects, elapsed_dots: u64) -> f64 {
    clock_time(effects, effects.rtc_clock(), elapsed_dots)
}

/// The current time on `clock`, in milliseconds: the frontend's wall clock,
/// or the `elapsed_dots` the emulator has run since power-on.
pub(super) fn clock_time(
    effects: &dyn CartridgeEffects,
    clock: RtcClock,
    elapsed_dots: u64,
) -> f64 {
    match clock {
        RtcClock::WallClock => effects.current_time_millis(),
        RtcClock::EmulatedTime => elapsed_dots as f64 / DOTS_PER_MILLISECOND,
    }
}

impl Mbc3CartridgeMapper {
    /// The controller at power-on, for a cartridge with this type byte, ROM
    /// size and RAM size. The clock follows `rtc_clock` and goes on from
    /// `rtc` (the state the frontend saved, or a new clock at 0), which reads
    /// show until the game latches it.
    pub(super) fn new(
        cartridge_type: u8,
        rom_size: usize,
        ram_size: usize,
        rtc_clock: RtcClock,
        rtc: RtcState,
    ) -> Self {
        let rtc = rtc.within_register_widths();
        Mbc3CartridgeMapper {
            rom_bank_number: 1,
            ram_rtc_enabled: false,
            ram_bank_or_rtc_register: 0,
            latched_rtc: LatchedRtc::of(&rtc),
            rtc,
            rtc_clock,
            is_mbc30: rom_size > MBC3_MAX_ROM_SIZE || ram_size > MBC3_MAX_RAM_SIZE,
            has_ram: !lacks_ram(cartridge_type),
            has_timer: matches!(
                cartridge_type,
                CARTRIDGE_TYPE_MBC3_TIMER_RAM_BATTERY | CARTRIDGE_TYPE_MBC3_TIMER_BATTERY
            ),
            rtc_changed: false,
        }
    }

    /// Brings the clock up to date and copies its registers to the ones
    /// reads show, when the game latches it.
    fn latch_rtc(&mut self, current_time: f64) {
        if self.rtc.catch_up(current_time) {
            self.rtc_changed = true;
        }
        self.latched_rtc = LatchedRtc::of(&self.rtc);
    }

    /// Brings the clock up to date, then writes a clock register.
    ///
    /// The write replaces the register's value, like on real hardware, so the
    /// time that passed before the write is counted first. A clock that was
    /// halted until this write doesn't count the time it spent halted, and a
    /// wall clock that went back restarts from the current time. Writing the
    /// seconds register also restarts the current second.
    fn write_rtc_register(&mut self, register: u8, value: u8, current_time: f64) {
        self.rtc.catch_up(current_time);
        if self.rtc.halted {
            self.rtc.base_timestamp = current_time;
        }

        match register {
            RTC_SECONDS => {
                self.rtc.seconds = value & SECONDS_REGISTER_MAX;
                self.rtc.milliseconds = 0;
                self.rtc.base_timestamp = current_time;
            }
            RTC_MINUTES => self.rtc.minutes = value & MINUTES_REGISTER_MAX,
            RTC_HOURS => self.rtc.hours = value & HOURS_REGISTER_MAX,
            RTC_DAY_LOW => self.rtc.days = (self.rtc.days & 0x100) | u16::from(value),
            RTC_DAY_HIGH => {
                self.rtc.days = (self.rtc.days & 0xFF) | ((u16::from(value) & 0x01) << 8);
                self.rtc.halted = value & 0x40 != 0;
                self.rtc.day_carry = value & 0x80 != 0;
            }
            _ => {}
        }

        self.rtc_changed = true;
    }

    /// The clock's state, on a cartridge that has the clock.
    pub(super) fn rtc(&self) -> Option<&RtcState> {
        self.has_timer.then_some(&self.rtc)
    }

    /// Replaces the clock's state, and what reads show until the game latches
    /// it, as if the cartridge had been inserted with `state`. Does nothing
    /// on a cartridge without the clock.
    pub(super) fn set_rtc(&mut self, state: RtcState) {
        if self.has_timer {
            let state = state.within_register_widths();
            self.latched_rtc = LatchedRtc::of(&state);
            self.rtc = state;
        }
    }

    /// Finishes loading a save state: cuts the registers to the bits they
    /// have, as a game's writes and a loaded `.rtc` file are, so a damaged
    /// state can't make one hold a value the hardware can't, and moves the
    /// clock over to `clock` if it follows the other one (see
    /// [`RtcState::rebase`]), keeping what it shows. `now` gives the current
    /// time on either clock.
    pub(super) fn finish_loading_save_state(
        &mut self,
        clock: RtcClock,
        now: impl Fn(RtcClock) -> f64,
    ) {
        let bank = if self.is_mbc30 {
            self.rom_bank_number
        } else {
            self.rom_bank_number & 0x7F
        };
        self.rom_bank_number = if bank == 0 { 1 } else { bank };
        self.ram_bank_or_rtc_register &= 0x0F;
        self.rtc = std::mem::take(&mut self.rtc).within_register_widths();
        self.latched_rtc = self.latched_rtc.within_register_widths();
        if self.rtc_clock != clock {
            self.rtc.rebase(now(self.rtc_clock), now(clock));
            self.rtc_clock = clock;
        }
    }

    /// The clock's state if it changed since the last call, for the frontend
    /// to save.
    pub(super) fn take_changed_rtc(&mut self) -> Option<&RtcState> {
        std::mem::take(&mut self.rtc_changed).then_some(&self.rtc)
    }

    /// The ROM banks visible at 0x0000-0x3FFF and 0x4000-0x7FFF.
    pub(super) fn rom_banks(&self) -> [u16; 2] {
        [0, u16::from(self.rom_bank_number)]
    }

    /// What 0xA000-0xBFFF shows: a RAM bank, or a clock register 0x08-0x0F.
    pub(super) fn ram_bank_or_rtc_register(&self) -> u8 {
        self.ram_bank_or_rtc_register
    }

    /// What the selected RAM bank or clock register is.
    fn selected(&self) -> Selected {
        let index = self.ram_bank_or_rtc_register & 0x07;
        if self.has_timer && self.ram_bank_or_rtc_register & 0x08 != 0 {
            Selected::Clock(index)
        } else if !self.has_ram {
            Selected::Nothing
        } else if self.is_mbc30 {
            Selected::Ram(index)
        } else if self.has_timer && index > 3 {
            // Banks 4-7 read nothing on an MBC3 with a clock (SameBoy).
            Selected::Nothing
        } else {
            Selected::Ram(index & 0x03)
        }
    }

    /// Sets a register by writing to the ROM area. Latching the clock asks
    /// `current_time` for the time the clock follows.
    pub(super) fn write_rom(
        &mut self,
        address: u16,
        value: u8,
        current_time: impl FnOnce() -> f64,
    ) {
        match address {
            0x0000..=0x1FFF => {
                if self.has_ram || self.has_timer {
                    self.ram_rtc_enabled = (value & 0xF) == 0x0A;
                }
            }
            0x2000..=0x3FFF => {
                // The bank number has seven bits (eight on an MBC30); only
                // after masking does 0 select bank 1, so writing 0x80 selects
                // bank 1 on an MBC3 (SameBoy).
                let bank = if self.is_mbc30 { value } else { value & 0x7F };
                self.rom_bank_number = if bank == 0 { 1 } else { bank };
            }
            0x4000..=0x5FFF => self.ram_bank_or_rtc_register = value & 0x0F,
            0x6000..=0x7FFF => {
                if self.has_timer {
                    self.latch_rtc(current_time());
                }
            }
            _ => unreachable!("the address bus only routes ROM addresses here, got {address:#X}"),
        }
    }

    /// Reads the selected RAM bank or clock register at `address` (relative
    /// to 0xA000), or 0xFF while they are disabled or missing.
    pub(super) fn read_ram(&self, ram: &[u8], address: u16) -> u8 {
        if !self.ram_rtc_enabled {
            return 0xFF;
        }
        match self.selected() {
            Selected::Ram(bank) => banked_read(ram, RAM_BANK_SIZE, address, u16::from(bank)),
            Selected::Clock(index) => self.latched_rtc.register(index),
            Selected::Nothing => 0xFF,
        }
    }

    /// Writes the selected RAM bank or clock register at `address` (relative
    /// to 0xA000) if they are enabled. Writing the clock asks `current_time`
    /// for the time it follows. Returns whether a byte of RAM was stored.
    pub(super) fn write_ram(
        &mut self,
        ram: &mut [u8],
        address: u16,
        value: u8,
        current_time: impl FnOnce() -> f64,
    ) -> bool {
        if !self.ram_rtc_enabled {
            return false;
        }
        match self.selected() {
            Selected::Ram(bank) => {
                banked_write(ram, RAM_BANK_SIZE, address, u16::from(bank), value)
            }
            Selected::Clock(index) if RTC_SECONDS + index <= RTC_DAY_HIGH => {
                self.write_rtc_register(RTC_SECONDS + index, value, current_time());
                false
            }
            Selected::Clock(_) | Selected::Nothing => false,
        }
    }
}

/// What 0xA000-0xBFFF of an MBC3 shows.
enum Selected {
    /// A RAM bank.
    Ram(u8),
    /// A clock register: 0-4 for seconds, minutes, hours, DL and DH, 5-7 for
    /// nothing.
    Clock(u8),
    /// Neither: reads give 0xFF and writes are lost.
    Nothing,
}

/// The clock registers as the game reads them, copied from the clock at
/// each latch.
#[derive(Debug, Default, Clone, Copy, Serializable)]
struct LatchedRtc {
    seconds: u8,
    minutes: u8,
    hours: u8,
    /// DL: the lower 8 bits of the day counter.
    day_low: u8,
    /// DH: bit 0 is bit 8 of the day counter, bit 6 the halt flag and bit 7
    /// the day carry flag.
    day_high: u8,
}

impl LatchedRtc {
    /// The registers of `rtc` as they are now.
    fn of(rtc: &RtcState) -> Self {
        let mut day_high = (rtc.days >> 8) as u8 & 0x01;
        if rtc.halted {
            day_high |= 0x40;
        }
        if rtc.day_carry {
            day_high |= 0x80;
        }
        LatchedRtc {
            seconds: rtc.seconds,
            minutes: rtc.minutes,
            hours: rtc.hours,
            day_low: (rtc.days & 0xFF) as u8,
            day_high,
        }
    }

    /// The registers cut to the bits they have, as `LatchedRtc::of` gives
    /// them.
    fn within_register_widths(self) -> Self {
        LatchedRtc {
            seconds: self.seconds & SECONDS_REGISTER_MAX,
            minutes: self.minutes & MINUTES_REGISTER_MAX,
            hours: self.hours & HOURS_REGISTER_MAX,
            day_low: self.day_low,
            day_high: self.day_high & 0xC1,
        }
    }

    /// Clock register `index` (0x08-0x0C written to 0x4000-0x5FFF, without
    /// bit 3), or 0xFF for the ones after DH.
    fn register(self, index: u8) -> u8 {
        match index {
            0 => self.seconds,
            1 => self.minutes,
            2 => self.hours,
            3 => self.day_low,
            4 => self.day_high,
            _ => 0xFF,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address_bus::cartridge::test_utils::*;
    use crate::address_bus::cartridge::*;
    use crate::address_bus::constants::{
        CARTRIDGE_TYPE_MBC3, CARTRIDGE_TYPE_MBC3_RAM, CARTRIDGE_TYPE_MBC3_RAM_BATTERY,
        RAM_SIZE_2KB, RAM_SIZE_8KB, RAM_SIZE_32KB, RAM_SIZE_64KB, ROM_SIZE_2MB, ROM_SIZE_4MB,
        ROM_SIZE_64KB,
    };
    use crate::address_bus::effects::{CartridgeEffects, Shared, empty_cartridge_effects};
    use crate::address_bus::test_utils::*;

    /// Starts the clock at 10:10:10 and `milliseconds`, last updated at
    /// `base_timestamp`, and reports `now` as the current time.
    struct ClockEffects {
        now: f64,
        milliseconds: u16,
        base_timestamp: f64,
    }

    impl CartridgeEffects for ClockEffects {
        fn current_time_millis(&self) -> f64 {
            self.now
        }

        fn load_rtc_state(&self) -> Option<RtcState> {
            Some(RtcState {
                milliseconds: self.milliseconds,
                seconds: 10,
                minutes: 10,
                hours: 10,
                base_timestamp: self.base_timestamp,
                ..RtcState::default()
            })
        }
    }

    /// A clock at 10:10:10 whose time stands still.
    fn fake_cartridge_effects() -> Box<dyn CartridgeEffects> {
        Box::new(ClockEffects {
            now: 0.0,
            milliseconds: 0,
            base_timestamp: 0.0,
        })
    }

    fn latched_clock(effects: ClockEffects) -> Cartridge {
        let mut mapper = build_cartridge_mapper_with_effects(
            CARTRIDGE_TYPE_MBC3_TIMER_BATTERY,
            ROM_SIZE_64KB,
            0,
            Box::new(effects),
        );
        mapper.write_rom(0x0000, 0xA);
        mapper.write_rom(0x6000, 0x0);
        mapper.write_rom(0x6000, 0x1);
        mapper
    }

    fn read_rtc_register(mapper: &mut Cartridge, register: u8) -> u8 {
        mapper.write_rom(0x4000, register);
        mapper.read_ram(0x0000)
    }

    #[test]
    fn the_clock_state_round_trips_through_the_rtc_file_layout() {
        let state = RtcState {
            milliseconds: 999,
            seconds: 59,
            minutes: 30,
            hours: 23,
            days: 511,
            base_timestamp: 1_791_633_740_867.5,
            halted: true,
            day_carry: true,
        };
        let bytes = state.to_bytes();
        assert_eq!(
            bytes,
            [
                0xE7, 0x03, 59, 30, 23, 0xFF, 0x01, 0x00, 0x38, 0x44, 0x15, 0x5B, 0x12, 0x7A, 0x42,
                1, 1
            ]
        );
        assert_eq!(RtcState::from_bytes(&bytes), Some(state));
    }

    #[test]
    fn a_loaded_clock_holds_only_what_its_registers_can() {
        let damaged = RtcState {
            seconds: 0xFF,
            minutes: 0xFF,
            hours: 0xFF,
            days: 0xFFFF,
            ..RtcState::default()
        };
        let registers = |mapper: &Mbc3CartridgeMapper| {
            (0..5)
                .map(|index| mapper.latched_rtc.register(index))
                .collect::<Vec<_>>()
        };
        let mut mapper = Mbc3CartridgeMapper::new(
            CARTRIDGE_TYPE_MBC3_TIMER_BATTERY,
            0x1_0000,
            0,
            RtcClock::WallClock,
            damaged.clone(),
        );
        assert_eq!(registers(&mapper), [63, 63, 31, 0xFF, 0x01]);

        mapper.set_rtc(RtcState::default());
        mapper.set_rtc(damaged);
        assert_eq!(registers(&mapper), [63, 63, 31, 0xFF, 0x01]);
        assert_eq!(mapper.rtc.days, 511);
    }

    #[test]
    fn a_clock_state_of_another_length_is_ignored() {
        assert!(RtcState::from_bytes(&[0; RtcState::BYTES - 1]).is_none());
        assert!(RtcState::from_bytes(&[0; RtcState::BYTES + 1]).is_none());
    }

    #[test]
    fn rebasing_counts_the_time_on_the_old_clock_and_goes_on_from_the_new_one() {
        let mut state = RtcState {
            seconds: 10,
            base_timestamp: 1_000.25,
            ..RtcState::default()
        };
        state.rebase(3_500.5, 20.0);
        assert_eq!((state.seconds, state.milliseconds), (12, 500));
        assert_eq!(state.base_timestamp, 20.0 - 0.25);
    }

    #[test]
    fn a_damaged_timestamp_starts_the_clock_again_from_now() {
        for damaged in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut state = RtcState {
                seconds: 10,
                base_timestamp: damaged,
                ..RtcState::default()
            };
            assert!(!state.catch_up(5_000.0));
            assert_eq!(state.seconds, 10);
            assert_eq!(state.base_timestamp, 5_000.0);

            assert!(state.catch_up(7_000.0));
            assert_eq!(state.seconds, 12);
        }
    }

    #[test]
    fn an_invalid_register_wraps_to_0_and_keeps_the_time_left_over() {
        let mut state = RtcState {
            seconds: 62,
            minutes: 5,
            ..RtcState::default()
        };
        state.advance(10_000.0);
        assert_eq!((state.seconds, state.minutes), (8, 5));

        state.advance(60_000.0);
        assert_eq!((state.seconds, state.minutes), (8, 6));
    }

    #[test]
    fn invalid_registers_carry_only_after_they_wrap() {
        let mut state = RtcState {
            seconds: 30,
            minutes: 62,
            hours: 30,
            days: 511,
            ..RtcState::default()
        };
        // 3 minutes: 62 and 63, wrap to 0 without a carry, then 1.
        state.advance(180_000.0);
        assert_eq!((state.seconds, state.minutes, state.hours), (30, 1, 30));

        // 59 minutes more: the hours register counts once, from 30 to 31.
        state.advance(59.0 * 60_000.0);
        assert_eq!((state.minutes, state.hours, state.days), (0, 31, 511));

        // 25 hours more: 31 wraps to 0 without a carry, then 24 hours make
        // a day, which overflows the day counter.
        state.advance(25.0 * 3_600_000.0);
        assert_eq!((state.hours, state.days, state.day_carry), (0, 0, true));
    }

    #[test]
    fn a_clock_whose_time_went_back_counts_on_from_now() {
        let mut state = RtcState {
            seconds: 10,
            base_timestamp: 1_000_000.0,
            ..RtcState::default()
        };
        assert!(state.catch_up(5_000.0));
        assert_eq!(state.seconds, 10);
        assert_eq!(state.base_timestamp, 5_000.0);

        assert!(state.catch_up(8_000.0));
        assert_eq!(state.seconds, 13);
    }

    #[test]
    fn rebasing_a_halted_clock_counts_nothing() {
        let mut state = RtcState {
            seconds: 10,
            halted: true,
            base_timestamp: 1_000.0,
            ..RtcState::default()
        };
        state.rebase(90_000.0, 20.0);
        assert_eq!(state.seconds, 10);
        assert_eq!(state.base_timestamp, 20.0);
    }

    #[test]
    fn latching_advances_the_clock_by_the_elapsed_time() {
        let mut mapper = latched_clock(ClockEffects {
            now: 3_723_000.0,
            milliseconds: 0,
            base_timestamp: 0.0,
        });

        assert_eq!(read_rtc_register(&mut mapper, RTC_SECONDS), 13);
        assert_eq!(read_rtc_register(&mut mapper, RTC_MINUTES), 12);
        assert_eq!(read_rtc_register(&mut mapper, RTC_HOURS), 11);
    }

    /// A wall clock at `now` with no saved clock state.
    struct UnsavedClock {
        now: Shared<f64>,
    }

    impl CartridgeEffects for UnsavedClock {
        fn current_time_millis(&self) -> f64 {
            self.now.get()
        }
    }

    #[test]
    fn a_new_clock_starts_at_0_and_counts_from_when_the_cartridge_is_inserted() {
        let now = Shared::new(1_000_000_000_000.0);
        let mut mapper = build_cartridge_mapper_with_effects(
            CARTRIDGE_TYPE_MBC3_TIMER_BATTERY,
            ROM_SIZE_64KB,
            0,
            Box::new(UnsavedClock { now: now.clone() }),
        );
        now.set(1_000_000_000_000.0 + 61_000.0);
        mapper.write_rom(0x0000, 0xA);
        mapper.write_rom(0x6000, 0x0);
        mapper.write_rom(0x6000, 0x1);

        assert_eq!(read_rtc_register(&mut mapper, RTC_SECONDS), 1);
        assert_eq!(read_rtc_register(&mut mapper, RTC_MINUTES), 1);
        assert_eq!(read_rtc_register(&mut mapper, RTC_HOURS), 0);
        assert_eq!(read_rtc_register(&mut mapper, RTC_DAY_LOW), 0);
        assert_eq!(read_rtc_register(&mut mapper, RTC_DAY_HIGH), 0);
    }

    /// A clock that starts at 10:10:10 at time 0, whose current time the
    /// test can move forward, and that counts how often it is saved.
    struct AdjustableClock {
        now: Shared<f64>,
        saves: Shared<usize>,
    }

    impl CartridgeEffects for AdjustableClock {
        fn current_time_millis(&self) -> f64 {
            self.now.get()
        }

        fn load_rtc_state(&self) -> Option<RtcState> {
            Some(RtcState {
                seconds: 10,
                minutes: 10,
                hours: 10,
                ..RtcState::default()
            })
        }

        fn save_rtc_state(&mut self, _state: &RtcState) {
            self.saves.set(self.saves.get() + 1);
        }
    }

    /// A cartridge with an `AdjustableClock` and its RAM and clock enabled,
    /// with the clock's current time and save count.
    fn adjustable_clock_counting_saves() -> (Cartridge, Shared<f64>, Shared<usize>) {
        let now = Shared::new(0.0);
        let saves = Shared::new(0);
        let mut mapper = build_cartridge_mapper_with_effects(
            CARTRIDGE_TYPE_MBC3_TIMER_BATTERY,
            ROM_SIZE_64KB,
            0,
            Box::new(AdjustableClock {
                now: now.clone(),
                saves: saves.clone(),
            }),
        );
        mapper.write_rom(0x0000, 0xA);
        (mapper, now, saves)
    }

    fn adjustable_clock() -> (Cartridge, Shared<f64>) {
        let (mapper, now, _) = adjustable_clock_counting_saves();
        (mapper, now)
    }

    fn latch(mapper: &mut Cartridge) {
        mapper.write_rom(0x6000, 0x0);
        mapper.write_rom(0x6000, 0x1);
    }

    fn write_rtc_register(mapper: &mut Cartridge, register: u8, value: u8) {
        mapper.write_rom(0x4000, register);
        mapper.write_ram(0x0000, value);
    }

    #[test]
    fn the_clock_is_saved_once_per_report_however_often_it_changes() {
        let (mut mapper, now, saves) = adjustable_clock_counting_saves();
        for step in 1..=100 {
            now.set(f64::from(step) * 1_000.0);
            latch(&mut mapper);
        }
        write_rtc_register(&mut mapper, RTC_MINUTES, 5);
        assert_eq!(saves.get(), 0);

        mapper.report_save_changes();
        assert_eq!(saves.get(), 1);

        mapper.report_save_changes();
        assert_eq!(saves.get(), 1);
    }

    #[test]
    fn latching_a_clock_that_has_not_moved_does_not_save_it() {
        let (mut mapper, _, saves) = adjustable_clock_counting_saves();
        latch(&mut mapper);
        latch(&mut mapper);

        mapper.report_save_changes();
        assert_eq!(saves.get(), 0);
    }

    #[test]
    fn a_wall_clock_that_went_back_is_saved_and_counts_on_from_then() {
        let (mut mapper, now, saves) = adjustable_clock_counting_saves();
        now.set(-60_000.0);
        latch(&mut mapper);
        mapper.report_save_changes();
        assert_eq!(saves.get(), 1);

        now.set(-55_000.0);
        latch(&mut mapper);
        assert_eq!(read_rtc_register(&mut mapper, RTC_SECONDS), 15);
    }

    #[test]
    fn latching_more_often_than_every_millisecond_keeps_counting() {
        let (mut mapper, now) = adjustable_clock();
        for step in 1..=4_000 {
            now.set(f64::from(step) * 0.25);
            latch(&mut mapper);
        }

        assert_eq!(read_rtc_register(&mut mapper, RTC_SECONDS), 11);
    }

    #[test]
    fn writing_a_register_other_than_seconds_keeps_the_current_second() {
        let (mut mapper, now) = adjustable_clock();
        now.set(400.5);
        write_rtc_register(&mut mapper, RTC_MINUTES, 20);

        now.set(1_000.0);
        latch(&mut mapper);

        assert_eq!(read_rtc_register(&mut mapper, RTC_SECONDS), 11);
    }

    #[test]
    fn writing_a_register_replaces_its_value_instead_of_adding_elapsed_time() {
        let (mut mapper, now) = adjustable_clock();
        now.set(5_000.0);

        write_rtc_register(&mut mapper, RTC_SECONDS, 0);
        latch(&mut mapper);

        assert_eq!(read_rtc_register(&mut mapper, RTC_SECONDS), 0);
    }

    #[test]
    fn writing_a_register_keeps_the_time_that_passed_before_the_write() {
        let (mut mapper, now) = adjustable_clock();
        now.set(120_000.0);

        write_rtc_register(&mut mapper, RTC_SECONDS, 30);
        latch(&mut mapper);

        assert_eq!(read_rtc_register(&mut mapper, RTC_MINUTES), 12);
        assert_eq!(read_rtc_register(&mut mapper, RTC_SECONDS), 30);
    }

    #[test]
    fn writing_seconds_restarts_the_current_second() {
        let (mut mapper, now) = adjustable_clock();
        now.set(900.0);
        write_rtc_register(&mut mapper, RTC_SECONDS, 0);

        now.set(1_800.0);
        latch(&mut mapper);

        assert_eq!(read_rtc_register(&mut mapper, RTC_SECONDS), 0);
    }

    #[test]
    fn a_halted_clock_does_not_count_the_time_it_was_halted() {
        let (mut mapper, now) = adjustable_clock();
        write_rtc_register(&mut mapper, RTC_DAY_HIGH, 0x40);

        now.set(3_600_000.0);
        write_rtc_register(&mut mapper, RTC_DAY_HIGH, 0x00);
        now.set(3_605_000.0);
        latch(&mut mapper);

        assert_eq!(read_rtc_register(&mut mapper, RTC_HOURS), 10);
        assert_eq!(read_rtc_register(&mut mapper, RTC_MINUTES), 10);
        assert_eq!(read_rtc_register(&mut mapper, RTC_SECONDS), 15);
    }

    #[test]
    fn halting_the_clock_keeps_the_time_that_passed_before_it_stopped() {
        let (mut mapper, now) = adjustable_clock();
        now.set(5_000.0);
        write_rtc_register(&mut mapper, RTC_DAY_HIGH, 0x40);

        now.set(60_000.0);
        latch(&mut mapper);

        assert_eq!(read_rtc_register(&mut mapper, RTC_SECONDS), 15);
    }

    #[test]
    fn latching_after_a_huge_elapsed_time_does_not_panic() {
        let mut mapper = latched_clock(ClockEffects {
            now: 0.0,
            milliseconds: 999,
            base_timestamp: -1e300,
        });

        assert!(read_rtc_register(&mut mapper, RTC_SECONDS) < 60);
    }

    #[test]
    fn reads_from_rom_bank_0() {
        let mut rom = build_rom(CARTRIDGE_TYPE_MBC3, ROM_SIZE_64KB, RAM_SIZE_2KB);
        rom[0] = 0xB1;
        rom[1] = 0xD2;
        let mapper = load_cartridge(rom, empty_cartridge_effects()).unwrap();

        let byte = mapper.read_rom(0x1);
        assert_eq!(byte, 0xD2);
    }

    #[test]
    fn reads_from_rom_bank_2() {
        let mut rom = build_rom(CARTRIDGE_TYPE_MBC3, ROM_SIZE_64KB, RAM_SIZE_2KB);
        rom[0x8000] = 0xB1;
        rom[0x8001] = 0xD2;
        let mut mapper = load_cartridge(rom, empty_cartridge_effects()).unwrap();

        mapper.write_rom(0x2000, 2);

        let byte = mapper.read_rom(0x4001);
        assert_eq!(byte, 0xD2);
    }

    #[test]
    fn reads_from_ram_bank_1() {
        let mut mapper =
            build_cartridge_mapper(CARTRIDGE_TYPE_MBC3_RAM, ROM_SIZE_64KB, RAM_SIZE_32KB);

        let mut ram = vec![0; 0x8000];
        ram[0x2001] = 0xDD;
        mapper.set_ram(ram);

        // Enable RAM
        mapper.write_rom(0x0000, 0xA);

        // Select RAM bank 1
        mapper.write_rom(0x4000, 0x1);

        let byte = mapper.read_ram(0x0001);
        assert_eq!(byte, 0xDD);
    }

    #[test]
    fn does_not_read_from_ram_if_not_supported() {
        let mut mapper = build_cartridge_mapper(
            CARTRIDGE_TYPE_MBC3_TIMER_BATTERY,
            ROM_SIZE_64KB,
            RAM_SIZE_32KB,
        );

        let mut ram = vec![0; 0x8000];
        ram[0x2001] = 0xDD;
        mapper.set_ram(ram);

        // Enable RAM
        mapper.write_rom(0x0000, 0xA);

        // Select RAM bank 1
        mapper.write_rom(0x4000, 0x1);

        let byte = mapper.read_ram(0x0001);
        assert_eq!(byte, 0xFF);
    }

    #[test]
    fn writes_to_ram_bank_1() {
        let mut mapper =
            build_cartridge_mapper(CARTRIDGE_TYPE_MBC3_RAM, ROM_SIZE_64KB, RAM_SIZE_32KB);

        // Enable RAM
        mapper.write_rom(0x0000, 0xA);

        // Select RAM bank 1
        mapper.write_rom(0x4000, 0x1);

        mapper.write_ram(0x0001, 0xDD);

        let byte = mapper.read_ram(0x001);
        assert_eq!(byte, 0xDD);
    }

    #[test]
    fn does_not_write_to_ram_if_not_supported() {
        let mut mapper = build_cartridge_mapper(
            CARTRIDGE_TYPE_MBC3_TIMER_BATTERY,
            ROM_SIZE_64KB,
            RAM_SIZE_32KB,
        );

        // Enable RAM
        mapper.write_rom(0x0000, 0xA);

        // Select RAM bank 1
        mapper.write_rom(0x4000, 0x1);

        mapper.write_ram(0x0001, 0xDD);

        let byte = mapper.read_ram(0x001);
        assert_eq!(byte, 0xFF);
    }

    #[test]
    fn changes_rom_bank_number_to_bank_1_when_writing_0() {
        let mut rom = build_rom(CARTRIDGE_TYPE_MBC3, ROM_SIZE_64KB, RAM_SIZE_2KB);
        rom[0x4000] = 0xB1;
        rom[0x4001] = 0xD2;
        let mut mapper = load_cartridge(rom, empty_cartridge_effects()).unwrap();

        mapper.write_rom(0x2000, 2);

        mapper.write_rom(0x2000, 0);

        let byte = mapper.read_rom(0x4001);
        assert_eq!(byte, 0xD2);
    }

    #[test]
    fn selects_bank_1_when_the_seven_bank_bits_are_0() {
        let mut rom = build_rom(CARTRIDGE_TYPE_MBC3, ROM_SIZE_64KB, RAM_SIZE_2KB);
        rom[0x4001] = 0xD2;
        let mut mapper = load_cartridge(rom, empty_cartridge_effects()).unwrap();

        mapper.write_rom(0x2000, 2);
        mapper.write_rom(0x2000, 0x80);

        assert_eq!(mapper.rom_banks(), [0, 1]);
        assert_eq!(mapper.read_rom(0x4001), 0xD2);
    }

    #[test]
    fn reads_from_minutes_rtc_register() {
        let mut mapper = build_cartridge_mapper_with_effects(
            CARTRIDGE_TYPE_MBC3_TIMER_RAM_BATTERY,
            ROM_SIZE_64KB,
            RAM_SIZE_2KB,
            fake_cartridge_effects(),
        );

        mapper.write_rom(0x0000, 0xA);

        mapper.write_rom(0x4000, 0x9);

        // Latch RTC
        mapper.write_rom(0x6000, 0x0);
        mapper.write_rom(0x6000, 0x1);

        let byte = mapper.read_ram(0x0000);
        assert_eq!(byte, 0x0A);
    }

    #[test]
    fn a_clock_register_without_a_clock_selects_a_ram_bank() {
        let mut mapper = build_cartridge_mapper_with_effects(
            CARTRIDGE_TYPE_MBC3_RAM_BATTERY,
            ROM_SIZE_64KB,
            RAM_SIZE_32KB,
            fake_cartridge_effects(),
        );
        mapper.write_rom(0x0000, 0xA);
        mapper.write_rom(0x4000, 0x1);
        mapper.write_ram(0x0000, 0x5A);

        mapper.write_rom(0x4000, 0x9);
        assert_eq!(mapper.read_ram(0x0000), 0x5A);
        mapper.write_ram(0x0000, 0x2);
        mapper.write_rom(0x4000, 0x1);
        assert_eq!(mapper.read_ram(0x0000), 0x2);
    }

    #[test]
    fn a_written_clock_register_reads_back_after_the_next_latch() {
        let mut mapper = build_cartridge_mapper_with_effects(
            CARTRIDGE_TYPE_MBC3_TIMER_RAM_BATTERY,
            ROM_SIZE_64KB,
            RAM_SIZE_2KB,
            fake_cartridge_effects(),
        );
        mapper.write_rom(0x0000, 0xA);
        mapper.write_rom(0x6000, 0x1);
        assert_eq!(read_rtc_register(&mut mapper, RTC_MINUTES), 10);

        mapper.write_ram(0x0000, 0x2);
        assert_eq!(mapper.read_ram(0x0000), 10);
        mapper.write_rom(0x6000, 0x1);
        assert_eq!(mapper.read_ram(0x0000), 0x2);
    }

    #[test]
    fn any_write_to_the_latch_register_latches_the_clock() {
        for value in [0x00, 0x01, 0x5A] {
            let mut mapper = build_cartridge_mapper_with_effects(
                CARTRIDGE_TYPE_MBC3_TIMER_BATTERY,
                ROM_SIZE_64KB,
                0,
                fake_cartridge_effects(),
            );
            mapper.write_rom(0x0000, 0xA);
            mapper.write_rom(0x4000, RTC_HOURS);
            mapper.write_ram(0x0000, 0x7);

            mapper.write_rom(0x6000, value);
            assert_eq!(mapper.read_ram(0x0000), 0x7, "{value:#X}");
        }
    }

    #[test]
    fn the_bank_register_keeps_its_four_lower_bits() {
        let mut mapper = build_cartridge_mapper_with_effects(
            CARTRIDGE_TYPE_MBC3_TIMER_RAM_BATTERY,
            ROM_SIZE_64KB,
            RAM_SIZE_32KB,
            fake_cartridge_effects(),
        );
        mapper.write_rom(0x0000, 0xA);
        for bank in 0..4 {
            mapper.write_rom(0x4000, bank);
            mapper.write_ram(0x0000, bank + 0x10);
        }
        mapper.write_rom(0x6000, 0x1);

        let expected = [
            0x10, 0x11, 0x12, 0x13, 0xFF, 0xFF, 0xFF, 0xFF, 10, 10, 10, 0, 0, 0xFF, 0xFF, 0xFF,
        ];
        for high_bits in [0x00, 0x10, 0xF0] {
            for (low_bits, expected) in expected.iter().enumerate() {
                mapper.write_rom(0x4000, high_bits | low_bits as u8);
                assert_eq!(
                    mapper.read_ram(0x0000),
                    *expected,
                    "bank {:#X}",
                    high_bits | low_bits as u8
                );
            }
        }
    }

    /// Cartridge effects that count how often the clock is saved.
    struct RtcSaveCounter(Shared<usize>);

    impl CartridgeEffects for RtcSaveCounter {
        fn current_time_millis(&self) -> f64 {
            0.0
        }

        fn save_rtc_state(&mut self, _state: &RtcState) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn writes_to_the_missing_clock_registers_change_nothing() {
        let saves = Shared::new(0);
        let mut mapper = build_cartridge_mapper_with_effects(
            CARTRIDGE_TYPE_MBC3_TIMER_BATTERY,
            ROM_SIZE_64KB,
            0,
            Box::new(RtcSaveCounter(saves.clone())),
        );
        mapper.write_rom(0x0000, 0xA);
        for register in 0x0D..=0x0F {
            mapper.write_rom(0x4000, register);
            mapper.write_ram(0x0000, 0x00);
            assert_eq!(mapper.read_ram(0x0000), 0xFF);
        }
        mapper.report_save_changes();
        assert_eq!(saves.get(), 0);

        mapper.write_rom(0x4000, RTC_SECONDS);
        mapper.write_ram(0x0000, 0x00);
        mapper.report_save_changes();
        assert_eq!(saves.get(), 1);
    }

    #[test]
    fn ram_banks_4_to_7_wrap_around_without_a_clock() {
        let mut mapper = build_cartridge_mapper(
            CARTRIDGE_TYPE_MBC3_RAM_BATTERY,
            ROM_SIZE_64KB,
            RAM_SIZE_32KB,
        );
        mapper.write_rom(0x0000, 0xA);
        mapper.write_rom(0x4000, 0x1);
        mapper.write_ram(0x0000, 0x5A);

        mapper.write_rom(0x4000, 0x5);
        assert_eq!(mapper.read_ram(0x0000), 0x5A);
    }

    #[test]
    fn an_mbc3_with_more_than_2_mib_of_rom_uses_all_eight_rom_bank_bits() {
        let mut mbc3 = build_cartridge_mapper(CARTRIDGE_TYPE_MBC3, ROM_SIZE_2MB, 0);
        mbc3.write_rom(0x2000, 0x85);
        assert_eq!(mbc3.rom_banks(), [0, 0x05]);

        let mut mbc30 = build_cartridge_mapper(CARTRIDGE_TYPE_MBC3, ROM_SIZE_4MB, 0);
        mbc30.write_rom(0x2000, 0x85);
        assert_eq!(mbc30.rom_banks(), [0, 0x85]);
        mbc30.write_rom(0x2000, 0x00);
        assert_eq!(mbc30.rom_banks(), [0, 0x01]);
    }

    #[test]
    fn an_mbc3_with_64_kib_of_ram_has_eight_ram_banks() {
        let mut mbc30 = build_cartridge_mapper_with_effects(
            CARTRIDGE_TYPE_MBC3_TIMER_RAM_BATTERY,
            ROM_SIZE_64KB,
            RAM_SIZE_64KB,
            fake_cartridge_effects(),
        );
        mbc30.write_rom(0x0000, 0xA);
        for bank in 0..8 {
            mbc30.write_rom(0x4000, bank);
            mbc30.write_ram(0x0000, bank + 0x10);
        }
        for bank in 0..8 {
            mbc30.write_rom(0x4000, bank);
            assert_eq!(mbc30.read_ram(0x0000), bank + 0x10);
        }

        let mut mbc3 =
            build_cartridge_mapper(CARTRIDGE_TYPE_MBC3_RAM_BATTERY, ROM_SIZE_64KB, RAM_SIZE_8KB);
        mbc3.write_rom(0x0000, 0xA);
        mbc3.write_rom(0x4000, 0x7);
        mbc3.write_ram(0x0000, 0x33);
        mbc3.write_rom(0x4000, 0x3);
        assert_eq!(mbc3.read_ram(0x0000), 0x33);
    }

    #[test]
    fn battery_backed_ram_is_saved_once_per_report_while_enabled() {
        assert_eq!(
            ram_saves_after_writing(CARTRIDGE_TYPE_MBC3_RAM_BATTERY, 0x0A),
            1
        );
        assert_eq!(
            ram_saves_after_writing(CARTRIDGE_TYPE_MBC3_TIMER_RAM_BATTERY, 0x0A),
            1
        );
        assert_eq!(
            ram_saves_after_writing(CARTRIDGE_TYPE_MBC3_RAM_BATTERY, 0x00),
            0
        );
        assert_eq!(ram_saves_after_writing(CARTRIDGE_TYPE_MBC3_RAM, 0x0A), 0);
        assert_eq!(
            ram_saves_after_writing(CARTRIDGE_TYPE_MBC3_TIMER_BATTERY, 0x0A),
            0
        );
    }
}
