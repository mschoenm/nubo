//! The MBC3 memory bank controller, which can also have a real-time clock
//! (RTC) that keeps counting while the console is off, powered by the
//! cartridge battery.
//!
//! The clock has five registers: seconds, minutes, hours, and a 9-bit day
//! counter split over DL (its lower 8 bits) and DH (its top bit, plus a halt
//! flag and a day carry flag). A game selects a register by writing 0x08-0x0C
//! where it would otherwise select a RAM bank, then reads or writes it at
//! 0xA000-0xBFFF. Writing 0 and then 1 to 0x6000-0x7FFF latches the clock,
//! which is when Nubo brings the registers up to date with the
//! frontend's clock (or, with [`RtcClock::EmulatedTime`], with the time the
//! emulator has run).

use crate::address_bus::bank_utils::{banked_read, banked_write};
use crate::address_bus::constants::{
    CART_TYPE_MBC3_RAM, CART_TYPE_MBC3_RAM_BATTERY, CART_TYPE_MBC3_TIMER_BATTERY,
    CART_TYPE_MBC3_TIMER_RAM_BATTERY, RAM_BANK_SIZE,
};
use crate::address_bus::effects::{CartridgeEffects, RtcClock};
use serializable_derive::Serializable;

/// The state of an MBC3 real-time clock. Frontends store it next to the
/// game's save so the clock keeps counting while the emulator is closed.
#[derive(Debug, Default, Serializable)]
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
    /// The time from [`CartridgeEffects::current_time_millis`], in
    /// milliseconds, that the registers above were last brought up to date
    /// with.
    ///
    /// [`CartridgeEffects::current_time_millis`]: crate::CartridgeEffects::current_time_millis
    pub base_timestamp: f64,
    /// Whether the clock is stopped (bit 6 of DH).
    pub halted: bool,
    /// Whether the day counter went past 511 (bit 7 of DH). It stays set until
    /// the game clears it.
    pub day_carry: bool,
}

/// The registers and clock of an MBC3, with or without RAM and a clock.
#[derive(Debug, Serializable)]
pub(super) struct Mbc3CartridgeMapper {
    /// The ROM bank visible at 0x4000-0x7FFF.
    rom_bank_number: u8,
    /// Whether the game may access the RAM and the clock registers.
    ram_rtc_enabled: bool,
    /// What 0xA000-0xBFFF shows: RAM bank 0-3, or clock register 0x08-0x0C.
    ram_bank_or_rtc_register: u8,
    rtc: RtcState,
    /// The last value written to 0x6000-0x7FFF. Writing 1 after 0 latches the
    /// clock.
    last_latch_write: u8,
    /// Whether the cartridge type has RAM, which the game can then enable.
    #[serializable(skip)]
    has_ram: bool,
    /// Whether the cartridge type has the clock.
    #[serializable(skip)]
    has_timer: bool,
    /// Set when the clock changed, until it is passed to the frontend to save
    /// (see `Cartridge::report_save_changes`). Like the cartridge's
    /// `ram_changed`, loading a save state leaves it alone.
    #[serializable(skip)]
    rtc_changed: bool,
}

/// PPU dots in a millisecond: the clock runs at 4,194,304 Hz.
const DOTS_PER_MILLISECOND: f64 = 4_194.304;
/// The seconds register has six bits, so it can hold values up to 63.
const SECONDS_REGISTER_MAX: u8 = 63;
/// The minutes register has six bits, so it can hold values up to 63.
const MINUTES_REGISTER_MAX: u8 = 63;
/// The hours register has five bits, so it can hold values up to 31.
const HOURS_REGISTER_MAX: u8 = 31;

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
    match effects.rtc_clock() {
        RtcClock::WallClock => effects.current_time_millis(),
        RtcClock::EmulatedTime => elapsed_dots as f64 / DOTS_PER_MILLISECOND,
    }
}

impl Mbc3CartridgeMapper {
    /// The controller at power-on, for a cartridge with this type byte. The
    /// clock goes on from `saved_rtc`, the state the frontend saved, if any.
    pub(super) fn new(cartridge_type: u8, saved_rtc: Option<RtcState>) -> Self {
        Mbc3CartridgeMapper {
            rom_bank_number: 1,
            ram_rtc_enabled: false,
            ram_bank_or_rtc_register: 0,
            rtc: saved_rtc.unwrap_or_default(),
            last_latch_write: 0xFF,
            has_ram: matches!(
                cartridge_type,
                CART_TYPE_MBC3_RAM | CART_TYPE_MBC3_RAM_BATTERY | CART_TYPE_MBC3_TIMER_RAM_BATTERY
            ),
            has_timer: matches!(
                cartridge_type,
                CART_TYPE_MBC3_TIMER_RAM_BATTERY | CART_TYPE_MBC3_TIMER_BATTERY
            ),
            rtc_changed: false,
        }
    }

    /// Moves the clock forward by `elapsed_ms` milliseconds. A register that
    /// holds an invalid value (such as 61 seconds) doesn't carry into the next
    /// one; it keeps counting until it overflows its bits and wraps to 0.
    fn advance_rtc(&mut self, elapsed_ms: f64) {
        let mut new_milliseconds =
            u64::from(self.rtc.milliseconds).saturating_add(elapsed_ms as u64);
        let mut new_seconds = u64::from(self.rtc.seconds);
        let mut new_minutes = u64::from(self.rtc.minutes);
        let mut new_hours = u64::from(self.rtc.hours);
        let mut new_days = u64::from(self.rtc.days);

        let mut invalid_rollover = false;

        new_seconds += new_milliseconds / 1000;
        new_milliseconds %= 1000;

        if self.rtc.seconds < 60 {
            new_minutes += new_seconds / 60;
            new_seconds %= 60;
        } else if new_seconds > u64::from(SECONDS_REGISTER_MAX) {
            invalid_rollover = true;
            new_seconds = 0;
        }

        if self.rtc.minutes < 60 && !invalid_rollover {
            new_hours += new_minutes / 60;
            new_minutes %= 60;
        } else if new_minutes > u64::from(MINUTES_REGISTER_MAX) {
            invalid_rollover = true;
            new_minutes = 0;
        }

        if self.rtc.hours < 24 && !invalid_rollover {
            new_days += new_hours / 24;
            new_hours %= 24;
        } else if new_hours > u64::from(HOURS_REGISTER_MAX) {
            invalid_rollover = true;
            new_hours = 0;
        }

        if new_days >= 512 && !invalid_rollover {
            new_days %= 512;
            self.rtc.day_carry = true;
        }

        self.rtc.milliseconds = new_milliseconds as u16;
        self.rtc.seconds = new_seconds as u8;
        self.rtc.minutes = new_minutes as u8;
        self.rtc.hours = new_hours as u8;
        self.rtc.days = new_days as u16;
    }

    /// Counts the whole milliseconds that passed since the clock was last
    /// brought up to date. The fraction of a millisecond left over stays in
    /// `base_timestamp`, so a game that latches the clock very often doesn't
    /// lose it. A halted clock doesn't count. Returns whether the clock moved.
    fn catch_up(&mut self, current_time: f64) -> bool {
        let elapsed_ms = (current_time - self.rtc.base_timestamp).floor();
        if self.rtc.halted || elapsed_ms < 1.0 {
            return false;
        }
        self.advance_rtc(elapsed_ms);
        self.rtc.base_timestamp += elapsed_ms;
        true
    }

    /// Brings the clock registers up to date when the game latches them.
    fn latch_rtc(&mut self, current_time: f64) {
        if self.catch_up(current_time) {
            self.rtc_changed = true;
        }
    }

    fn read_rtc_register(&self, register: u8) -> u8 {
        match register {
            RTC_SECONDS => self.rtc.seconds,
            RTC_MINUTES => self.rtc.minutes,
            RTC_HOURS => self.rtc.hours,
            RTC_DAY_LOW => (self.rtc.days & 0xFF) as u8,
            RTC_DAY_HIGH => {
                let mut value = (self.rtc.days >> 8) as u8;
                if self.rtc.halted {
                    value |= 0x40;
                }
                if self.rtc.day_carry {
                    value |= 0x80;
                }
                value
            }
            _ => 0xFF,
        }
    }

    /// Brings the clock up to date, then writes a clock register.
    ///
    /// The write replaces the register's value, like on real hardware, so the
    /// time that passed before the write is counted first. A clock that was
    /// halted until this write doesn't count the time it spent halted, and a
    /// wall clock that went back restarts from the current time. Writing the
    /// seconds register also restarts the current second.
    fn write_rtc_register(&mut self, register: u8, value: u8, current_time: f64) {
        self.catch_up(current_time);
        if self.rtc.halted || current_time < self.rtc.base_timestamp {
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

    /// The clock's state if it changed since the last call, for the frontend
    /// to save.
    pub(super) fn take_changed_rtc(&mut self) -> Option<&RtcState> {
        std::mem::take(&mut self.rtc_changed).then_some(&self.rtc)
    }

    /// The ROM banks visible at 0x0000-0x3FFF and 0x4000-0x7FFF.
    pub(super) fn rom_banks(&self) -> [u16; 2] {
        [0, u16::from(self.rom_bank_number)]
    }

    /// What 0xA000-0xBFFF shows: RAM bank 0-3, or clock register 0x08-0x0C.
    pub(super) fn ram_bank_or_rtc_register(&self) -> u8 {
        self.ram_bank_or_rtc_register
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
                // The bank number has seven bits; only after masking does 0
                // select bank 1, so writing 0x80 selects bank 1 (SameBoy).
                let bank = value & 0x7F;
                self.rom_bank_number = if bank == 0 { 1 } else { bank };
            }
            0x4000..=0x5FFF => {
                if matches!(value, 0x00..=0x03 | RTC_SECONDS..=RTC_DAY_HIGH) {
                    self.ram_bank_or_rtc_register = value;
                }
            }
            0x6000..=0x7FFF => {
                if self.has_timer {
                    if self.last_latch_write == 0x00 && value == 0x01 {
                        self.latch_rtc(current_time());
                    }
                    self.last_latch_write = value;
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
        match self.ram_bank_or_rtc_register {
            bank @ 0x00..=0x03 if self.has_ram => {
                banked_read(ram, RAM_BANK_SIZE, address, u16::from(bank))
            }
            register @ RTC_SECONDS..=RTC_DAY_HIGH if self.has_timer => {
                self.read_rtc_register(register)
            }
            _ => 0xFF,
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
        match self.ram_bank_or_rtc_register {
            bank @ 0x00..=0x03 if self.has_ram => {
                banked_write(ram, RAM_BANK_SIZE, address, u16::from(bank), value)
            }
            register @ RTC_SECONDS..=RTC_DAY_HIGH if self.has_timer => {
                self.write_rtc_register(register, value, current_time());
                false
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address_bus::cartridge::test_utils::*;
    use crate::address_bus::cartridge::*;
    use crate::address_bus::constants::{
        CART_TYPE_MBC3, RAM_SIZE_2KB, RAM_SIZE_32KB, ROM_SIZE_64KB,
    };
    use crate::address_bus::effects::{CartridgeEffects, empty_cartridge_effects};
    use crate::address_bus::test_utils::*;
    use std::cell::Cell;
    use std::rc::Rc;

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
            CART_TYPE_MBC3_TIMER_BATTERY,
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

    /// A clock that starts at 10:10:10 at time 0, whose current time the
    /// test can move forward, and that counts how often it is saved.
    struct AdjustableClock {
        now: Rc<Cell<f64>>,
        saves: Rc<Cell<usize>>,
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
    fn adjustable_clock_counting_saves() -> (Cartridge, Rc<Cell<f64>>, Rc<Cell<usize>>) {
        let now = Rc::new(Cell::new(0.0));
        let saves = Rc::new(Cell::new(0));
        let mut mapper = build_cartridge_mapper_with_effects(
            CART_TYPE_MBC3_TIMER_BATTERY,
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

    fn adjustable_clock() -> (Cartridge, Rc<Cell<f64>>) {
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
        let mut rom = build_rom(CART_TYPE_MBC3, ROM_SIZE_64KB, RAM_SIZE_2KB);
        rom[0] = 0xB1;
        rom[1] = 0xD2;
        let mapper = load_cartridge(rom, empty_cartridge_effects()).unwrap();

        let byte = mapper.read_rom(0x1);
        assert_eq!(byte, 0xD2);
    }

    #[test]
    fn reads_from_rom_bank_2() {
        let mut rom = build_rom(CART_TYPE_MBC3, ROM_SIZE_64KB, RAM_SIZE_2KB);
        rom[0x8000] = 0xB1;
        rom[0x8001] = 0xD2;
        let mut mapper = load_cartridge(rom, empty_cartridge_effects()).unwrap();

        mapper.write_rom(0x2000, 2);

        let byte = mapper.read_rom(0x4001);
        assert_eq!(byte, 0xD2);
    }

    #[test]
    fn reads_from_ram_bank_1() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC3_RAM, ROM_SIZE_64KB, RAM_SIZE_32KB);

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
        let mut mapper =
            build_cartridge_mapper(CART_TYPE_MBC3_TIMER_BATTERY, ROM_SIZE_64KB, RAM_SIZE_32KB);

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
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC3_RAM, ROM_SIZE_64KB, RAM_SIZE_32KB);

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
        let mut mapper =
            build_cartridge_mapper(CART_TYPE_MBC3_TIMER_BATTERY, ROM_SIZE_64KB, RAM_SIZE_32KB);

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
        let mut rom = build_rom(CART_TYPE_MBC3, ROM_SIZE_64KB, RAM_SIZE_2KB);
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
        let mut rom = build_rom(CART_TYPE_MBC3, ROM_SIZE_64KB, RAM_SIZE_2KB);
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
            CART_TYPE_MBC3_TIMER_RAM_BATTERY,
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
    fn does_not_read_from_minutes_rtc_register_if_not_supported() {
        let mut mapper = build_cartridge_mapper_with_effects(
            CART_TYPE_MBC3_RAM_BATTERY,
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
        assert_eq!(byte, 0xFF);
    }

    #[test]
    fn writes_to_minutes_rtc_register() {
        let mut mapper = build_cartridge_mapper_with_effects(
            CART_TYPE_MBC3_TIMER_RAM_BATTERY,
            ROM_SIZE_64KB,
            RAM_SIZE_2KB,
            fake_cartridge_effects(),
        );

        mapper.write_rom(0x0000, 0xA);

        mapper.write_rom(0x4000, 0x9);

        // Latch RTC
        mapper.write_rom(0x6000, 0x0);
        mapper.write_rom(0x6000, 0x1);

        mapper.write_ram(0x0000, 0x2);
        let byte = mapper.read_ram(0x0000);
        assert_eq!(byte, 0x02);
    }

    #[test]
    fn does_not_write_to_minutes_rtc_register_if_not_supported() {
        let mut mapper = build_cartridge_mapper_with_effects(
            CART_TYPE_MBC3_RAM_BATTERY,
            ROM_SIZE_64KB,
            RAM_SIZE_2KB,
            fake_cartridge_effects(),
        );

        mapper.write_rom(0x0000, 0xA);

        mapper.write_rom(0x4000, 0x9);

        // Latch RTC
        mapper.write_rom(0x6000, 0x0);
        mapper.write_rom(0x6000, 0x1);

        mapper.write_ram(0x0000, 0x2);
        let byte = mapper.read_ram(0x0000);
        assert_eq!(byte, 0xFF);
    }

    #[test]
    fn battery_backed_ram_is_saved_once_per_report_while_enabled() {
        assert_eq!(ram_saves_after_writing(CART_TYPE_MBC3_RAM_BATTERY, 0x0A), 1);
        assert_eq!(
            ram_saves_after_writing(CART_TYPE_MBC3_TIMER_RAM_BATTERY, 0x0A),
            1
        );
        assert_eq!(ram_saves_after_writing(CART_TYPE_MBC3_RAM_BATTERY, 0x00), 0);
        assert_eq!(ram_saves_after_writing(CART_TYPE_MBC3_RAM, 0x0A), 0);
        assert_eq!(
            ram_saves_after_writing(CART_TYPE_MBC3_TIMER_BATTERY, 0x0A),
            0
        );
    }
}
