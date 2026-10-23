//! Game cartridges: reading the ROM header and choosing the memory bank
//! controller (MBC) that the cartridge uses.
//!
//! An MBC is the chip on the cartridge that switches which banks of its ROM
//! and RAM the console can see. Each kind of MBC keeps its registers in its
//! own module, and [`CartridgeMapper`] picks the one a cartridge has.

use crate::address_bus::bank_utils::{banked_read, banked_write};
use crate::address_bus::constants::{
    CARTRIDGE_TYPE_ADDRESS, CARTRIDGE_TYPE_HUC1_RAM_BATTERY, CARTRIDGE_TYPE_MBC1,
    CARTRIDGE_TYPE_MBC1_RAM, CARTRIDGE_TYPE_MBC1_RAM_BATTERY, CARTRIDGE_TYPE_MBC2,
    CARTRIDGE_TYPE_MBC2_BATTERY, CARTRIDGE_TYPE_MBC3, CARTRIDGE_TYPE_MBC3_RAM,
    CARTRIDGE_TYPE_MBC3_RAM_BATTERY, CARTRIDGE_TYPE_MBC3_TIMER_BATTERY,
    CARTRIDGE_TYPE_MBC3_TIMER_RAM_BATTERY, CARTRIDGE_TYPE_MBC5, CARTRIDGE_TYPE_MBC5_RAM,
    CARTRIDGE_TYPE_MBC5_RAM_BATTERY, CARTRIDGE_TYPE_MBC5_RUMBLE, CARTRIDGE_TYPE_MBC5_RUMBLE_RAM,
    CARTRIDGE_TYPE_MBC5_RUMBLE_RAM_BATTERY, CARTRIDGE_TYPE_ROM_ONLY, CARTRIDGE_TYPE_ROM_RAM,
    CARTRIDGE_TYPE_ROM_RAM_BATTERY, CGB_COMPATIBILITY_INDEX, HEADER_END_ADDRESS, MINIMUM_ROM_SIZE,
    RAM_BANK_SIZE, RAM_SIZE_ADDRESS, ROM_BANK_SIZE, ROM_SIZE_ADDRESS, SGB_SUPPORT_ADDRESS,
    TITLE_END_ADDRESS, TITLE_START_ADDRESS,
};
use crate::address_bus::effects::{CartridgeEffects, RtcClock, empty_cartridge_effects};
use crate::address_bus::huc1::Huc1CartridgeMapper;
use crate::address_bus::mbc1::Mbc1CartridgeMapper;
use crate::address_bus::mbc2::{MBC2_RAM_SIZE, Mbc2CartridgeMapper};
use crate::address_bus::mbc3::{Mbc3CartridgeMapper, RtcState, clock_time, rtc_time};
use crate::address_bus::mbc5::Mbc5CartridgeMapper;
use crate::error::LoadRomError;
use crate::serializable::{FieldWriter, Serializable, read_fields};
use nubo_serializable_derive::Serializable;
use std::io::{Read, Write};
use std::sync::Arc;

/// Information about a game, read from the header at the start of its ROM.
#[derive(Debug, Clone, Default)]
pub struct CartridgeHeader {
    /// Whether the game uses Super Game Boy features (header byte 0x146 is 0x03).
    pub supports_sgb: bool,
    /// The cartridge type byte at 0x147, which tells which memory bank
    /// controller and extra hardware (RAM, battery, clock, rumble) the
    /// cartridge has.
    pub cartridge_type: u8,
    /// The number of 16 KiB ROM banks the header announces, or 0 for a size
    /// code (header byte 0x148) that names no ROM size.
    pub rom_bank_count: u16,
    /// The number of 8 KiB RAM banks the header announces. A 2 KiB RAM counts
    /// as one bank. MBC2's built-in 512 half-byte RAM counts as zero.
    pub ram_bank_count: u8,
    /// The game title, up to 16 characters.
    pub title: String,
    /// Whether a battery keeps the cartridge RAM (and clock) running while the
    /// console is off, so the frontend should save it.
    pub has_battery: bool,
    /// Whether the game uses Game Boy Color features (the CGB flag at 0x143
    /// is 0x80 or 0xC0). Such games look best, or only work, on a CGB.
    pub supports_cgb: bool,
}

/// A game cartridge: its ROM, its RAM, the information from its header and
/// the memory bank controller that decides which parts of the ROM and RAM the
/// console sees.
///
/// Save states keep the RAM and the controller's registers; the rest comes
/// from the ROM and the frontend, which stay the same.
#[derive(Debug, Serializable)]
pub(crate) struct Cartridge {
    /// The ROM, padded to at least 32 KiB. The address bus shares it to read
    /// it without going through the cartridge (see `Cartridge::rom_banks`).
    #[serializable(skip)]
    rom: Arc<[u8]>,
    #[serializable(skip)]
    header: CartridgeHeader,
    /// The cartridge RAM. It is at least as large as the header says, but can
    /// be larger, or any size at all once a save state or
    /// `Cartridge::set_ram` replaced it; reads and writes wrap around its
    /// length.
    ram: Vec<u8>,
    mapper: CartridgeMapper,
    /// Lets the cartridge save its RAM and clock through the frontend.
    #[serializable(skip)]
    effects: Box<dyn CartridgeEffects>,
    /// Set when the game writes battery-backed RAM, until the RAM is passed to
    /// the frontend to save (see `Cartridge::report_save_changes`). Loading a
    /// save state replaces the RAM but leaves this alone, so the frontend
    /// keeps the player's own save until the game writes the RAM again.
    #[serializable(skip)]
    ram_changed: bool,
    /// How long the rumble motor was on since the frontend last asked
    /// (`Cartridge::take_rumble_level`). Loading a save state restarts it.
    #[serializable(skip)]
    rumble: RumbleMeter,
}

impl Cartridge {
    /// The ROM, shared so the address bus can read it directly.
    pub fn rom(&self) -> &Arc<[u8]> {
        &self.rom
    }

    pub fn header(&self) -> &CartridgeHeader {
        &self.header
    }

    /// The game title from the header.
    pub fn title(&self) -> &str {
        &self.header.title
    }

    pub fn ram(&self) -> &[u8] {
        &self.ram
    }

    /// The RAM, to change directly. Changes made this way aren't passed to
    /// the frontend to save, as the game's own writes are.
    pub fn ram_mut(&mut self) -> &mut [u8] {
        &mut self.ram
    }

    /// Replaces the cartridge RAM, for example with a save file. A buffer of
    /// the wrong size is accepted; reads and writes wrap around its length.
    pub fn set_ram(&mut self, ram: Vec<u8>) {
        self.ram = ram;
    }

    /// The ROM banks mapped at 0x0000-0x3FFF and at 0x4000-0x7FFF. Reading
    /// ROM is the same as reading those banks of 16 KiB, wrapping around the
    /// ROM's length (see `banked_read`), which lets the address bus read ROM
    /// without asking the cartridge each time.
    pub fn rom_banks(&self) -> [u16; 2] {
        self.mapper.rom_banks()
    }

    /// Reads from the ROM area, 0x0000-0x7FFF, through the banks in
    /// `rom_banks`, the way the address bus reads it.
    pub fn read_rom(&self, address: u16) -> u8 {
        let bank = self.rom_banks()[usize::from(address >> 14)];
        banked_read(&self.rom, ROM_BANK_SIZE, address, bank)
    }

    /// Handles a write to the ROM area. ROM can't be changed, so these writes
    /// set the controller's registers instead, such as the selected bank.
    /// `elapsed_dots` is how many PPU dots have passed since power-on, which
    /// an MBC3 clock following [`RtcClock::EmulatedTime`] counts.
    ///
    /// [`RtcClock::EmulatedTime`]: crate::RtcClock::EmulatedTime
    pub fn write_rom_at(&mut self, address: u16, value: u8, elapsed_dots: u64) {
        let effects = self.effects.as_ref();
        match &mut self.mapper {
            CartridgeMapper::RomOnly | CartridgeMapper::RomRam => {}
            CartridgeMapper::Mbc1(mbc1) => mbc1.write_rom(address, value),
            CartridgeMapper::Mbc2(mbc2) => mbc2.write_rom(address, value),
            CartridgeMapper::Mbc3(mbc3) => {
                mbc3.write_rom(address, value, || rtc_time(effects, elapsed_dots));
            }
            CartridgeMapper::Mbc5(mbc5) => {
                mbc5.write_rom(address, value);
                self.rumble.switch(mbc5.is_rumbling(), elapsed_dots);
            }
            CartridgeMapper::Huc1(huc1) => huc1.write_rom(address, value),
        }
    }

    /// Reads from the cartridge RAM area. `address` is relative to 0xA000.
    pub fn read_ram(&self, address: u16) -> u8 {
        let ram = &self.ram;
        match &self.mapper {
            CartridgeMapper::RomOnly => 0xFF,
            CartridgeMapper::RomRam => banked_read(ram, RAM_BANK_SIZE, address, 0),
            CartridgeMapper::Mbc1(mbc1) => mbc1.read_ram(ram, address),
            CartridgeMapper::Mbc2(mbc2) => mbc2.read_ram(ram, address),
            CartridgeMapper::Mbc3(mbc3) => mbc3.read_ram(ram, address),
            CartridgeMapper::Mbc5(mbc5) => mbc5.read_ram(ram, address),
            CartridgeMapper::Huc1(huc1) => huc1.read_ram(ram, address),
        }
    }

    /// Writes to the cartridge RAM area. `address` is relative to 0xA000, and
    /// `elapsed_dots` is the time as for [`Cartridge::write_rom_at`].
    pub fn write_ram_at(&mut self, address: u16, value: u8, elapsed_dots: u64) {
        let effects = self.effects.as_ref();
        let ram = &mut self.ram;
        let stored = match &mut self.mapper {
            CartridgeMapper::RomOnly => false,
            CartridgeMapper::RomRam => banked_write(ram, RAM_BANK_SIZE, address, 0, value),
            CartridgeMapper::Mbc1(mbc1) => mbc1.write_ram(ram, address, value),
            CartridgeMapper::Mbc2(mbc2) => mbc2.write_ram(ram, address, value),
            CartridgeMapper::Mbc3(mbc3) => {
                mbc3.write_ram(ram, address, value, || rtc_time(effects, elapsed_dots))
            }
            CartridgeMapper::Mbc5(mbc5) => mbc5.write_ram(ram, address, value),
            CartridgeMapper::Huc1(huc1) => huc1.write_ram(ram, address, value),
        };
        if stored && self.header.has_battery {
            self.ram_changed = true;
        }
    }

    /// The number GameShark codes compare their bank byte with at
    /// 0xA000-0xBFFF: the RAM bank the controller has selected. On an MBC3
    /// it is whatever 0xA000-0xBFFF shows, so 0x08-0x0F while a clock
    /// register is selected, like in SameBoy.
    pub fn selected_ram_bank(&self) -> u8 {
        match &self.mapper {
            CartridgeMapper::RomOnly | CartridgeMapper::RomRam | CartridgeMapper::Mbc2(_) => 0,
            CartridgeMapper::Mbc1(mbc1) => mbc1.ram_bank(),
            CartridgeMapper::Mbc3(mbc3) => mbc3.ram_bank_or_rtc_register(),
            CartridgeMapper::Mbc5(mbc5) => mbc5.ram_bank(),
            CartridgeMapper::Huc1(huc1) => huc1.ram_bank(),
        }
    }

    /// Passes the battery-backed RAM and the clock to the frontend to save if
    /// the game changed them since the last call. The emulator calls this at
    /// the end of each call that ran the CPU, instead of after every change,
    /// because a game that uses battery-backed RAM as scratch space can write
    /// it thousands of times a frame and saving copies all of it, and games
    /// that show the time (such as Pokémon Gold) read the clock about once a
    /// frame.
    pub fn report_save_changes(&mut self) {
        if self.ram_changed {
            self.ram_changed = false;
            self.effects.save_ram(&self.ram);
        }
        if let CartridgeMapper::Mbc3(mbc3) = &mut self.mapper
            && let Some(rtc) = mbc3.take_changed_rtc()
        {
            self.effects.save_rtc_state(rtc);
        }
    }

    /// The state of the cartridge's real-time clock, expressed on `clock`, or
    /// `None` without one. `elapsed_dots` is how many PPU dots have passed
    /// since power-on.
    pub fn rtc_state(&self, clock: RtcClock, elapsed_dots: u64) -> Option<RtcState> {
        let CartridgeMapper::Mbc3(mbc3) = &self.mapper else {
            return None;
        };
        let mut state = mbc3.rtc()?.clone();
        let own_clock = self.effects.rtc_clock();
        if clock != own_clock {
            let now = |clock| clock_time(self.effects.as_ref(), clock, elapsed_dots);
            state.rebase(now(own_clock), now(clock));
        }
        Some(state)
    }

    /// Replaces the state of the cartridge's real-time clock with `state`,
    /// whose `base_timestamp` is a time on `clock`, as if the cartridge had
    /// been inserted with it. Does nothing on a cartridge without a clock.
    /// The registers are cut to their widths before the clock moves over to
    /// the other [`RtcClock`], since counting on assumes they fit.
    pub fn set_rtc_state(&mut self, state: RtcState, clock: RtcClock, elapsed_dots: u64) {
        let mut state = state.within_register_widths();
        let own_clock = self.effects.rtc_clock();
        if clock != own_clock {
            let now = |clock| clock_time(self.effects.as_ref(), clock, elapsed_dots);
            state.rebase(now(clock), now(own_clock));
        }
        if let CartridgeMapper::Mbc3(mbc3) = &mut self.mapper {
            mbc3.set_rtc(state);
        }
    }

    /// Cuts a loaded MBC3's registers to the bits they have, and moves a
    /// clock loaded from a save state made on the other [`RtcClock`] over to
    /// the one this cartridge follows, keeping what it shows.
    pub fn finish_loading_save_state(&mut self, elapsed_dots: u64) {
        match &mut self.mapper {
            CartridgeMapper::Mbc3(mbc3) => {
                let effects = self.effects.as_ref();
                mbc3.finish_loading_save_state(effects.rtc_clock(), |clock| {
                    clock_time(effects, clock, elapsed_dots)
                });
            }
            CartridgeMapper::Mbc5(mbc5) => {
                self.rumble = RumbleMeter::starting(mbc5.is_rumbling(), elapsed_dots);
            }
            _ => self.rumble = RumbleMeter::default(),
        }
    }

    /// The share of the time since the last call, up to `elapsed_dots` PPU
    /// dots since power-on, that the rumble motor was on: from 0.0 to 1.0.
    /// Games pulse the motor many times a frame to rumble more gently.
    pub fn take_rumble_level(&mut self, elapsed_dots: u64) -> f32 {
        self.rumble.take_level(elapsed_dots)
    }
}

/// Measures how long a rumble cartridge's motor is on, in PPU dots since
/// power-on, between two calls to [`RumbleMeter::take_level`].
#[derive(Debug, Default)]
struct RumbleMeter {
    /// When the current measurement started.
    since: u64,
    /// When the motor was switched on, while it is on.
    on_since: Option<u64>,
    /// How long the motor was on in the current measurement, not counting
    /// the time since `on_since`.
    on_dots: u64,
}

impl RumbleMeter {
    /// A measurement starting at `now`, with the motor on or off.
    fn starting(on: bool, now: u64) -> Self {
        RumbleMeter {
            since: now,
            on_since: on.then_some(now),
            on_dots: 0,
        }
    }

    /// Notes that the motor is `on` (or still is) at `now`.
    fn switch(&mut self, on: bool, now: u64) {
        match (on, self.on_since) {
            (true, None) => self.on_since = Some(now),
            (false, Some(start)) => {
                self.on_dots += now.saturating_sub(start.max(self.since));
                self.on_since = None;
            }
            _ => {}
        }
    }

    /// The share of the time from the start of the measurement to `now` the
    /// motor was on, starting a new measurement at `now`.
    fn take_level(&mut self, now: u64) -> f32 {
        let total = now.saturating_sub(self.since);
        let mut on = self.on_dots;
        if let Some(start) = self.on_since {
            on += now.saturating_sub(start.max(self.since));
        }
        let level = if total == 0 {
            if self.on_since.is_some() { 1.0 } else { 0.0 }
        } else {
            (on as f64 / total as f64).min(1.0) as f32
        };
        *self = RumbleMeter::starting(self.on_since.is_some(), now);
        level
    }
}

/// The memory bank controller on a cartridge, with its registers. Each kind
/// of controller decides differently which part of the ROM and RAM the
/// console sees.
#[derive(Debug)]
enum CartridgeMapper {
    /// No controller: 32 KiB of ROM, always visible, and no RAM.
    RomOnly,
    /// No controller, but RAM next to the 32 KiB of ROM (cartridge types
    /// 0x08 and 0x09). No licensed game is known to use one, and how real
    /// hardware behaves is unknown; as daid's `rom_and_ram.gb` suggests, the
    /// RAM is always on, since there is no controller to turn it on or off,
    /// and a RAM smaller than 8 KiB repeats across 0xA000-0xBFFF.
    RomRam,
    Mbc1(Mbc1CartridgeMapper),
    Mbc2(Mbc2CartridgeMapper),
    Mbc3(Mbc3CartridgeMapper),
    Mbc5(Mbc5CartridgeMapper),
    Huc1(Huc1CartridgeMapper),
}

impl CartridgeMapper {
    /// See [`Cartridge::rom_banks`].
    fn rom_banks(&self) -> [u16; 2] {
        match self {
            Self::RomOnly | Self::RomRam => [0, 1],
            Self::Mbc1(mbc1) => mbc1.rom_banks(),
            Self::Mbc2(mbc2) => mbc2.rom_banks(),
            Self::Mbc3(mbc3) => mbc3.rom_banks(),
            Self::Mbc5(mbc5) => mbc5.rom_banks(),
            Self::Huc1(huc1) => huc1.rom_banks(),
        }
    }
}

/// Save states keep the controller's registers as one field named after the
/// kind of controller (`mbc1`, `mbc3`, ...), with nothing for a cartridge
/// without one. Which controller it is comes from the ROM, since a save state
/// only loads into the game it was made with; the name makes sure that if a
/// later version picks another controller for a cartridge type, it starts
/// that one at power-on instead of reading the other's registers.
impl Serializable for CartridgeMapper {
    fn serialize(&self, writer: &mut dyn Write) -> std::io::Result<()> {
        let mut fields = FieldWriter::new(writer);
        match self {
            Self::RomOnly | Self::RomRam => {}
            Self::Mbc1(mbc1) => fields.field("mbc1", mbc1)?,
            Self::Mbc2(mbc2) => fields.field("mbc2", mbc2)?,
            Self::Mbc3(mbc3) => fields.field("mbc3", mbc3)?,
            Self::Mbc5(mbc5) => fields.field("mbc5", mbc5)?,
            Self::Huc1(huc1) => fields.field("huc1", huc1)?,
        }
        fields.finish()
    }

    fn deserialize(&mut self, reader: &mut dyn Read) -> std::io::Result<()> {
        read_fields(reader, |name, bytes| {
            match (name, &mut *self) {
                ("mbc1", Self::Mbc1(mbc1)) => mbc1.deserialize(bytes)?,
                ("mbc2", Self::Mbc2(mbc2)) => mbc2.deserialize(bytes)?,
                ("mbc3", Self::Mbc3(mbc3)) => mbc3.deserialize(bytes)?,
                ("mbc5", Self::Mbc5(mbc5)) => mbc5.deserialize(bytes)?,
                ("huc1", Self::Huc1(huc1)) => huc1.deserialize(bytes)?,
                _ => return Ok(false),
            }
            Ok(true)
        })
    }
}

/// The memory bank controllers Nubo emulates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MapperKind {
    RomOnly,
    RomRam,
    Mbc1,
    Mbc2,
    Mbc3,
    Mbc5,
    Huc1,
}

impl MapperKind {
    /// Returns the controller for a cartridge type byte, or `None` when
    /// Nubo doesn't emulate it.
    fn from_cartridge_type(cartridge_type: u8) -> Option<Self> {
        match cartridge_type {
            CARTRIDGE_TYPE_ROM_ONLY => Some(Self::RomOnly),
            CARTRIDGE_TYPE_ROM_RAM | CARTRIDGE_TYPE_ROM_RAM_BATTERY => Some(Self::RomRam),
            CARTRIDGE_TYPE_MBC1 | CARTRIDGE_TYPE_MBC1_RAM | CARTRIDGE_TYPE_MBC1_RAM_BATTERY => {
                Some(Self::Mbc1)
            }
            CARTRIDGE_TYPE_MBC2 | CARTRIDGE_TYPE_MBC2_BATTERY => Some(Self::Mbc2),
            CARTRIDGE_TYPE_MBC3
            | CARTRIDGE_TYPE_MBC3_RAM
            | CARTRIDGE_TYPE_MBC3_RAM_BATTERY
            | CARTRIDGE_TYPE_MBC3_TIMER_BATTERY
            | CARTRIDGE_TYPE_MBC3_TIMER_RAM_BATTERY => Some(Self::Mbc3),
            CARTRIDGE_TYPE_MBC5
            | CARTRIDGE_TYPE_MBC5_RAM
            | CARTRIDGE_TYPE_MBC5_RAM_BATTERY
            | CARTRIDGE_TYPE_MBC5_RUMBLE
            | CARTRIDGE_TYPE_MBC5_RUMBLE_RAM
            | CARTRIDGE_TYPE_MBC5_RUMBLE_RAM_BATTERY => Some(Self::Mbc5),
            CARTRIDGE_TYPE_HUC1_RAM_BATTERY => Some(Self::Huc1),
            _ => None,
        }
    }

    /// The controller in its power-on state, for a cartridge with this ROM,
    /// type byte and RAM size (from the header). Whether it has RAM comes
    /// from the type byte, never from the size of a save file. An MBC3 with
    /// more ROM or RAM than an MBC3 can address is an MBC30, and its clock
    /// starts from the state the frontend saved, if any.
    fn create_mapper(
        self,
        rom: &[u8],
        cartridge_type: u8,
        ram_size: usize,
        effects: &dyn CartridgeEffects,
    ) -> CartridgeMapper {
        match self {
            Self::RomOnly => CartridgeMapper::RomOnly,
            Self::RomRam => CartridgeMapper::RomRam,
            Self::Mbc1 => CartridgeMapper::Mbc1(Mbc1CartridgeMapper::new(rom, cartridge_type)),
            Self::Mbc2 => CartridgeMapper::Mbc2(Mbc2CartridgeMapper::new()),
            Self::Mbc3 => CartridgeMapper::Mbc3(Mbc3CartridgeMapper::new(
                cartridge_type,
                rom.len(),
                ram_size,
                effects.rtc_clock(),
                effects.load_rtc_state(),
            )),
            Self::Mbc5 => CartridgeMapper::Mbc5(Mbc5CartridgeMapper::new(cartridge_type)),
            Self::Huc1 => CartridgeMapper::Huc1(Huc1CartridgeMapper::new()),
        }
    }
}

/// An empty cartridge slot, used before a game is loaded: no ROM, no RAM and
/// nothing to save.
pub(super) fn empty_cartridge() -> Cartridge {
    Cartridge {
        rom: Arc::from([]),
        header: CartridgeHeader::default(),
        ram: Vec::new(),
        mapper: CartridgeMapper::RomOnly,
        effects: empty_cartridge_effects(),
        ram_changed: false,
        rumble: RumbleMeter::default(),
    }
}

/// Converts the ROM size byte from the header into a number of 16 KiB banks:
/// 2 to 512 for codes 0x00-0x08, the 1.1, 1.2 and 1.5 MiB sizes Pan Docs lists
/// for codes 0x52-0x54, and 0 for codes that name no size.
pub(super) fn rom_bank_count(rom_size_index: u8) -> u16 {
    match rom_size_index {
        0x00..=0x08 => 2 << rom_size_index,
        0x52 => 72,
        0x53 => 80,
        0x54 => 96,
        _ => 0,
    }
}

/// Converts a RAM size in bytes into a number of 8 KiB banks. A 2 KiB RAM
/// counts as one bank.
fn ram_bank_count(ram_size: usize) -> u8 {
    match ram_size {
        0 => 0,
        0x800 => 1,
        _ => u8::try_from(ram_size / RAM_BANK_SIZE).unwrap_or(u8::MAX),
    }
}

/// Whether the header's cartridge type byte names a cartridge with a battery,
/// which keeps its RAM (and clock) while the console is off.
fn is_battery_backed(cartridge_type: u8) -> bool {
    matches!(
        cartridge_type,
        CARTRIDGE_TYPE_MBC1_RAM_BATTERY
            | CARTRIDGE_TYPE_MBC2_BATTERY
            | CARTRIDGE_TYPE_ROM_RAM_BATTERY
            | CARTRIDGE_TYPE_MBC3_TIMER_BATTERY
            | CARTRIDGE_TYPE_MBC3_TIMER_RAM_BATTERY
            | CARTRIDGE_TYPE_MBC3_RAM_BATTERY
            | CARTRIDGE_TYPE_MBC5_RAM_BATTERY
            | CARTRIDGE_TYPE_MBC5_RUMBLE_RAM_BATTERY
            | CARTRIDGE_TYPE_HUC1_RAM_BATTERY
    )
}

/// Whether the header's cartridge type byte names a cartridge without RAM
/// (ROM only, and MBC1, MBC3 and MBC5 without "+RAM"). Such a cartridge gets
/// no RAM buffer, whatever RAM size its header claims, since its controller
/// never reaches one, and its controller never enables RAM.
pub(super) fn lacks_ram(cartridge_type: u8) -> bool {
    matches!(
        cartridge_type,
        CARTRIDGE_TYPE_ROM_ONLY
            | CARTRIDGE_TYPE_MBC1
            | CARTRIDGE_TYPE_MBC3
            | CARTRIDGE_TYPE_MBC3_TIMER_BATTERY
            | CARTRIDGE_TYPE_MBC5
            | CARTRIDGE_TYPE_MBC5_RUMBLE
    )
}

/// Returns the size of the cartridge RAM the header announces in bytes, or
/// `None` when the header holds a RAM size byte that does not exist.
fn ram_size(kind: MapperKind, ram_size_index: u8) -> Option<usize> {
    if kind == MapperKind::Mbc2 {
        return Some(MBC2_RAM_SIZE);
    }
    match ram_size_index {
        0x0 => Some(0),
        0x1 => Some(0x800),
        0x2 => Some(0x2000),
        0x3 => Some(0x8000),
        0x4 => Some(0x20000),
        0x5 => Some(0x10000),
        _ => None,
    }
}

/// Whether `byte` at `index` of the title is the CGB flag: 0x80 (works on
/// both) or 0xC0 (Game Boy Color only) in the title's last byte.
fn is_cgb_compatibility_flag(index: usize, byte: u8) -> bool {
    index == CGB_COMPATIBILITY_INDEX && (byte == 0xC0 || byte == 0x80)
}

/// Reads the title from the header. It ends at the first zero byte or at the
/// CGB flag, which newer games store in the title's last byte.
fn read_title(rom: &[u8]) -> String {
    rom[TITLE_START_ADDRESS..=TITLE_END_ADDRESS]
        .iter()
        .enumerate()
        .take_while(|&(index, &byte)| !is_cgb_compatibility_flag(index, byte) && byte != 0x00)
        .map(|(_, &byte)| char::from(byte))
        .collect()
}

/// Returns the name Pan Docs uses for a cartridge type byte.
pub(super) fn cartridge_type_name(cartridge_type: u8) -> &'static str {
    match cartridge_type {
        0x00 => "ROM ONLY",
        0x01 => "MBC1",
        0x02 => "MBC1+RAM",
        0x03 => "MBC1+RAM+BATTERY",
        0x05 => "MBC2",
        0x06 => "MBC2+BATTERY",
        0x08 => "ROM+RAM",
        0x09 => "ROM+RAM+BATTERY",
        0x0B => "MMM01",
        0x0C => "MMM01+RAM",
        0x0D => "MMM01+RAM+BATTERY",
        0x0F => "MBC3+TIMER+BATTERY",
        0x10 => "MBC3+TIMER+RAM+BATTERY",
        0x11 => "MBC3",
        0x12 => "MBC3+RAM",
        0x13 => "MBC3+RAM+BATTERY",
        0x19 => "MBC5",
        0x1A => "MBC5+RAM",
        0x1B => "MBC5+RAM+BATTERY",
        0x1C => "MBC5+RUMBLE",
        0x1D => "MBC5+RUMBLE+RAM",
        0x1E => "MBC5+RUMBLE+RAM+BATTERY",
        0x20 => "MBC6",
        0x22 => "MBC7+SENSOR+RUMBLE+RAM+BATTERY",
        0xFC => "POCKET CAMERA",
        0xFD => "BANDAI TAMA5",
        0xFE => "HuC3",
        0xFF => "HuC1+RAM+BATTERY",
        _ => "UNKNOWN",
    }
}

/// Reads the cartridge header from a ROM and checks that Nubo supports
/// its cartridge type and RAM size. Returns the header together with the
/// mapper kind and the RAM size in bytes. The header's RAM bank count comes
/// from that size.
fn parse_header(rom: &[u8]) -> Result<(CartridgeHeader, MapperKind, usize), LoadRomError> {
    if rom.len() <= HEADER_END_ADDRESS {
        return Err(LoadRomError::TooSmall);
    }

    let cartridge_type = rom[CARTRIDGE_TYPE_ADDRESS];
    let kind = MapperKind::from_cartridge_type(cartridge_type).ok_or(
        LoadRomError::UnsupportedCartridge(cartridge_type_name(cartridge_type)),
    )?;
    let ram_size_index = rom[RAM_SIZE_ADDRESS];
    let ram_size =
        ram_size(kind, ram_size_index).ok_or(LoadRomError::InvalidRamSize(ram_size_index))?;
    let cgb_flag = rom[TITLE_START_ADDRESS + CGB_COMPATIBILITY_INDEX];

    let header = CartridgeHeader {
        supports_sgb: rom[SGB_SUPPORT_ADDRESS] == 0x03,
        cartridge_type,
        rom_bank_count: rom_bank_count(rom[ROM_SIZE_ADDRESS]),
        ram_bank_count: ram_bank_count(ram_size),
        title: read_title(rom),
        has_battery: is_battery_backed(cartridge_type),
        supports_cgb: is_cgb_compatibility_flag(CGB_COMPATIBILITY_INDEX, cgb_flag),
    };

    Ok((header, kind, ram_size))
}

/// Reads the cartridge header from a ROM without loading it, for example to
/// list games in a library.
pub(crate) fn read_header(rom: &[u8]) -> Result<CartridgeHeader, LoadRomError> {
    parse_header(rom).map(|(header, _, _)| header)
}

/// Reads the cartridge header from a ROM and creates the cartridge, with the
/// controller that matches its cartridge type. When the frontend has saved
/// RAM for this game, `effects` provides it; otherwise the RAM starts out
/// zeroed. A save shorter than the RAM the header announces fills its start
/// and the rest starts zeroed, so even an empty save file leaves the game
/// RAM to save to. A longer one is kept whole. A cartridge type without RAM
/// gets none, even when its header announces some; the header's size still
/// decides whether an MBC3 is an MBC30.
pub(super) fn load_cartridge(
    mut rom: Vec<u8>,
    effects: Box<dyn CartridgeEffects>,
) -> Result<Cartridge, LoadRomError> {
    let (header, kind, ram_size) = parse_header(&rom)?;
    let usable_ram_size = if lacks_ram(header.cartridge_type) {
        0
    } else {
        ram_size
    };
    let mut ram = effects.load_ram().unwrap_or_default();
    if ram.len() < usable_ram_size {
        ram.resize(usable_ram_size, 0);
    }

    if rom.len() < MINIMUM_ROM_SIZE {
        rom.resize(MINIMUM_ROM_SIZE, 0xFF);
    }

    let mapper = kind.create_mapper(&rom, header.cartridge_type, ram_size, effects.as_ref());
    Ok(Cartridge {
        rom: rom.into(),
        header,
        ram,
        mapper,
        effects,
        ram_changed: false,
        rumble: RumbleMeter::default(),
    })
}

/// Writes at power-on, for the many tests that don't use the clock.
#[cfg(test)]
impl Cartridge {
    pub fn write_rom(&mut self, address: u16, value: u8) {
        self.write_rom_at(address, value, 0);
    }

    pub fn write_ram(&mut self, address: u16, value: u8) {
        self.write_ram_at(address, value, 0);
    }
}

#[cfg(test)]
pub(crate) mod test_utils {
    use super::*;
    use crate::address_bus::test_utils::*;

    pub(crate) fn build_cartridge_mapper(
        cartridge_type: u8,
        rom_size_index: u8,
        ram_size_index: u8,
    ) -> Cartridge {
        let rom_buffer = build_rom(cartridge_type, rom_size_index, ram_size_index);
        load_cartridge(rom_buffer, empty_cartridge_effects()).unwrap()
    }

    pub(crate) fn build_cartridge_mapper_with_effects(
        cartridge_type: u8,
        rom_size_index: u8,
        ram_size_index: u8,
        cartridge_effects: Box<dyn CartridgeEffects>,
    ) -> Cartridge {
        let rom_buffer = build_rom(cartridge_type, rom_size_index, ram_size_index);
        load_cartridge(rom_buffer, cartridge_effects).unwrap()
    }

    /// Cartridge effects that count how often the RAM is saved.
    pub(crate) struct RamSaveCounter(pub(crate) crate::address_bus::effects::Shared<usize>);

    impl CartridgeEffects for RamSaveCounter {
        fn current_time_millis(&self) -> f64 {
            0.0
        }

        fn save_ram(&mut self, _ram: &[u8]) {
            self.0.set(self.0.get() + 1);
        }
    }

    /// Loads a cartridge of `cartridge_type` with 32 KiB of RAM in its
    /// header, writes `ram_enable` to the RAM enable register (0x0000), then
    /// writes every byte of 0xA000-0xBFFF, and reports the changes twice, as
    /// the emulator does at the end of two calls. Returns how often the RAM
    /// was passed to the frontend to save.
    pub(crate) fn ram_saves_after_writing(cartridge_type: u8, ram_enable: u8) -> usize {
        let saves = crate::address_bus::effects::Shared::<usize>::default();
        let mut cartridge = build_cartridge_mapper_with_effects(
            cartridge_type,
            0x01,
            0x03,
            Box::new(RamSaveCounter(saves.clone())),
        );
        cartridge.write_rom(0x0000, ram_enable);
        for address in 0x0000..0x2000 {
            cartridge.write_ram(address, address as u8);
        }
        cartridge.report_save_changes();
        cartridge.report_save_changes();
        saves.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address_bus::test_utils::build_rom;

    #[test]
    fn rejects_a_rom_that_is_too_small_for_a_header() {
        let result = load_cartridge(vec![0; 0x120], empty_cartridge_effects());
        assert_eq!(result.err(), Some(LoadRomError::TooSmall));
    }

    #[test]
    fn the_rom_bank_count_follows_the_header_size_codes() {
        let counts = [0x00, 0x01, 0x08, 0x09, 0x0F, 0x52, 0x53, 0x54, 0xFF].map(rom_bank_count);
        assert_eq!(counts, [2, 4, 512, 0, 0, 72, 80, 96, 0]);
    }

    #[test]
    fn rejects_an_unsupported_cartridge_type() {
        let rom = build_rom(0x22, 0x01, 0x00);
        let result = load_cartridge(rom, empty_cartridge_effects());
        assert_eq!(
            result.err(),
            Some(LoadRomError::UnsupportedCartridge(
                "MBC7+SENSOR+RUMBLE+RAM+BATTERY"
            ))
        );
    }

    #[test]
    fn rejects_an_invalid_ram_size() {
        let rom = build_rom(CARTRIDGE_TYPE_MBC1, 0x01, 0x09);
        let result = load_cartridge(rom, empty_cartridge_effects());
        assert_eq!(result.err(), Some(LoadRomError::InvalidRamSize(0x09)));
    }

    const SUPPORTED_CARTRIDGE_TYPES: [u8; 20] = [
        CARTRIDGE_TYPE_ROM_ONLY,
        CARTRIDGE_TYPE_ROM_RAM,
        CARTRIDGE_TYPE_ROM_RAM_BATTERY,
        CARTRIDGE_TYPE_MBC1,
        CARTRIDGE_TYPE_MBC1_RAM,
        CARTRIDGE_TYPE_MBC1_RAM_BATTERY,
        CARTRIDGE_TYPE_MBC2,
        CARTRIDGE_TYPE_MBC2_BATTERY,
        CARTRIDGE_TYPE_MBC3_TIMER_BATTERY,
        CARTRIDGE_TYPE_MBC3_TIMER_RAM_BATTERY,
        CARTRIDGE_TYPE_MBC3,
        CARTRIDGE_TYPE_MBC3_RAM,
        CARTRIDGE_TYPE_MBC3_RAM_BATTERY,
        CARTRIDGE_TYPE_MBC5,
        CARTRIDGE_TYPE_MBC5_RAM,
        CARTRIDGE_TYPE_MBC5_RAM_BATTERY,
        CARTRIDGE_TYPE_MBC5_RUMBLE,
        CARTRIDGE_TYPE_MBC5_RUMBLE_RAM,
        CARTRIDGE_TYPE_MBC5_RUMBLE_RAM_BATTERY,
        CARTRIDGE_TYPE_HUC1_RAM_BATTERY,
    ];

    /// Writes many values to every controller register and then reads and
    /// writes the whole ROM and RAM area.
    fn exercise(cartridge: &mut Cartridge) {
        for value in [0x00, 0x01, 0x03, 0x08, 0x0A, 0x0C, 0x0E, 0x7F, 0xFF] {
            for register in (0x0000..0x8000).step_by(0x100) {
                cartridge.write_rom(register, value);
                for address in (0x0000..0x2000).step_by(0x3FF) {
                    cartridge.write_ram(address, value);
                    cartridge.read_ram(address);
                }
            }
            for address in (0x0000..0x8000).step_by(0x7FF) {
                cartridge.read_rom(address);
            }
        }
    }

    #[test]
    fn any_header_ram_size_is_handled_without_panicking() {
        for cartridge_type in SUPPORTED_CARTRIDGE_TYPES {
            for ram_size_index in 0..=5 {
                let rom = build_rom(cartridge_type, 0x01, ram_size_index);
                let mut cartridge = load_cartridge(rom, empty_cartridge_effects()).unwrap();
                exercise(&mut cartridge);
            }
        }
    }

    #[test]
    fn cartridge_ram_of_any_size_is_handled_without_panicking() {
        for cartridge_type in SUPPORTED_CARTRIDGE_TYPES {
            for ram_size in [0, 1, 0x1FF, 0x7FF, 0x2001, 0x8000] {
                let rom = build_rom(cartridge_type, 0x01, 0x03);
                let mut cartridge = load_cartridge(rom, empty_cartridge_effects()).unwrap();
                cartridge.set_ram(vec![0x5A; ram_size]);
                exercise(&mut cartridge);
            }
        }
    }

    #[test]
    fn a_save_state_with_a_different_ram_size_is_handled_without_panicking() {
        for cartridge_type in SUPPORTED_CARTRIDGE_TYPES {
            let mut small_ram_cartridge = load_cartridge(
                build_rom(cartridge_type, 0x01, 0x00),
                empty_cartridge_effects(),
            )
            .unwrap();
            small_ram_cartridge.set_ram(vec![0x5A; 3]);
            let mut state = Vec::new();
            small_ram_cartridge.serialize(&mut state).unwrap();

            let mut cartridge = load_cartridge(
                build_rom(cartridge_type, 0x01, 0x03),
                empty_cartridge_effects(),
            )
            .unwrap();
            cartridge.deserialize(&mut state.as_slice()).unwrap();
            exercise(&mut cartridge);
        }
    }

    #[test]
    fn a_controller_keeps_its_power_on_registers_in_a_state_of_another_controller() {
        let mut mbc1 = load_cartridge(
            build_rom(CARTRIDGE_TYPE_MBC1_RAM_BATTERY, 0x05, 0x03),
            empty_cartridge_effects(),
        )
        .unwrap();
        mbc1.write_rom(0x2000, 0x05);
        let mut state = Vec::new();
        mbc1.serialize(&mut state).unwrap();

        let mut mbc5 = load_cartridge(
            build_rom(CARTRIDGE_TYPE_MBC5_RAM_BATTERY, 0x05, 0x03),
            empty_cartridge_effects(),
        )
        .unwrap();
        let unreadable =
            crate::serializable::collect_unreadable(|| mbc5.deserialize(&mut state.as_slice()))
                .unwrap();
        assert!(unreadable.is_empty());
        assert_eq!(mbc5.rom_banks(), [0, 1]);

        let layout = crate::serializable::stored_layout(|| mbc1.serialize(&mut std::io::sink()));
        assert!(layout.unwrap().contains("mapper.mbc1.bank1: u8"));
    }

    /// Cartridge effects that load `ram` as the game's save.
    struct SavedRam(Vec<u8>);

    impl CartridgeEffects for SavedRam {
        fn current_time_millis(&self) -> f64 {
            0.0
        }

        fn load_ram(&self) -> Option<Vec<u8>> {
            Some(self.0.clone())
        }
    }

    #[test]
    fn the_header_ram_size_does_not_depend_on_the_save_file() {
        let rom = build_rom(CARTRIDGE_TYPE_MBC1_RAM_BATTERY, 0x01, 0x03);
        let cartridge =
            load_cartridge(rom.clone(), Box::new(SavedRam(vec![0x5A; 0x2000]))).unwrap();
        assert_eq!(cartridge.header().ram_bank_count, 4);
        assert_eq!(read_header(&rom).unwrap().ram_bank_count, 4);
    }

    #[test]
    fn a_short_save_file_fills_the_start_of_the_ram() {
        for cartridge_type in SUPPORTED_CARTRIDGE_TYPES {
            let rom = build_rom(cartridge_type, 0x01, 0x03);
            let header_ram_size = load_cartridge(rom.clone(), empty_cartridge_effects())
                .unwrap()
                .ram()
                .len();
            let cartridge = load_cartridge(rom, Box::new(SavedRam(vec![0x5A; 3]))).unwrap();
            assert_eq!(cartridge.ram().len(), header_ram_size.max(3));
            assert_eq!(cartridge.ram()[..3], [0x5A; 3]);
        }
    }

    #[test]
    fn a_cartridge_without_ram_gets_none_whatever_its_header_says() {
        for cartridge_type in [
            CARTRIDGE_TYPE_ROM_ONLY,
            CARTRIDGE_TYPE_MBC1,
            CARTRIDGE_TYPE_MBC3,
            CARTRIDGE_TYPE_MBC3_TIMER_BATTERY,
            CARTRIDGE_TYPE_MBC5,
            CARTRIDGE_TYPE_MBC5_RUMBLE,
        ] {
            let rom = build_rom(cartridge_type, 0x01, 0x04);
            let cartridge = load_cartridge(rom, empty_cartridge_effects()).unwrap();
            assert!(cartridge.ram().is_empty());
            assert_eq!(cartridge.header().ram_bank_count, 16);
        }
        let rom = build_rom(CARTRIDGE_TYPE_MBC1_RAM, 0x01, 0x04);
        let cartridge = load_cartridge(rom, empty_cartridge_effects()).unwrap();
        assert_eq!(cartridge.ram().len(), 0x20000);
    }

    #[test]
    fn a_long_save_file_is_kept_whole() {
        let rom = build_rom(CARTRIDGE_TYPE_MBC5_RAM_BATTERY, 0x01, 0x02);
        let cartridge = load_cartridge(rom, Box::new(SavedRam(vec![0x5A; 0x2030]))).unwrap();
        assert_eq!(cartridge.ram(), [0x5A; 0x2030]);
    }

    #[test]
    fn a_huc1_game_with_an_empty_save_file_can_still_save() {
        let rom = build_rom(CARTRIDGE_TYPE_HUC1_RAM_BATTERY, 0x01, 0x03);
        let mut cartridge = load_cartridge(rom, Box::new(SavedRam(Vec::new()))).unwrap();
        cartridge.write_rom(0x0000, 0x0A);
        cartridge.write_ram(0x0005, 0xCC);
        assert_eq!(cartridge.read_ram(0x0005), 0xCC);
        assert!(cartridge.ram_changed);
    }

    /// Loading a save state must never overwrite the player's battery save by
    /// itself (see `Emulator::load_state`).
    #[test]
    fn loading_a_save_state_does_not_save_the_ram() {
        let saves = crate::address_bus::effects::Shared::<usize>::default();
        let mut cartridge = test_utils::build_cartridge_mapper_with_effects(
            CARTRIDGE_TYPE_MBC1_RAM_BATTERY,
            0x01,
            0x03,
            Box::new(test_utils::RamSaveCounter(saves.clone())),
        );
        let mut state = Vec::new();
        cartridge.serialize(&mut state).unwrap();
        cartridge.write_rom(0x0000, 0x0A);
        cartridge.write_ram(0x0005, 0xCC);
        cartridge.report_save_changes();
        assert_eq!(saves.get(), 1);

        cartridge.deserialize(&mut state.as_slice()).unwrap();
        cartridge.report_save_changes();
        assert_eq!(saves.get(), 1);
        assert_eq!(cartridge.ram()[5], 0x00);
    }

    /// Writes every value to every controller register and checks after each
    /// write that reading the ROM through the cartridge gives what the
    /// address bus reads through `rom_banks`, so tests that read ROM through
    /// the cartridge check what games see.
    #[test]
    fn read_rom_matches_the_banks_the_address_bus_reads() {
        for cartridge_type in SUPPORTED_CARTRIDGE_TYPES {
            let mut rom = build_rom(cartridge_type, 0x06, 0x03);
            for (index, byte) in rom.iter_mut().enumerate().skip(0x150) {
                *byte = (index / ROM_BANK_SIZE) as u8 ^ index as u8;
            }
            let mut cartridge = load_cartridge(rom.clone(), empty_cartridge_effects()).unwrap();
            for register in (0x0000..0x8000).step_by(0x1000) {
                for value in 0..=0xFF {
                    cartridge.write_rom(register, value);
                    let banks = cartridge.rom_banks();
                    for address in [0x0150, 0x3FFF, 0x4000, 0x7FFF] {
                        let bank = banks[usize::from(address >> 14)];
                        assert_eq!(
                            cartridge.read_rom(address),
                            banked_read(&rom, ROM_BANK_SIZE, address, bank)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn reads_the_title_up_to_the_cgb_flag() {
        let mut rom = build_rom(CARTRIDGE_TYPE_ROM_ONLY, 0x00, 0x00);
        rom[TITLE_START_ADDRESS..=TITLE_END_ADDRESS].copy_from_slice(b"POKEMON CRYSTAL\xC0");
        let cartridge = load_cartridge(rom, empty_cartridge_effects()).unwrap();
        assert_eq!(cartridge.title(), "POKEMON CRYSTAL");
    }

    #[test]
    fn reads_the_battery_from_the_cartridge_type() {
        for cartridge_type in SUPPORTED_CARTRIDGE_TYPES {
            let rom = build_rom(cartridge_type, 0x01, 0x03);
            assert_eq!(
                read_header(&rom).unwrap().has_battery,
                cartridge_type_name(cartridge_type).contains("BATTERY"),
                "{}",
                cartridge_type_name(cartridge_type)
            );
        }
    }

    #[test]
    fn reads_the_cgb_flag() {
        let mut rom = build_rom(CARTRIDGE_TYPE_ROM_ONLY, 0x00, 0x00);
        assert!(!read_header(&rom).unwrap().supports_cgb);

        rom[TITLE_END_ADDRESS] = 0x80;
        assert!(read_header(&rom).unwrap().supports_cgb);

        rom[TITLE_END_ADDRESS] = 0xC0;
        assert!(read_header(&rom).unwrap().supports_cgb);
    }

    #[test]
    fn pads_a_small_rom_so_every_rom_address_can_be_read() {
        let mut rom = vec![0; 0x200];
        rom[CARTRIDGE_TYPE_ADDRESS] = CARTRIDGE_TYPE_ROM_ONLY;
        let cartridge = load_cartridge(rom, empty_cartridge_effects()).unwrap();
        assert_eq!(cartridge.read_rom(0x7FFF), 0xFF);
    }

    #[test]
    fn rom_and_ram_needs_no_enable_and_repeats_a_small_ram() {
        let mut cartridge = test_utils::build_cartridge_mapper(CARTRIDGE_TYPE_ROM_RAM, 0x00, 0x01);
        assert_eq!(cartridge.ram().len(), 0x800);
        cartridge.write_ram(0x0000, 0x12);
        assert_eq!(cartridge.read_ram(0x0000), 0x12);
        assert_eq!(cartridge.read_ram(0x0800), 0x12);
        assert_eq!(cartridge.read_ram(0x1800), 0x12);

        cartridge.write_rom(0x0000, 0x00);
        cartridge.write_ram(0x1FFF, 0x34);
        assert_eq!(cartridge.read_ram(0x07FF), 0x34);
        assert_eq!(cartridge.rom_banks(), [0, 1]);
    }

    #[test]
    fn rom_and_ram_without_ram_in_its_header_reads_open_bus() {
        let mut cartridge = test_utils::build_cartridge_mapper(CARTRIDGE_TYPE_ROM_RAM, 0x00, 0x00);
        cartridge.write_ram(0x0000, 0x12);
        assert_eq!(cartridge.read_ram(0x0000), 0xFF);
    }

    #[test]
    fn rom_only_has_no_ram_whatever_its_header_says() {
        let mut cartridge = test_utils::build_cartridge_mapper(CARTRIDGE_TYPE_ROM_ONLY, 0x00, 0x02);
        cartridge.write_ram(0x0000, 0x12);
        assert_eq!(cartridge.read_ram(0x0000), 0xFF);
    }

    #[test]
    fn only_rom_and_ram_with_a_battery_saves_its_ram() {
        assert_eq!(
            test_utils::ram_saves_after_writing(CARTRIDGE_TYPE_ROM_RAM_BATTERY, 0x00),
            1
        );
        assert_eq!(
            test_utils::ram_saves_after_writing(CARTRIDGE_TYPE_ROM_RAM, 0x0A),
            0
        );
    }

    #[test]
    fn reads_open_bus_before_a_rom_is_loaded() {
        let cartridge = empty_cartridge();
        assert_eq!(cartridge.read_rom(0x0100), 0xFF);
        assert_eq!(cartridge.read_ram(0x0100), 0xFF);
    }
}
