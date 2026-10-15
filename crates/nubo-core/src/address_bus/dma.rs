/*
    The bus conflicts between OAM DMA and the CPU are ported from SameBoy's memory.c
    (https://github.com/LIJI32/SameBoy, Expat License, see the notice in apu.rs), adjusted where
    Gambatte's hardware tests (gambatte/oamdma in the test ROM collection) show otherwise.
*/

//! OAM DMA: the transfer started by writing to the DMA register (0xFF46) that
//! copies 160 bytes into OAM (object attribute memory, the sprite table at
//! 0xFE00-0xFE9F), one byte per M-cycle (an M-cycle is four clock ticks).
//!
//! The transfer occupies the memory bus its source is on, so while it runs
//! the CPU can't use that bus normally: its reads see the byte the DMA just
//! copied, and its writes go astray in ways that differ between models.
//!
//! While it copies, the transfer also holds OAM from the PPU, as in SameBoy:
//! OAM scan re-checks the last Y and X it read, mode 3's object reads see the
//! bytes being copied, and STAT shows mode 0 instead of mode 2 (see
//! `ppu_oam_dma_access`).

use crate::address_bus::{AddressBus, MemoryMapped, reaches_ppu, reads_plain_memory};
use crate::apu::Apu;
use crate::gpu::OamDmaAccess;
use crate::model::Model;
use nubo_serializable_derive::Serializable;

/// The state of the OAM DMA transfer. The rest of the address bus only sees
/// it through the methods below; the transfer itself runs in this file.
#[derive(Clone, Debug, Default, Serializable)]
pub(super) struct DmaState {
    /// The address the running transfer copies from.
    source_address: u16,
    /// How many bytes the running transfer has copied so far.
    offset: u8,
    /// A transfer runs: from when its start delay runs out until the M-cycle
    /// after the last byte is copied.
    in_progress: bool,
    /// The value last written to the DMA register: the high byte of the source
    /// address of the next transfer.
    register: u8,
    /// M-cycles left until a requested transfer begins, or 0 if none is
    /// waiting.
    start_delay: u8,
    /// The CPU is halted, which stops the transfer until it wakes up.
    paused_by_halt: bool,
}

/// How many bytes one OAM DMA transfer copies: the whole of OAM.
pub(super) const DMA_TRANSFER_BYTES: u8 = 160;
const DMA_START_DELAY: u8 = 2;
const ECHO_RAM_START: u16 = 0xE000;
const ECHO_RAM_OFFSET: u16 = 0x2000;
const VIDEO_RAM_START: u16 = 0x8000;
const CARTRIDGE_RAM_START: u16 = 0xA000;
const WORK_RAM_START: u16 = 0xC000;
const OAM_START: u16 = 0xFE00;
/// The address bit that picks the switchable work RAM bank (0xD000-0xDFFF)
/// over bank 0 (0xC000-0xCFFF).
const WORK_RAM_BANK_BIT: u16 = 0x1000;

/// The separate memory buses of the Game Boy. Each can carry one access per
/// M-cycle, so the CPU and OAM DMA only collide when they use the same one.
#[derive(PartialEq, Eq)]
enum Bus {
    /// The cartridge (ROM and cartridge RAM), and on monochrome models also
    /// work RAM.
    External,
    /// Video RAM (0x8000-0x9FFF).
    Video,
    /// Work RAM (0xC000 and up), which the CGB gives its own bus.
    WorkRam,
}

/// What memory an OAM DMA transfer copies from, which decides what a CPU
/// write that collides with it does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    /// ROM or cartridge RAM, and on the CGB also echo RAM.
    Cartridge,
    VideoRam,
    WorkRam,
}

/// Where a CPU access that collides with OAM DMA ends up.
enum Conflict {
    /// The CPU reaches this address instead of the one it asked for.
    Redirect(u16),
    /// A CGB read returns 0xFF and a write is lost.
    OpenBus,
}

impl DmaState {
    /// Handles a write to the DMA register. The transfer begins two M-cycles
    /// later; a transfer already running keeps going until then, but takes
    /// the high byte of its source from the register at once, so it already
    /// occupies the new source's bus (Gambatte's oamdma
    /// src8000_srcchange0000_busyinc tests).
    pub(super) fn start_dma(&mut self, source_high_byte: u8) {
        self.register = source_high_byte;
        self.start_delay = DMA_START_DELAY;
        if self.in_progress {
            self.source_address = u16::from(source_high_byte) << 8;
        }
    }

    /// Whether no transfer is requested or running, from the write to the DMA
    /// register until the M-cycle after the last byte is copied.
    pub(super) fn is_idle(&self) -> bool {
        self.start_delay == 0 && !self.in_progress
    }

    /// Whether a transfer runs (see `in_progress`), which keeps the CPU from
    /// OAM.
    pub(super) fn in_progress(&self) -> bool {
        self.in_progress
    }

    /// Sets the DMA register's value at power-on, without starting a transfer.
    pub(super) fn set_power_on_register(&mut self, value: u8) {
        self.register = value;
    }

    /// The value of the DMA register as the CPU reads it.
    pub(super) fn register(&self) -> u8 {
        self.register
    }

    /// Counts down the delay of a requested transfer and starts it when the
    /// delay runs out.
    fn advance_start_delay(&mut self) {
        if self.start_delay > 0 {
            self.start_delay -= 1;
            if self.start_delay == 0 {
                self.source_address = u16::from(self.register) << 8;
                self.offset = 0;
                self.in_progress = true;
            }
        }
    }
}

impl AddressBus {
    /// Copies the next byte of the running transfer into OAM. Sources in echo
    /// RAM (0xE000 and up) read from the work RAM they mirror, 0x2000 lower,
    /// except on the CGB, which copies 0xFF from there. While VRAM DMA copies
    /// a block, the transfer takes VRAM DMA's byte instead (see
    /// `hdma_transfer_block`).
    fn dma_transfer_byte(&mut self) {
        if let Some((address, value)) = self.vram_dma_bus {
            let index = address & 0xFF;
            if index < u16::from(DMA_TRANSFER_BYTES) {
                self.gpu.write_oam_from_dma(index, value);
            }
            return;
        }
        let offset = u16::from(self.dma.offset);
        let address = self.dma.source_address + offset;
        let byte_to_transfer = match address {
            ECHO_RAM_START.. if self.model.is_cgb() => 0xFF,
            ECHO_RAM_START.. => self.peek(address - ECHO_RAM_OFFSET),
            // The PPU keeps the CPU from VRAM while it draws, but not OAM DMA, whose reads
            // SameBoy lets past that block (`in_dma_read`).
            VIDEO_RAM_START..CARTRIDGE_RAM_START => {
                self.apply_cheats(address, self.gpu.read_byte(address))
            }
            _ => self.peek(address),
        };
        self.gpu.write_oam_from_dma(offset, byte_to_transfer);
    }

    /// The bus an address is on.
    fn bus_for(&self, address: u16) -> Bus {
        match address {
            VIDEO_RAM_START..CARTRIDGE_RAM_START => Bus::Video,
            ..WORK_RAM_START => Bus::External,
            _ if self.model.is_cgb() => Bus::WorkRam,
            _ => Bus::External,
        }
    }

    /// The address the running transfer copies from next, once it has copied
    /// at least one byte. Only then does it get in the CPU's way.
    fn dma_next_source(&self) -> Option<u16> {
        (self.dma.in_progress && self.dma.offset > 0)
            .then(|| self.dma.source_address + u16::from(self.dma.offset))
    }

    /// Whether a CPU access to `address` collides with the running transfer,
    /// whose next source address is `next_source`. The CGB also routes work
    /// RAM accesses through the bus of a cartridge or echo RAM transfer.
    fn collides_with_dma(&self, address: u16, next_source: u16) -> bool {
        if address >= OAM_START
            || address == next_source
            || (next_source >= ECHO_RAM_START && next_source - ECHO_RAM_OFFSET == address)
        {
            return false;
        }
        if self.model.is_cgb() {
            if address >= WORK_RAM_START {
                return self.bus_for(next_source) != Bus::Video;
            }
            if next_source >= ECHO_RAM_START {
                return self.bus_for(address) != Bus::Video;
            }
        }
        self.bus_for(address) == self.bus_for(next_source)
    }

    /// Where a CPU access to `address` lands while OAM DMA runs, or `None`
    /// if it isn't affected. Most collisions reach the byte the transfer just
    /// copied. On the CGB, work RAM accesses during a transfer from outside
    /// work RAM keep their address within the bank but take the bank from
    /// the transfer, and cartridge accesses during a transfer from echo RAM
    /// reach nothing.
    fn dma_conflict(&self, address: u16) -> Option<Conflict> {
        let next_source = self.dma_next_source()?;
        if !self.collides_with_dma(address, next_source) {
            return None;
        }
        let copied_source = next_source - 1;
        let cgb = self.model.is_cgb();
        if cgb && self.bus_for(address) == Bus::External && next_source >= ECHO_RAM_START {
            return Some(Conflict::OpenBus);
        }
        if cgb
            && address >= WORK_RAM_START
            && (self.bus_for(next_source) != Bus::WorkRam || next_source >= ECHO_RAM_START)
        {
            return Some(Conflict::Redirect(
                (copied_source & WORK_RAM_BANK_BIT) | (address & 0x0FFF) | WORK_RAM_START,
            ));
        }
        let read_source = if copied_source >= ECHO_RAM_START {
            copied_source - ECHO_RAM_OFFSET
        } else {
            copied_source
        };
        Some(Conflict::Redirect(read_source))
    }

    /// What the running transfer copies from, as far as collisions with the
    /// CPU are concerned. Echo RAM counts as work RAM on monochrome models,
    /// which copy from the work RAM behind it, and as the cartridge on the
    /// CGB, which copies 0xFF from it.
    fn dma_source_kind(&self, next_source: u16) -> SourceKind {
        let copied_source = next_source - 1;
        match copied_source {
            VIDEO_RAM_START..CARTRIDGE_RAM_START => SourceKind::VideoRam,
            WORK_RAM_START..ECHO_RAM_START => SourceKind::WorkRam,
            ECHO_RAM_START.. if !self.model.is_cgb() => SourceKind::WorkRam,
            _ => SourceKind::Cartridge,
        }
    }

    /// Reads a byte for the CPU, which during OAM DMA may see the byte the
    /// transfer is copying instead. On the CGB, a read that collides with a
    /// transfer from video RAM also clears the OAM byte just copied. Before
    /// reading anything but plain memory, it runs the M-cycles waiting to run
    /// and catches the PPU and the APU up where the address reaches them.
    #[inline]
    pub(crate) fn cpu_read_byte(&mut self, address: u16) -> u8 {
        match self.cpu_read_plain_byte(address) {
            Some(byte) => byte,
            None => self.cpu_read_byte_elsewhere(address),
        }
    }

    /// `cpu_read_byte` for a read of plain memory (see `reads_plain_memory`)
    /// while no OAM DMA runs, outside processor test mode, which needs nothing
    /// else to happen first; `None` for any other read.
    #[inline]
    pub(crate) fn cpu_read_plain_byte(&self, address: u16) -> Option<u8> {
        // The read's M-cycle has already run, so only a transfer in progress can collide with it;
        // writes have to check for a requested one too (see `cpu_write_cycle`).
        (reads_plain_memory(address) && !self.dma.in_progress && !self.processor_test_mode())
            .then(|| self.read_plain_memory(address))
    }

    /// `cpu_read_byte` for everything but plain memory read while no OAM DMA
    /// runs.
    #[inline(never)]
    fn cpu_read_byte_elsewhere(&mut self, address: u16) -> u8 {
        if !reads_plain_memory(address) {
            self.run_deferred_cycles();
            if reaches_ppu(address) && !self.ppu_read_can_wait(address) {
                self.catch_up_ppu();
            }
            if Apu::reads_channel_state(address) {
                self.apu.catch_up();
            }
        }
        if !self.dma.in_progress {
            return self.peek(address);
        }
        match self.dma_conflict(address) {
            None => self.peek(address),
            Some(Conflict::OpenBus) => 0xFF,
            Some(Conflict::Redirect(redirected)) => {
                let byte = self.peek(redirected);
                if let Some(next_source) = self.dma_next_source()
                    && self.model.is_cgb()
                    && self.dma_source_kind(next_source) == SourceKind::VideoRam
                {
                    self.set_last_copied_oam_byte(0);
                }
                byte
            }
        }
    }

    /// Writes a byte for the CPU. During OAM DMA, a write that collides with
    /// the transfer changes the OAM byte just copied instead of memory. The
    /// CGB's work RAM writes during a transfer from outside work RAM are the
    /// exception: they land in work RAM, in the bank the transfer picks.
    pub(super) fn cpu_write_byte(&mut self, address: u16, value: u8) {
        let Some(next_source) = self.dma_next_source() else {
            self.poke(address, value);
            return;
        };
        match self.dma_conflict(address) {
            None => self.poke(address, value),
            Some(Conflict::Redirect(redirected))
                if self.model.is_cgb()
                    && address >= WORK_RAM_START
                    && !(WORK_RAM_START..ECHO_RAM_START).contains(&next_source) =>
            {
                self.poke(redirected, value);
            }
            Some(_) => self.write_dma_conflict_to_oam(next_source, value),
        }
    }

    /// Applies a CPU write that collided with OAM DMA to the OAM byte the
    /// transfer just copied, following Gambatte's hardware tests on DMG and
    /// CGB-C and SameBoy for the other CGB revisions. Monochrome models AND
    /// the value into bytes copied from work RAM and store it over the rest.
    /// The CGB clears bytes copied from video RAM, stores the value over bytes
    /// from the cartridge, and treats bytes from work RAM differently per
    /// revision. On CGB-E and the AGB a write that collides with a transfer
    /// from video RAM also reaches the transfer's source.
    fn write_dma_conflict_to_oam(&mut self, next_source: u16, value: u8) {
        let copied = self
            .gpu
            .object_attribute_memory_byte(u16::from(self.dma.offset) - 1);
        let source_kind = self.dma_source_kind(next_source);
        let new_byte = match (self.model.is_cgb(), source_kind) {
            (false, SourceKind::WorkRam) => copied & value,
            (false, _) | (true, SourceKind::Cartridge) => value,
            (true, SourceKind::VideoRam) => 0,
            (true, SourceKind::WorkRam) => match self.model {
                Model::CgbB => copied & value,
                Model::Cgb0 | Model::CgbA | Model::Agb => value,
                _ => copied,
            },
        };
        self.set_last_copied_oam_byte(new_byte);

        if self.model >= Model::CgbE && source_kind == SourceKind::VideoRam {
            self.poke(next_source - 1, value);
        }
    }

    /// Called after the CPU's first halted M-cycle. OAM DMA stands still
    /// until the CPU wakes up, like in SameBoy.
    pub(crate) fn dma_halt_started(&mut self) {
        self.run_deferred_cycles_before_change();
        self.dma.paused_by_halt = true;
    }

    /// Called when the CPU wakes up from HALT. OAM DMA resumes, starting
    /// with the M-cycle in which the CPU woke up.
    pub(crate) fn dma_halt_ended(&mut self) {
        self.run_deferred_cycles_before_change();
        if self.dma.paused_by_halt {
            self.dma.paused_by_halt = false;
            self.dma_step();
        }
    }

    /// How the transfer affects the PPU's own OAM reads in the coming
    /// M-cycle. It holds OAM only in the M-cycles in which it copies a byte,
    /// not in the one after the DMA write nor once all 160 bytes are copied,
    /// even if HALT stops it before it finishes.
    pub(super) fn ppu_oam_dma_access(&self) -> OamDmaAccess {
        let dma = &self.dma;
        let starts_copying = dma.start_delay == 1;
        let copies_byte = starts_copying || (dma.in_progress && dma.offset < DMA_TRANSFER_BYTES);
        if dma.start_delay == 0 && !copies_byte {
            OamDmaAccess::Free
        } else if dma.paused_by_halt {
            OamDmaAccess::PausedByHalt
        } else if copies_byte {
            OamDmaAccess::Running {
                destination: if starts_copying { 0 } else { dma.offset },
            }
        } else {
            OamDmaAccess::Free
        }
    }

    fn set_last_copied_oam_byte(&mut self, value: u8) {
        let index = u16::from(self.dma.offset) - 1;
        self.gpu.set_object_attribute_memory_byte(index, value);
    }

    /// Advances OAM DMA by one M-cycle: starts a requested transfer when its
    /// delay runs out, then copies one byte, or ends the transfer once all 160
    /// bytes are copied. Nothing happens while the CPU is halted.
    #[inline]
    pub(super) fn dma_step(&mut self) {
        if self.dma.start_delay != 0 || self.dma.in_progress {
            self.dma_step_in_progress();
        }
    }

    /// `dma_step` while a transfer is requested or running.
    #[inline(never)]
    fn dma_step_in_progress(&mut self) {
        if self.dma.paused_by_halt {
            return;
        }
        self.dma.advance_start_delay();

        if self.dma.in_progress {
            if self.dma.offset == DMA_TRANSFER_BYTES {
                self.dma.in_progress = false;
            } else {
                self.dma_transfer_byte();
                self.dma.offset += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address_bus::constants::*;
    use crate::address_bus::effects::empty_cartridge_effects;
    use crate::address_bus::test_utils::*;

    #[test]
    fn should_start_dma_transfer_two_m_cycles_after_the_write() {
        let mut dma = DmaState::default();
        dma.start_dma(0x12);
        assert_eq!(dma.register(), 0x12);
        assert!(!dma.in_progress);

        dma.advance_start_delay();
        assert!(!dma.in_progress);

        dma.advance_start_delay();
        assert_eq!(dma.source_address, 0x1200);
        assert_eq!(dma.offset, 0x0);
        assert!(dma.in_progress);
    }

    #[test]
    fn should_keep_the_running_transfer_from_the_new_source_until_a_restarted_one_begins() {
        let mut dma = DmaState::default();
        dma.start_dma(0x12);
        dma.advance_start_delay();
        dma.advance_start_delay();
        dma.offset = 0x40;

        dma.start_dma(0x34);
        dma.advance_start_delay();
        assert_eq!(dma.source_address, 0x3400);
        assert_eq!(dma.offset, 0x40);

        dma.advance_start_delay();
        assert_eq!(dma.source_address, 0x3400);
        assert_eq!(dma.offset, 0x0);
        assert!(dma.in_progress);
    }

    #[test]
    fn should_transfer_byte_from_source_to_destination() {
        let mut address_bus = initialize_test_address_bus();

        let mut rom = build_rom(CART_TYPE_MBC1, ROM_SIZE_64KB, RAM_SIZE_2KB);
        rom[0x1200] = 0x12;
        address_bus
            .load_rom(rom, empty_cartridge_effects())
            .unwrap();

        address_bus.dma.source_address = 0x1200;
        address_bus.dma.offset = 0x0;
        address_bus.dma.in_progress = true;

        address_bus.sync();

        assert_eq!(address_bus.gpu.object_attribute_memory_byte(0), 0x12);
        assert_eq!(address_bus.dma.source_address, 0x1200);
        assert_eq!(address_bus.dma.offset, 1);
        assert!(address_bus.dma.in_progress);
    }

    #[test]
    fn should_stop_dma_transfer_after_transferring_160_bytes() {
        let mut address_bus = initialize_test_address_bus();

        let mut rom = build_rom(CART_TYPE_MBC1, ROM_SIZE_64KB, RAM_SIZE_2KB);
        rom[0x129F] = 0x12;
        address_bus
            .load_rom(rom, empty_cartridge_effects())
            .unwrap();

        address_bus.dma.source_address = 0x1200;
        address_bus.dma.offset = 0x0;
        address_bus.dma.in_progress = true;

        for _ in 0..DMA_TRANSFER_BYTES {
            address_bus.sync();
        }

        assert_eq!(address_bus.gpu.object_attribute_memory_byte(0x9F), 0x12);
        assert_eq!(address_bus.dma.offset, DMA_TRANSFER_BYTES);
        assert!(address_bus.dma.in_progress);

        address_bus.sync();
        assert!(!address_bus.dma.in_progress);
    }

    #[test]
    fn should_do_nothing_if_no_dma_transfer_is_in_progress() {
        let mut address_bus = initialize_test_address_bus();
        address_bus.dma_step();
        assert_eq!(address_bus.dma.source_address, 0x0);
        assert_eq!(address_bus.dma.offset, 0x0);
        assert!(!address_bus.dma.in_progress);
    }

    #[test]
    fn copies_from_vram_while_the_ppu_keeps_the_cpu_from_it() {
        let mut address_bus = initialize_test_address_bus();
        address_bus.gpu.set_video_ram_byte(0x0000, 0x5A);
        address_bus.gpu.set_lcdc(0x80);
        while !address_bus.gpu.vram_read_blocked() {
            address_bus.gpu.step_dots(1, &mut address_bus.hdma);
        }
        assert_eq!(address_bus.peek(0x8000), 0xFF);

        address_bus.dma.source_address = 0x8000;
        address_bus.dma.in_progress = true;
        address_bus.sync();

        assert_eq!(address_bus.gpu.object_attribute_memory_byte(0), 0x5A);
    }
}
