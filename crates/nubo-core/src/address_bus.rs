//! The address bus: the Game Boy's 64 KiB memory map. Every CPU memory access
//! goes through here and is routed to the cartridge, video RAM, work RAM, OAM
//! (object attribute memory, the sprite table) or an I/O register. The bus
//! also owns the hardware components and advances them in step with the CPU.
//!
//! # One M-cycle
//!
//! Every M-cycle (machine cycle, see `clock.rs`) moves the hardware on in this
//! order (`sync_with_ppu_lead` and `sync_all_but_timers`):
//!
//! 1. Plain M-cycles that were waiting run first (`deferred_cycles`, see
//!    `bulk_cycles.rs`).
//! 2. The APU starts the M-cycle (`Apu::begin_machine_cycle`).
//! 3. The timers count, and the APU sees the new system counter
//!    (`step_timers`).
//! 4. A speed switch in progress says how many PPU dots the M-cycle has
//!    (`advance_speed_switch`); they count as time since power-on.
//! 5. OAM DMA copies a byte and the PPU runs the dots, plus the ones a timed
//!    register write still owes (`take_cycle_dots`). While nothing can see
//!    it, the PPU is left behind instead (`advance_ppu`).
//! 6. The APU counts the M-cycle (`Apu::step`).
//! 7. The serial port sees the new system counter.
//!
//! Some M-cycles differ, and say how: `sync_until_mid_cycle` stops halfway so
//! a halted DMG can sample interrupts, with the timers running only in the
//! second half; `sync_except_last_dots` leaves the PPU's last dots for later;
//! `sync_while_stopped` (STOP's second byte) holds OAM DMA; `sync_stopped`
//! (STOP mode) only runs the PPU and the APU; and `run_bulk_cycles` runs a
//! whole stretch of M-cycles at once.
//!
//! A new component that has to act every M-cycle goes into its place in
//! `sync_all_but_timers` and into each variant above that runs it. It also
//! has to tell `bulk_cycles_available` how many M-cycles it can run at once
//! (and run them in `run_bulk_cycles`), or return 0 there while it is busy.
//! Building with the `eager` feature runs every M-cycle on its own, with the
//! PPU and APU never left behind; `cargo benchmark --compare` between an eager
//! and a normal build of the same commit shows whether the shortcuts change
//! anything.
//!
//! Other components keep copies of the hardware's modes: CGB mode, double
//! speed and the system counter behind DIV. Only the address bus's setters
//! change them (such as `set_model`, `set_cgb_double_speed` and
//! `reset_divider`), so every copy stays in step.

use crate::address_bus::bank_utils::banked_read;
use crate::address_bus::cartridge::{Cartridge, empty_cartridge};
use crate::address_bus::cheats::CheatState;
use crate::address_bus::constants::ROM_BANK_SIZE;
use crate::address_bus::dma::DmaState;
use crate::address_bus::hdma::HdmaState;
use crate::address_bus::speed_switch::SpeedSwitch;
use crate::apu::Apu;
use crate::clock::dots_per_m_cycle;
use crate::controller::{ControllerMapping, ControllerState};
use crate::cpu::timers::TimerRegisters;
use crate::error::LoadRomError;
use crate::gpu::Gpu;
use crate::joypad::{Joypad, Key};
use crate::model::Model;
use crate::serial::Serial;
use crate::serializable::{FieldWriter, Serializable, read_fields};
use std::fmt;
use std::sync::Arc;

pub use crate::address_bus::cartridge::CartridgeHeader;
pub(crate) use crate::address_bus::cartridge::read_header;
pub use crate::address_bus::effects::{CartridgeEffects, NoSaves, RtcClock};
pub use crate::address_bus::mbc3::RtcState;

/// A component the address bus reads and writes registers or memory of, such
/// as the PPU or the timer. The address bus itself has `peek` and `poke` for
/// the raw view of the memory map, and `cpu_read_byte` and `cpu_write_cycle`
/// for the CPU's accesses.
pub(crate) trait MemoryMapped {
    fn read_byte(&self, address: u16) -> u8;
    fn write_byte(&mut self, address: u16, value: u8);
}

/// How many dots (ticks of the 4 MHz PPU clock) before the end of an M-cycle
/// a mid-cycle sample is taken: half an M-cycle. Only a DMG samples there
/// (a halted one samples interrupts), so the M-cycle lasts four dots.
const MID_CYCLE_PPU_LEAD_DOTS: u8 = 2;
const EXTRA_OAM_SIZE: usize = 0x60;
const ADDRESS_SPACE_SIZE: usize = 0x10000;
/// Room for all eight 4 KiB work RAM banks of the CGB. Echo RAM (0xE000-0xFDFF)
/// mirrors 0xC000-0xDDFF, so `working_ram_index` never goes past bank 7.
const WORKING_RAM_SIZE: usize = 0x8000;
const OAM_RANGE: std::ops::Range<u16> = 0xFE00..0xFEA0;
/// The STAT interrupt's bit in IE and IF.
const STAT_INTERRUPT_BIT: u8 = 0x02;
/// Whether every M-cycle runs on its own, with no M-cycles waiting to run in
/// bulk and the PPU and the APU never left behind (the `eager` feature). Only
/// for checking that those shortcuts change nothing; see the module docs.
const EAGER: bool = cfg!(feature = "eager");

/// Whether a CPU access to `address` reads or changes the PPU's state: VRAM, OAM
/// and the area after it, IF (where the PPU requests interrupts), the LCD and
/// VRAM DMA registers from LCDC (0xFF40) to HDMA5 (0xFF55), the CGB palette
/// registers and OPRI, and IE, which decides whether the PPU's STAT requests
/// matter before anyone reads IF (see `Gpu::run_dots`). The PPU catches up
/// before such an access. Every register whose writes `write_timing.rs` times
/// to the dot must be one of these (its tests check that).
pub(crate) fn reaches_ppu(address: u16) -> bool {
    matches!(
        address,
        0x8000..=0x9FFF
            | 0xFE00..=0xFEFF
            | 0xFF0F
            | 0xFF40..=0xFF55
            | 0xFF68..=0xFF6C
            | 0xFFFF
    )
}

/// Whether a CPU read of `address` only sees memory that the hardware's timing
/// doesn't touch: the cartridge ROM, work RAM and HRAM. Plain M-cycles may
/// wait to run (see `bulk_cycles.rs`) across such reads.
pub(crate) fn reads_plain_memory(address: u16) -> bool {
    matches!(address, 0x0000..=0x7FFF | 0xC000..=0xFDFF | 0xFF80..=0xFFFE)
}

/// Whether a CPU write to `address` only changes memory that the hardware's
/// timing doesn't touch: work RAM and HRAM. Writes to the cartridge ROM area
/// set the bank controller's registers, which may read the clock.
pub(crate) fn writes_plain_memory(address: u16) -> bool {
    matches!(address, 0xC000..=0xFDFF | 0xFF80..=0xFFFE)
}

/// The cartridge ROM as the address bus shares it with the cartridge mapper.
/// Debug output only shows its size, since the cartridge shows the bytes.
#[derive(Clone, Default)]
struct SharedRom(Arc<[u8]>);

impl fmt::Debug for SharedRom {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "SharedRom({} bytes)", self.0.len())
    }
}

/// The most PPU dots since power-on a save state may hold: about 35,000
/// years of emulation, far from where adding to it or working out the next
/// frame boundary (`Emulator::run_frame`) could overflow.
const MAX_ELAPSED_DOTS: u64 = 1 << 62;

/// The dots a save state's `ppu_dot_debt` may hold. Every M-cycle settles the
/// debt, and a register write then leaves at most two dots owed (one landing
/// two dots early) or one dot run ahead (one landing a dot late), see
/// `write_timing.rs`. A value outside could overflow or trip the checks in
/// `take_cycle_dots`.
const PPU_DOT_DEBT_RANGE: std::ops::RangeInclusive<i16> = -1..=2;

/// The memory map and the hardware components connected to it.
///
/// Its `Debug` output is part of the emulator's, which test runners compare to
/// prove two emulators are in exactly the same state (`--verify-boot` in the
/// test ROM runner and `clone_with_rom_matches_a_real_boot`), so it must show
/// every field and stay deterministic: no hand-written `Debug` that leaves
/// fields out, and no collections with an unpredictable order such as a
/// `HashMap`.
#[derive(Debug)]
pub struct AddressBus {
    /// The boot ROM is mapped over the start of the cartridge ROM.
    in_boot_rom: bool,

    boot_rom: Vec<u8>,

    /// Work RAM (0xC000-0xDFFF): bank 0 followed by the switchable banks, 4 KiB
    /// each. The CGB has banks 1-7, the DMG only bank 1.
    working_ram: Box<[u8; WORKING_RAM_SIZE]>,

    /// High RAM (0xFF80-0xFFFE), 127 bytes the CPU can always reach.
    high_ram: [u8; 0x80],

    /// SVBK (0xFF70): selects the CGB work RAM bank at 0xD000.
    svbk: u8,

    cartridge: Cartridge,
    /// The cartridge's ROM, shared with the cartridge, and the banks its
    /// controller has mapped (`Cartridge::rom_banks`), so ROM reads don't have
    /// to go through the cartridge. Updated whenever the controller's
    /// registers change.
    rom: SharedRom,
    rom_banks: [u16; 2],

    /// Flat 64 KiB memory used instead of the memory map when running CPU
    /// tests ("processor test mode"), and `None` otherwise.
    processor_test_ram: Option<Box<[u8; ADDRESS_SPACE_SIZE]>>,

    /// IE (0xFFFF), the interrupt enable register: each bit lets one source
    /// of interrupt requests reach the CPU.
    interrupt_enable: u8,

    timers: TimerRegisters,

    gpu: Gpu,

    joypad: Joypad,

    apu: Apu,

    dma: DmaState,

    hdma: HdmaState,

    serial: Serial,

    cheats: CheatState,

    speed_switch: SpeedSwitch,

    /// Whether the model has CGB hardware (`Model::is_cgb`). The PPU, the
    /// APU and the serial port keep copies, which only `set_model` changes.
    cgb_mode: bool,

    /// The emulated hardware, chosen once when the emulator is made. The PPU
    /// and the APU keep copies, which only `set_model` changes.
    model: Model,

    /// Set when the boot ROM exits, until the CPU has seen it.
    boot_rom_exited: bool,

    /// Memory behind the unusable area after OAM (0xFEA0-0xFEFF), which CGB
    /// revisions before CGB-E back with real, partly mirrored RAM.
    extra_oam: [u8; EXTRA_OAM_SIZE],

    /// The undocumented CGB registers 0xFF72-0xFF75.
    undocumented_registers: [u8; 4],

    /// Dots the PPU still has to run (or, when negative, ran ahead) because a
    /// register write was timed off the M-cycle boundary. Paid back on the
    /// next M-cycle (`take_cycle_dots`); see `write_timing.rs`. Dot counts are
    /// unsigned everywhere else. Usually 0 between calls into the emulator,
    /// but not always: the byte after a HALT that an HBlank DMA block
    /// interrupted is fetched again without an M-cycle, so a write it makes
    /// in its last M-cycle can end a step. Save states keep it, and a loaded
    /// value outside `PPU_DOT_DEBT_RANGE` fails the load.
    ppu_dot_debt: i16,

    /// The source address and byte of the VRAM DMA (HDMA) copy in progress,
    /// which OAM DMA takes over while both run.
    vram_dma_bus: Option<(u16, u8)>,
    /// Set while the CPU is halted by a speed switch and copies an HBlank DMA
    /// block: OAM DMA stands still from the end of the block's first M-cycle,
    /// as after any halt.
    pause_oam_dma_after_cycle: bool,

    /// PPU dots (ticks of the 4 MHz clock at normal speed) since power-on,
    /// which a cartridge clock following [`RtcClock::EmulatedTime`] counts.
    /// Save states keep it, so such a clock goes on where it was, and a
    /// loaded value above `MAX_ELAPSED_DOTS` fails the load.
    elapsed_dots: u64,

    /// Dots that have passed but that the PPU hasn't run yet. Running the PPU in
    /// one go is much faster than dot by dot, so it is left behind while nothing
    /// it does can be seen (what `Gpu::run_dots` returns), and catches up before
    /// anything looks at it or changes it: CPU accesses that reach it
    /// (`reaches_ppu`), OAM DMA, speed switches, entering and leaving STOP mode
    /// and the boot ROM's exit. In STOP mode itself it is left behind like in
    /// any other M-cycle (see `sync_stopped`).
    ppu_owed_dots: u32,
    /// How many dots the PPU may be owed before it could request an interrupt,
    /// deliver a frame or signal HBlank DMA, counted from where it last caught
    /// up. 0 makes it catch up on every M-cycle.
    ppu_quiet_dots: u32,
    /// Set during an M-cycle in which the CPU writes to PPU state, whose dots then
    /// run as they come so the write lands on the right one.
    ppu_eager: bool,

    /// Plain M-cycles (see `sync`) that have passed but that the hardware hasn't
    /// run yet. They run in one go (see `bulk_cycles.rs`) before anything could
    /// tell.
    deferred_cycles: u32,
    /// How many more plain M-cycles may wait, worked out the last time one ran.
    deferral_budget: u32,
    /// The time since power-on, in PPU dots, at which `Emulator::run_frame`
    /// stops. Stretches of M-cycles run in one go end there, so the call
    /// stops at most one CPU step past it. `u64::MAX` outside that call.
    dot_limit: u64,
}

impl AddressBus {
    /// Creates a bus with no cartridge. In `processor_test_mode` all memory
    /// accesses go to a flat 64 KiB RAM, for CPU instruction tests.
    pub(super) fn new(processor_test_mode: bool) -> AddressBus {
        AddressBus {
            in_boot_rom: true,
            boot_rom: vec![0; 0x100],
            working_ram: zeroed_memory(),
            high_ram: [0; 0x80],
            svbk: 0,
            cartridge: empty_cartridge(),
            rom: SharedRom::default(),
            rom_banks: [0, 1],
            processor_test_ram: processor_test_mode.then(zeroed_memory),
            interrupt_enable: 0,
            timers: TimerRegisters::default(),
            gpu: Gpu::new(),
            joypad: Joypad::new(),
            apu: Apu::new(),
            dma: DmaState::default(),
            hdma: HdmaState::new(),
            serial: Serial::default(),
            cheats: CheatState::new(),
            speed_switch: SpeedSwitch::default(),
            cgb_mode: false,
            model: Model::Dmg,
            boot_rom_exited: false,
            extra_oam: [0; EXTRA_OAM_SIZE],
            undocumented_registers: [0; 4],
            ppu_dot_debt: 0,
            vram_dma_bus: None,
            pause_oam_dma_after_cycle: false,
            elapsed_dots: 0,
            ppu_owed_dots: 0,
            ppu_quiet_dots: 0,
            ppu_eager: false,
            deferred_cycles: 0,
            deferral_budget: 0,
            dot_limit: u64::MAX,
        }
    }

    /// Whether the boot ROM is still mapped, before the game starts at 0x0100.
    pub(crate) fn in_boot_rom(&self) -> bool {
        self.in_boot_rom
    }

    /// Maps the boot ROM in or out without running it.
    #[cfg(test)]
    pub(crate) fn set_in_boot_rom(&mut self, in_boot_rom: bool) {
        self.in_boot_rom = in_boot_rom;
    }

    /// Whether the emulated hardware is a CGB (see `cgb_mode`).
    pub(crate) fn cgb_mode(&self) -> bool {
        self.cgb_mode
    }

    /// The emulated hardware.
    pub(crate) fn model(&self) -> Model {
        self.model
    }

    /// Whether the CPU runs at the CGB's double speed.
    pub(crate) fn cgb_double_speed(&self) -> bool {
        self.speed_switch.cgb_double_speed()
    }

    /// Whether a VRAM DMA (HDMA) transfer is running or waiting for HBlanks.
    pub(crate) fn hdma_in_progress(&self) -> bool {
        self.hdma.in_progress()
    }

    /// Puts `address` on the CPU's memory bus for the M-cycle of an access,
    /// where some wave channel glitches of the APU read it.
    pub(crate) fn set_cpu_bus_address(&mut self, address: u16) {
        self.apu.set_bus_address(address);
    }

    /// The most recent complete frame (see `Gpu::completed_frame`). It is
    /// complete even while the PPU owes dots: the PPU is never left behind
    /// past the end of a frame (`Gpu::run_dots`).
    pub(crate) fn completed_frame(&self) -> &[u8] {
        debug_assert!(
            self.ppu_owed_dots <= self.ppu_quiet_dots,
            "the PPU is behind by more than its quiet dots"
        );
        self.gpu.completed_frame()
    }

    /// Whether a frame was finished since the last call.
    pub(crate) fn take_frame_ready(&mut self) -> bool {
        debug_assert!(
            self.ppu_owed_dots <= self.ppu_quiet_dots,
            "the PPU is behind by more than its quiet dots"
        );
        let ready = self.gpu.frame_ready();
        self.gpu.set_frame_ready(false);
        ready
    }

    /// Whether the APU has filled its audio buffers. The APU runs the channels
    /// itself before it fills them (see `apu/catch_up.rs`).
    pub(crate) fn audio_buffers_full(&self) -> bool {
        self.apu.audio_buffers_full()
    }

    /// The left and right audio samples made since the buffers were cleared.
    pub(crate) fn audio_samples(&self) -> (&[f32], &[f32]) {
        (self.apu.left_samples(), self.apu.right_samples())
    }

    /// Moves every sample made so far to the end of `left` and `right`.
    pub(crate) fn take_audio_samples(&mut self, left: &mut Vec<f32>, right: &mut Vec<f32>) {
        self.apu.take_samples(left, right);
    }

    /// Empties the audio buffers for the next batch of samples.
    pub(crate) fn clear_audio_buffers(&mut self) {
        self.apu.clear_audio_buffers();
    }

    /// Sets how many audio samples per second the APU makes.
    pub(crate) fn set_sample_rate(&mut self, sample_rate: u32) {
        self.apu.set_sample_rate(sample_rate);
    }

    /// The cheats the player turned on.
    pub(crate) fn cheats_mut(&mut self) -> &mut CheatState {
        &mut self.cheats
    }

    /// The system counter behind DIV.
    #[cfg(test)]
    pub(crate) fn system_counter(&self) -> u16 {
        self.timers.system_counter()
    }

    /// Whether all memory accesses go to a flat 64 KiB RAM instead of the
    /// memory map, for CPU instruction tests.
    #[inline]
    pub(crate) fn processor_test_mode(&self) -> bool {
        self.processor_test_ram.is_some()
    }

    /// The flat RAM of processor test mode.
    pub(crate) fn processor_test_ram(&self) -> &[u8; ADDRESS_SPACE_SIZE] {
        self.processor_test_ram
            .as_deref()
            .expect("only used in processor test mode")
    }

    /// The flat RAM of processor test mode, to change.
    pub(crate) fn processor_test_ram_mut(&mut self) -> &mut [u8; ADDRESS_SPACE_SIZE] {
        self.processor_test_ram
            .as_deref_mut()
            .expect("only used in processor test mode")
    }

    /// The work RAM bank mapped at 0xD000. Bank 0 can't be selected there, so
    /// writing 0 to SVBK selects bank 1.
    pub(super) fn working_ram_bank(&self) -> u8 {
        if self.cgb_mode {
            let masked_value = self.svbk & 0b111;
            if masked_value == 0 { 1 } else { masked_value }
        } else {
            1
        }
    }

    /// While OAM DMA runs, the CPU can't reach OAM.
    fn is_blocked_by_dma(&self, address: u16) -> bool {
        self.dma.in_progress() && OAM_RANGE.contains(&address)
    }

    /// The index into `working_ram` for a work RAM or echo RAM address.
    fn working_ram_index(&self, address: u16) -> usize {
        let offset = usize::from(address & 0x1FFF);
        if offset <= 0xFFF {
            offset
        } else {
            usize::from(self.working_ram_bank()) * 0x1000 + (offset & 0x0FFF)
        }
    }

    /// Inserts a cartridge and returns its header.
    pub(crate) fn load_rom(
        &mut self,
        rom: Vec<u8>,
        cartridge_effects: Box<dyn CartridgeEffects>,
    ) -> Result<CartridgeHeader, LoadRomError> {
        self.cartridge = cartridge::load_cartridge(rom, cartridge_effects)?;
        self.update_rom_view();
        Ok(self.cartridge.header().clone())
    }

    /// Takes the cartridge's ROM and mapped banks from the cartridge again.
    fn update_rom_view(&mut self) {
        self.rom = SharedRom(Arc::clone(self.cartridge.rom()));
        self.rom_banks = self.cartridge.rom_banks();
    }

    /// Reads plain memory (see `reads_plain_memory`), including active cheats: the
    /// boot ROM where it is mapped, the cartridge ROM, work RAM or HRAM.
    #[inline]
    pub(super) fn read_plain_memory(&self, address: u16) -> u8 {
        let byte = match address {
            0x0000..=0x7FFF if self.in_boot_rom && self.boot_rom_covers(address) => {
                self.boot_rom[usize::from(address)]
            }
            0x0000..=0x7FFF => self.read_cartridge_rom(address),
            0xC000..=0xFDFF => self.working_ram[self.working_ram_index(address)],
            _ => {
                debug_assert!(reads_plain_memory(address));
                self.high_ram[usize::from(address & 0x7F)]
            }
        };
        self.apply_cheats(address, byte)
    }

    /// Writes plain memory (see `writes_plain_memory`): work RAM or HRAM.
    #[inline]
    pub(super) fn write_plain_memory(&mut self, address: u16, value: u8) {
        if let 0xC000..=0xFDFF = address {
            let index = self.working_ram_index(address);
            self.working_ram[index] = value;
        } else {
            debug_assert!(writes_plain_memory(address));
            self.high_ram[usize::from(address & 0x7F)] = value;
        }
    }

    /// Whether the boot ROM, while it is mapped, covers `address`: 0x0000-0x00FF,
    /// and from 0x0200 for as long as Nubo's boot ROMs are (up to 0x08FF for the
    /// DMG one, which real DMGs don't map, and 0x0BFF for the CGB one, where real
    /// CGBs stop at 0x08FF).
    fn boot_rom_covers(&self, address: u16) -> bool {
        address <= 0x00FF || (address >= 0x0200 && usize::from(address) < self.boot_rom.len())
    }

    /// Reads cartridge ROM (0x0000-0x7FFF) from the banks the cartridge has mapped.
    fn read_cartridge_rom(&self, address: u16) -> u8 {
        debug_assert_eq!(
            self.rom_banks,
            self.cartridge.rom_banks(),
            "the mapped banks changed without update_rom_view or a ROM-area write"
        );
        let bank = self.rom_banks[usize::from(address >> 14)];
        banked_read(&self.rom.0, ROM_BANK_SIZE, address, bank)
    }

    /// A copy of the bus and all the hardware behind it, with the cartridge
    /// made from `rom` inserted instead of the current one.
    pub(crate) fn clone_with_rom(
        &self,
        rom: Vec<u8>,
        cartridge_effects: Box<dyn CartridgeEffects>,
    ) -> Result<AddressBus, LoadRomError> {
        let cartridge = cartridge::load_cartridge(rom, cartridge_effects)?;
        let rom = SharedRom(Arc::clone(cartridge.rom()));
        let rom_banks = cartridge.rom_banks();
        Ok(AddressBus {
            in_boot_rom: self.in_boot_rom,
            boot_rom: self.boot_rom.clone(),
            working_ram: self.working_ram.clone(),
            high_ram: self.high_ram,
            svbk: self.svbk,
            cartridge,
            rom,
            rom_banks,
            processor_test_ram: self.processor_test_ram.clone(),
            interrupt_enable: self.interrupt_enable,
            timers: self.timers.clone(),
            gpu: self.gpu.clone(),
            joypad: self.joypad.clone(),
            apu: self.apu.clone(),
            dma: self.dma.clone(),
            hdma: self.hdma.clone(),
            serial: self.serial.clone(),
            cheats: self.cheats.clone(),
            speed_switch: self.speed_switch.clone(),
            cgb_mode: self.cgb_mode,
            model: self.model,
            boot_rom_exited: self.boot_rom_exited,
            extra_oam: self.extra_oam,
            undocumented_registers: self.undocumented_registers,
            ppu_dot_debt: self.ppu_dot_debt,
            vram_dma_bus: self.vram_dma_bus,
            pause_oam_dma_after_cycle: self.pause_oam_dma_after_cycle,
            elapsed_dots: self.elapsed_dots,
            ppu_owed_dots: self.ppu_owed_dots,
            ppu_quiet_dots: self.ppu_quiet_dots,
            ppu_eager: self.ppu_eager,
            deferred_cycles: self.deferred_cycles,
            deferral_budget: self.deferral_budget,
            dot_limit: self.dot_limit,
        })
    }

    /// Passes battery-backed RAM and the clock the game changed since the last
    /// call on to the frontend to save (see `Cartridge::report_save_changes`).
    pub(crate) fn report_save_changes(&mut self) {
        self.cartridge.report_save_changes();
    }

    pub(crate) fn cartridge_ram(&self) -> &[u8] {
        self.cartridge.ram()
    }

    pub(crate) fn set_cartridge_ram(&mut self, buffer: Vec<u8>) {
        self.cartridge.set_ram(buffer);
    }

    /// The cartridge, to read and set its clock and read its rumble motor.
    pub(crate) fn cartridge_mut(&mut self) -> &mut Cartridge {
        &mut self.cartridge
    }

    /// All of work RAM: bank 0 followed by banks 1-7, 4 KiB each. Only banks
    /// 0 and 1 exist on a DMG, or on a CGB running a game for the DMG.
    pub(crate) fn working_ram(&self) -> &[u8] {
        self.working_ram.as_slice()
    }

    /// High RAM (0xFF80-0xFFFE).
    pub(crate) fn high_ram(&self) -> &[u8] {
        &self.high_ram[..0x7F]
    }

    /// Video RAM and OAM, after catching the PPU up so an HBlank DMA it owes
    /// has copied its bytes.
    pub(crate) fn video_memory(&mut self) -> (&[u8], &[u8]) {
        self.catch_up_ppu();
        self.gpu.video_memory()
    }

    /// Advances every component except the CPU by one M-cycle (four clock
    /// ticks, or two in CGB double speed mode). While nothing can tell the
    /// difference, the M-cycle only waits to run in one go with the ones
    /// around it (see `bulk_cycles.rs`).
    #[inline]
    pub(super) fn sync(&mut self) {
        if self.deferral_budget > 0 {
            self.deferral_budget -= 1;
            self.deferred_cycles += 1;
        } else {
            self.sync_now();
        }
    }

    /// `sync` for an M-cycle that can't wait: runs it and works out how many of
    /// the ones after it may.
    #[inline(never)]
    fn sync_now(&mut self) {
        self.sync_with_ppu_lead(0);
        self.deferral_budget = self.deferrable_cycles();
    }

    /// Like [`AddressBus::sync`], but leaves the last `dots` PPU dots for
    /// [`AddressBus::finish_sync`], so something can happen in between.
    pub(crate) fn sync_except_last_dots(&mut self, dots: u8) {
        self.sync_with_ppu_lead(dots);
    }

    /// Runs the `dots` PPU dots that [`AddressBus::sync_except_last_dots`] left.
    pub(crate) fn finish_sync(&mut self, dots: u8) {
        self.advance_ppu(dots);
    }

    /// Like [`AddressBus::sync`], but stops the PPU `lead` dots short of the
    /// end of the M-cycle. Dots owed from an earlier write are run first.
    fn sync_with_ppu_lead(&mut self, lead: u8) {
        self.run_deferred_cycles_before_change();
        self.apu.begin_machine_cycle();
        self.step_timers_unless_held();
        self.sync_all_but_timers(lead);
    }

    /// Runs everything but the timers for one M-cycle, with the PPU stopping
    /// `lead` dots short of its end.
    fn sync_all_but_timers(&mut self, lead: u8) {
        let cycle = self.advance_speed_switch();
        self.count_elapsed_dots(cycle.dots);
        let dots = self.take_cycle_dots(cycle.dots, lead);
        if self.dma.is_idle() && self.gpu.oam_dma_access_settled() {
            self.advance_ppu(dots);
        } else {
            self.catch_up_ppu();
            let access_before_step = self.ppu_oam_dma_access();
            if !cycle.frozen {
                self.dma_step();
            }
            // The PPU sees OAM DMA hold OAM from half an M-cycle into the M-cycles
            // in which it copies a byte (Gambatte's oamdma late_sp tests).
            let access_delay = dots_per_m_cycle(self.speed_switch.cgb_double_speed()) / 2;
            self.gpu
                .set_oam_dma_access(access_before_step, access_delay);
            self.gpu.step_dots(dots, &mut self.hdma);
            self.ppu_quiet_dots = 0;
        }
        self.step_apu();
        self.serial.step(self.timers.system_counter());
    }

    /// Counts the M-cycle in the APU, which runs it later (see
    /// `apu/catch_up.rs`), or at once in an eager build.
    fn step_apu(&mut self) {
        self.apu.step();
        if EAGER {
            self.apu.catch_up();
        }
    }

    /// Runs the M-cycle in which STOP reads its second byte. The CPU clock
    /// is stopping, so OAM DMA stands still while the rest carries on.
    pub(crate) fn sync_while_stopped(&mut self) {
        self.run_deferred_cycles_before_change();
        self.catch_up_ppu_before_change();
        self.apu.begin_machine_cycle();
        self.step_timers();
        let cycle = self.advance_speed_switch();
        self.count_elapsed_dots(cycle.dots);
        self.gpu.set_oam_dma_access(self.ppu_oam_dma_access(), 0);
        let dots = self.take_cycle_dots(cycle.dots, 0);
        self.gpu.step_dots(dots, &mut self.hdma);
        self.step_apu();
        self.serial.step(self.timers.system_counter());
    }

    /// The PPU dots to run in an M-cycle of `dots` dots in which the PPU stops
    /// `lead` dots short of the end: plus the dots an earlier register write
    /// still owes, or minus those it ran ahead (`ppu_dot_debt`), which this
    /// pays back.
    fn take_cycle_dots(&mut self, dots: u8, lead: u8) -> u8 {
        let dots = i16::from(dots) + std::mem::take(&mut self.ppu_dot_debt) - i16::from(lead);
        // A write runs the PPU at most a dot or two ahead, and the M-cycle after it is a full one.
        debug_assert!(dots >= 0, "the PPU ran {} dots too far ahead", -dots);
        u8::try_from(dots).unwrap_or(0)
    }

    /// PPU dots (ticks of the 4 MHz clock at normal speed) since power-on, not
    /// counting the M-cycles still waiting to run (see `bulk_cycles.rs`).
    pub(crate) fn elapsed_dots(&self) -> u64 {
        self.elapsed_dots
    }

    /// [`AddressBus::elapsed_dots`] plus the dots of the M-cycles still
    /// waiting to run.
    pub(crate) fn elapsed_dots_including_waiting(&self) -> u64 {
        self.elapsed_dots + u64::from(self.deferred_cycles) * u64::from(self.dots_per_cycle())
    }

    /// Makes stretches of M-cycles run in one go end at `dot_limit` dots
    /// since power-on (`u64::MAX` for no limit).
    pub(crate) fn set_dot_limit(&mut self, dot_limit: u64) {
        self.dot_limit = dot_limit;
    }

    /// How many whole M-cycles fit before the dot limit.
    fn cycles_before_dot_limit(&self) -> u32 {
        let dots = self
            .dot_limit
            .saturating_sub(self.elapsed_dots_including_waiting());
        u32::try_from(dots / u64::from(self.dots_per_cycle())).unwrap_or(u32::MAX)
    }

    /// Adds the dots of an M-cycle to the time since power-on.
    fn count_elapsed_dots(&mut self, dots: u8) {
        self.elapsed_dots += u64::from(dots);
    }

    /// Advances the timers by one M-cycle, unless STOP holds them.
    fn step_timers_unless_held(&mut self) {
        if !self.take_timer_hold() {
            self.step_timers();
        }
    }

    /// Advances the timers by one M-cycle and lets the APU see the new DIV.
    fn step_timers(&mut self) {
        let previous_system_counter = self.timers.system_counter();
        self.timers.step();
        self.apu.system_counter_changed(
            previous_system_counter,
            self.timers.system_counter(),
            false,
        );
    }

    /// Runs an M-cycle up to its middle, where the CPU can sample state (a
    /// halted DMG samples interrupts there). Finish it with [`AddressBus::finish_mid_cycle_sync`].
    /// The timer raises its interrupt late in the M-cycle, so it only runs in
    /// the second half: a halted DMG sees a timer interrupt one M-cycle later
    /// than a PPU interrupt raised at the same time (Gambatte's tima tests).
    pub(crate) fn sync_until_mid_cycle(&mut self) {
        self.run_deferred_cycles_before_change();
        self.apu.begin_machine_cycle();
        self.sync_all_but_timers(MID_CYCLE_PPU_LEAD_DOTS);
    }

    /// Runs the timers and the PPU dots left over by
    /// [`AddressBus::sync_until_mid_cycle`].
    pub(crate) fn finish_mid_cycle_sync(&mut self) {
        self.step_timers_unless_held();
        self.advance_ppu(MID_CYCLE_PPU_LEAD_DOTS);
    }

    /// Runs `dots` PPU dots, or (while the PPU is left behind) adds them to the
    /// dots it is owed, letting it catch up once it could do something the rest
    /// of the hardware sees.
    fn advance_ppu(&mut self, dots: u8) {
        if self.ppu_eager || dots == 0 || EAGER {
            self.catch_up_ppu();
            self.gpu.step_dots(dots, &mut self.hdma);
            self.ppu_quiet_dots = 0;
            return;
        }
        self.ppu_owed_dots += u32::from(dots);
        if self.ppu_owed_dots > self.ppu_quiet_dots {
            let stat_interrupt_enabled = self.stat_interrupt_enabled();
            self.ppu_quiet_dots = self.gpu.run_dots(
                std::mem::take(&mut self.ppu_owed_dots),
                &mut self.hdma,
                stat_interrupt_enabled,
            );
        }
    }

    /// Runs the dots the PPU is owed, so its state is up to date.
    pub(crate) fn catch_up_ppu(&mut self) {
        self.run_deferred_cycles();
        if self.ppu_owed_dots > 0 {
            let owed = std::mem::take(&mut self.ppu_owed_dots);
            let stat_interrupt_enabled = self.stat_interrupt_enabled();
            self.gpu
                .run_dots(owed, &mut self.hdma, stat_interrupt_enabled);
            self.ppu_quiet_dots = self.ppu_quiet_dots.saturating_sub(owed);
        }
    }

    /// IE (0xFFFF), the interrupt enable register.
    pub(crate) fn interrupt_enable(&self) -> u8 {
        self.interrupt_enable
    }

    /// Whether IE enables the STAT interrupt.
    fn stat_interrupt_enabled(&self) -> bool {
        self.interrupt_enable & STAT_INTERRUPT_BIT != 0
    }

    /// Lets the PPU catch up before something changes its state, after which
    /// the next M-cycle works out again how long it may be left behind.
    pub(crate) fn catch_up_ppu_before_change(&mut self) {
        self.run_deferred_cycles_before_change();
        self.catch_up_ppu();
        self.ppu_quiet_dots = 0;
    }

    /// Resets DIV (0xFF04) and lets the APU and serial port see the change,
    /// since a DIV bit clocks the frame sequencer and the serial clock.
    pub(crate) fn reset_divider(&mut self, during_div_write: bool) {
        let previous_system_counter = self.timers.system_counter();
        self.timers.reset_divider();
        self.system_counter_reset(previous_system_counter, during_div_write);
    }

    /// Resets DIV as STOP does when it switches speed, which may make TIMA
    /// count once more first (`TimerRegisters::reset_divider_for_speed_switch`).
    pub(crate) fn reset_divider_for_speed_switch(&mut self) {
        let previous_system_counter = self.timers.system_counter();
        self.timers.reset_divider_for_speed_switch(self.model);
        self.system_counter_reset(previous_system_counter, false);
    }

    /// Lets the APU and the serial port see the system counter fall from
    /// `previous_system_counter` to 0.
    fn system_counter_reset(&mut self, previous_system_counter: u16, during_div_write: bool) {
        self.apu.system_counter_changed(
            previous_system_counter,
            self.timers.system_counter(),
            during_div_write,
        );
        self.serial.step(self.timers.system_counter());
    }

    /// Corrupts OAM as a CPU write, or a 16-bit increment or decrement of a
    /// register pointing into OAM, does during OAM scan on monochrome models
    /// (the "OAM bug", see `gpu/oam_bug.rs`).
    pub(super) fn trigger_oam_bug_write(&mut self) {
        self.catch_up_ppu_before_change();
        self.gpu.trigger_oam_bug_write();
    }

    /// Corrupts OAM as a CPU read during OAM scan does (the "OAM bug").
    pub(super) fn trigger_oam_bug_read(&mut self) {
        self.catch_up_ppu_before_change();
        self.gpu.trigger_oam_bug_read();
    }

    /// Corrupts OAM as an instruction that reads through a register pointing
    /// into OAM and increments or decrements it in the same M-cycle, such as
    /// `LD A, (HL+)`, does during OAM scan (the "OAM bug").
    pub(super) fn trigger_oam_bug_mixed(&mut self) {
        self.catch_up_ppu_before_change();
        self.gpu.trigger_oam_bug_mixed();
    }

    /// Selects the emulated hardware and configures every component for it.
    pub(crate) fn set_model(&mut self, model: Model) {
        self.model = model;
        self.gpu.set_model(model);
        self.apu.set_model(model);
        self.set_cgb_mode(model.is_cgb());

        // Registers the boot ROM leaves alone power on as 0xFF on monochrome
        // models and 0x00 on the CGB (SameBoy; GBMicrotest's poweron tests).
        let power_on_value = if model.is_cgb() { 0x00 } else { 0xFF };
        self.working_ram = power_on_work_ram(model);
        self.dma.set_power_on_register(power_on_value);
        let palettes = self.gpu.palettes_mut();
        palettes.set_obp0(power_on_value);
        palettes.set_obp1(power_on_value);
    }

    fn set_cgb_mode(&mut self, value: bool) {
        self.cgb_mode = value;
        self.serial.set_cgb_mode(value);
    }

    pub(crate) fn cartridge(&self) -> &Cartridge {
        &self.cartridge
    }

    pub(crate) fn press_key(&mut self, key: Key) {
        self.joypad.press_key(key);
    }

    pub(crate) fn release_key(&mut self, key: Key) {
        self.joypad.release_key(key);
    }

    pub(crate) fn set_controller_state(&mut self, state: &ControllerState) {
        self.joypad.set_controller_state(state);
    }

    pub(crate) fn set_controller_mapping(&mut self, mapping: ControllerMapping) {
        self.joypad.set_controller_mapping(mapping);
    }

    /// Reads from the unusable area are blocked whenever OAM reads are.
    fn is_unusable_oam_read_blocked(&self) -> bool {
        self.dma.in_progress() || self.gpu.oam_read_blocked()
    }

    /// Where an address in 0xFEA0-0xFEFF lands in `extra_oam`, or `None` on
    /// models without memory there. CGB-D maps all of 0xFEC0-0xFEFF onto
    /// 0xFEF0-0xFEFF; older CGB revisions have 8 bytes per 32-byte row,
    /// repeated four times.
    fn extra_oam_index(&self, address: u16) -> Option<usize> {
        let offset = address & 0xFF;
        match self.model {
            Model::CgbD => {
                let mirrored_offset = if offset >= 0xC0 {
                    offset | 0xF0
                } else {
                    offset
                };
                Some(usize::from(mirrored_offset) - 0xA0)
            }
            Model::Cgb0 | Model::CgbA | Model::CgbB | Model::CgbC => {
                Some(usize::from(offset & !0x18) - 0xA0)
            }
            _ => None,
        }
    }

    /// Reads the unusable area after OAM (0xFEA0-0xFEFF). DMG models read 0,
    /// CGB-E and AGB a pattern made from the address, and older CGB revisions
    /// the RAM behind it.
    fn read_unusable_oam(&self, address: u16) -> u8 {
        if self.is_unusable_oam_read_blocked() {
            return 0xFF;
        }
        let offset = (address & 0xFF) as u8;
        match self.extra_oam_index(address) {
            Some(index) => self.extra_oam[index],
            None if matches!(self.model, Model::CgbE | Model::Agb) => {
                (offset & 0xF0) | (offset >> 4)
            }
            None => 0x00,
        }
    }

    /// Writes the unusable area after OAM (0xFEA0-0xFEFF), which only older
    /// CGB revisions store (see `extra_oam_index`). Blocked whenever OAM
    /// writes are.
    fn write_unusable_oam(&mut self, address: u16, value: u8) {
        if self.gpu.oam_write_blocked() || self.dma.in_progress() {
            return;
        }
        if let Some(index) = self.extra_oam_index(address) {
            self.extra_oam[index] = value;
        }
    }
}

impl AddressBus {
    /// Reads `address` as the memory map holds it right now, including the
    /// CPU's VRAM and OAM blocking and active cheats, but none of what a CPU
    /// read does around that (`cpu_read_byte`): M-cycles waiting to run and
    /// the PPU's owed dots aren't caught up first, and OAM DMA's bus conflicts
    /// don't apply. So callers must have caught the PPU up before peeking at
    /// anything that reaches it (`reaches_ppu`), which debug builds check.
    pub(crate) fn peek(&self, address: u16) -> u8 {
        debug_assert!(
            self.ppu_owed_dots == 0 || !reaches_ppu(address),
            "{address:#06X} is read while the PPU is behind"
        );
        if let Some(processor_test_ram) = &self.processor_test_ram {
            processor_test_ram[usize::from(address)]
        } else if self.is_blocked_by_dma(address) {
            0xFF
        } else if reads_plain_memory(address) {
            self.read_plain_memory(address)
        } else {
            let byte = match address {
                0x8000..=0x9FFF if self.gpu.vram_read_blocked() => 0xFF,
                0x8000..=0x9FFF => self.gpu.read_byte(address),
                0xA000..=0xBFFF => self.cartridge.read_ram(address & 0x1FFF),
                0xFE00..=0xFE9F if self.gpu.oam_read_blocked() => 0xFF,
                0xFE00..=0xFE9F => self.gpu.read_byte(address),
                0xFEA0..=0xFEFF => self.read_unusable_oam(address),
                0xFFFF => self.interrupt_enable,
                _ => self.read_io_register(address),
            };
            self.apply_cheats(address, byte)
        }
    }

    /// Writes `address` in the memory map as it is right now, without what a
    /// CPU write does around that (`cpu_write_cycle`): no M-cycle runs, the
    /// write lands on no particular dot and OAM DMA's bus conflicts don't
    /// apply. Like `peek`, it needs the PPU caught up for anything that
    /// reaches it, which debug builds check.
    pub(crate) fn poke(&mut self, address: u16, value: u8) {
        debug_assert!(
            self.ppu_owed_dots == 0 || !reaches_ppu(address),
            "{address:#06X} is written while the PPU is behind"
        );
        if let Some(processor_test_ram) = &mut self.processor_test_ram {
            processor_test_ram[usize::from(address)] = value;
        } else if writes_plain_memory(address) {
            self.write_plain_memory(address, value);
        } else if !self.is_blocked_by_dma(address) {
            match address {
                0x0000..=0x7FFF => {
                    self.cartridge
                        .write_rom_at(address, value, self.elapsed_dots);
                    self.rom_banks = self.cartridge.rom_banks();
                }
                0x8000..=0x9FFF if self.gpu.vram_write_blocked() => (),
                0x8000..=0x9FFF => self.gpu.write_byte(address, value),
                0xA000..=0xBFFF => {
                    self.cartridge
                        .write_ram_at(address & 0x1FFF, value, self.elapsed_dots);
                }
                0xFE00..=0xFE9F if self.gpu.oam_write_blocked() => (),
                0xFE00..=0xFE9F => self.gpu.write_byte(address, value),
                0xFEA0..=0xFEFF => self.write_unusable_oam(address, value),
                0xFFFF => self.interrupt_enable = value,
                _ => self.write_io_register(address, value),
            }
        }
    }
}

/// The contents of work RAM at power-on. Real RAM chips come up holding noise that programs
/// can't rely on but can see. A monochrome model's RAM comes up in 256-byte blocks whose bits are
/// mostly set or mostly clear, alternately (SameBoy); this uses the typical value of each block,
/// which is what Gambatte's oamdma busyread and srcFF00 tests read back on a DMG. CGB RAM is plain
/// noise (Bully's initram test checks it isn't blank). The noise is a fixed pseudo-random pattern
/// so every run is the same.
fn power_on_work_ram(model: Model) -> Box<[u8; WORKING_RAM_SIZE]> {
    let mut memory = zeroed_memory::<WORKING_RAM_SIZE>();
    let mut state: u32 = 0x2545_F491;
    for (offset, byte) in memory.iter_mut().enumerate() {
        *byte = if model.is_cgb() {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state.to_le_bytes()[0]
        } else if offset & 0x100 == 0 {
            0xFF
        } else {
            0x00
        };
    }
    memory
}

/// A zero-filled block of memory, allocated directly on the heap so it never
/// sits on the stack (64 KiB arrays overflow it in debug builds).
fn zeroed_memory<const SIZE: usize>() -> Box<[u8; SIZE]> {
    vec![0; SIZE]
        .into_boxed_slice()
        .try_into()
        .expect("the vector has exactly SIZE bytes")
}

/// Save states hold every field of the address bus except these:
///
/// - `boot_rom`, `cgb_mode` and `model` come from the model, which the save
///   state's header checks.
/// - `rom` and `rom_banks` are views of the cartridge mapper's ROM and banks,
///   set up again after loading (`update_rom_view`).
/// - `processor_test_ram` only exists for CPU tests.
/// - `cheats` are the player's settings, not the console's.
/// - `ppu_owed_dots`: the PPU is saved as if it had caught up, which changes
///   nothing outside it (see `Gpu::run_dots`).
/// - `ppu_quiet_dots` and `deferral_budget` say how long the PPU and the plain
///   M-cycles may wait; 0, which loading sets, makes the next M-cycle work
///   them out again.
/// - `deferred_cycles`, `boot_rom_exited`, `vram_dma_bus`,
///   `pause_oam_dma_after_cycle` and `ppu_eager` are always zero, `None` or
///   false between calls into the emulator, and `dot_limit` is `u64::MAX`
///   (checked in debug builds).
impl AddressBus {
    /// Writes the components on the address bus to `fields`, the top level of
    /// a save state next to the CPU, one field each and named after the
    /// hardware: `bus` (the address bus's own memory and registers),
    /// `cartridge`, `timers`, `ppu`, `apu`, `oam_dma`, `vram_dma`, `serial`,
    /// `speed_switch` and `joypad`. These names are permanent (see
    /// `serializable.rs`).
    pub(crate) fn write_save_state_components(
        &self,
        fields: &mut FieldWriter,
    ) -> std::io::Result<()> {
        debug_assert_eq!(
            self.deferred_cycles, 0,
            "the waiting M-cycles run before a call returns"
        );
        debug_assert!(!self.boot_rom_exited, "the CPU takes it in the same step");
        debug_assert_eq!(self.vram_dma_bus, None, "only set during a block copy");
        debug_assert!(
            !self.pause_oam_dma_after_cycle,
            "only set during a block copy"
        );
        debug_assert!(!self.ppu_eager, "only set during a write's M-cycle");
        debug_assert_eq!(self.dot_limit, u64::MAX, "only set during run_frame");
        fields.field_with(
            "bus",
            || String::from("fields"),
            |writer| {
                let mut bus = FieldWriter::new(writer);
                bus.field("in_boot_rom", &self.in_boot_rom)?;
                bus.field("work_ram", &*self.working_ram)?;
                bus.field("high_ram", &self.high_ram)?;
                bus.field("svbk", &self.svbk)?;
                bus.field("interrupt_enable", &self.interrupt_enable)?;
                bus.field("undocumented_registers", &self.undocumented_registers)?;
                bus.field("extra_oam", &self.extra_oam)?;
                bus.field("elapsed_dots", &self.elapsed_dots)?;
                bus.field("ppu_dot_debt", &self.ppu_dot_debt)?;
                bus.finish()
            },
        )?;
        fields.field("cartridge", &self.cartridge)?;
        fields.field("timers", &self.timers)?;
        if self.ppu_owed_dots > 0 {
            // Save states hold the PPU as it would be if it had kept up. Catching up
            // changes nothing outside it (see `Gpu::run_dots`), not even the HBlank
            // signals VRAM DMA gets, so a copy of VRAM DMA can take them.
            let mut gpu = self.gpu.clone();
            gpu.run_dots(self.ppu_owed_dots, &mut self.hdma.clone(), true);
            fields.field("ppu", &gpu)?;
        } else {
            fields.field("ppu", &self.gpu)?;
        }
        fields.field("apu", &self.apu)?;
        fields.field("oam_dma", &self.dma)?;
        fields.field("vram_dma", &self.hdma)?;
        fields.field("serial", &self.serial)?;
        fields.field("speed_switch", &self.speed_switch)?;
        fields.field("joypad", &self.joypad)
    }

    /// Reads the component `name` of a save state, one that
    /// `write_save_state_components` writes, and returns whether it is one.
    pub(crate) fn read_save_state_component(
        &mut self,
        name: &str,
        bytes: &mut &[u8],
    ) -> std::io::Result<bool> {
        match name {
            "bus" => read_fields(bytes, |name, bytes| {
                match name {
                    "in_boot_rom" => self.in_boot_rom.deserialize(bytes)?,
                    "work_ram" => self.working_ram.deserialize(bytes)?,
                    "high_ram" => self.high_ram.deserialize(bytes)?,
                    "svbk" => self.svbk.deserialize(bytes)?,
                    "interrupt_enable" => self.interrupt_enable.deserialize(bytes)?,
                    "undocumented_registers" => self.undocumented_registers.deserialize(bytes)?,
                    "extra_oam" => self.extra_oam.deserialize(bytes)?,
                    "elapsed_dots" => {
                        self.elapsed_dots.deserialize(bytes)?;
                        if self.elapsed_dots > MAX_ELAPSED_DOTS {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "time since power-on out of range",
                            ));
                        }
                    }
                    "ppu_dot_debt" => {
                        self.ppu_dot_debt.deserialize(bytes)?;
                        if !PPU_DOT_DEBT_RANGE.contains(&self.ppu_dot_debt) {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "PPU dot debt out of range",
                            ));
                        }
                    }
                    _ => return Ok(false),
                }
                Ok(true)
            })?,
            "cartridge" => self.cartridge.deserialize(bytes)?,
            "timers" => self.timers.deserialize(bytes)?,
            "ppu" => self.gpu.deserialize(bytes)?,
            "apu" => self.apu.deserialize(bytes)?,
            "oam_dma" => self.dma.deserialize(bytes)?,
            "vram_dma" => self.hdma.deserialize(bytes)?,
            "serial" => self.serial.deserialize(bytes)?,
            "speed_switch" => self.speed_switch.deserialize(bytes)?,
            "joypad" => self.joypad.deserialize(bytes)?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Sets up what a loaded save state doesn't hold: the views of the
    /// cartridge's ROM, and the PPU and M-cycles caught up (see above).
    pub(crate) fn finish_loading_save_state(&mut self) {
        self.cartridge.finish_loading_save_state(self.elapsed_dots);
        self.update_rom_view();
        self.ppu_owed_dots = 0;
        self.ppu_quiet_dots = 0;
        self.deferred_cycles = 0;
        self.deferral_budget = 0;
    }
}

#[cfg(test)]
pub(crate) mod test_utils {
    use crate::address_bus::AddressBus;
    use crate::address_bus::cartridge::*;
    use crate::address_bus::constants::*;

    /// A blank ROM whose header names the cartridge type, ROM size and RAM
    /// size given by their header codes, with as many 16 KiB banks as the ROM
    /// size code says.
    pub(crate) fn build_rom(cartridge_type: u8, rom_size_index: u8, ram_size_index: u8) -> Vec<u8> {
        let number_of_banks = usize::from(rom_bank_count(rom_size_index));
        let mut rom_buffer = vec![0; ROM_BANK_SIZE * number_of_banks];
        rom_buffer[CARTRIDGE_TYPE_ADDRESS] = cartridge_type;
        rom_buffer[ROM_SIZE_ADDRESS] = rom_size_index;
        rom_buffer[RAM_SIZE_ADDRESS] = ram_size_index;
        rom_buffer
    }

    /// An address bus with no cartridge, outside processor test mode.
    pub(crate) fn initialize_test_address_bus() -> AddressBus {
        AddressBus::new(false)
    }
}

#[cfg(test)]
mod tests;

mod bank_utils;
mod boot_rom;
mod bulk_cycles;
mod cartridge;
pub(crate) mod cheats;
pub(crate) mod constants;
mod dma;
pub(crate) mod effects;
pub(crate) mod hdma;
mod huc1;
mod io_registers;
mod mbc1;
mod mbc2;
mod mbc3;
mod mbc5;
mod speed_switch;
mod stop_mode;
mod write_timing;
