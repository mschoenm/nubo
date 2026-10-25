//! The I/O registers at 0xFF00-0xFF7F: routes each CPU access to the component
//! that owns the register (joypad, serial port, timers, APU, PPU, DMA), plus
//! the interrupt flag register and the registers only the Game Boy Color has.

use crate::address_bus::{AddressBus, MemoryMapped};
use crate::utils::is_bit_set;

impl AddressBus {
    /// Reads the I/O register at `address` (0xFF00-0xFF7F).
    pub(super) fn read_io_register(&self, address: u16) -> u8 {
        match address & 0xFF {
            0x00 => self.joypad.read_byte(),
            0x01 => self.serial.read_byte(address),
            0x02 => self.serial.read_byte(address) | self.unused_serial_control_bits(),
            0x10..=0x26 | 0x30..=0x3F => self.apu.read_byte(address),
            0x41 => self.read_stat(),
            0x40..=0x45 | 0x47..=0x4B => self.gpu.read_byte(address),
            0x46 => self.dma.register(),
            0x4C..=0x7F => self.read_cgb_register(address),
            0x0F => self.interrupt_flags(),
            0x04..=0x07 => self.timers.read_byte(address),
            _ => 0xFF,
        }
    }

    /// Reads STAT. While OAM DMA holds OAM, the OAM scan shows as mode 0
    /// instead of mode 2 (SameBoy).
    fn read_stat(&self) -> u8 {
        let stat = self.gpu.read_byte(0xFF41);
        if !self.dma.is_idle() && stat & 0x03 == 0x02 {
            stat & !0x03
        } else {
            stat
        }
    }

    /// Writes JOYP (0xFF00). On a Super Game Boy the Super Nintendo sees the write too, and it may
    /// finish a command, such as `MLT_REQ`, which changes how many controllers the joypad reads
    /// (see `sgb.rs`). Every frame the PPU has finished by now has already reached the Super
    /// Nintendo, since the PPU is never left behind past the end of a frame.
    fn write_joyp(&mut self, value: u8) {
        let previous_select = self.joypad.group_select();
        match self.gpu.sgb_mut() {
            Some(sgb) => {
                let requested_players = sgb.write_joyp(previous_select, value);
                self.joypad
                    .write_byte_with_sgb_players(value, requested_players);
            }
            None => self.joypad.write_byte(value),
        }
    }

    /// Writes the I/O register at `address` (0xFF00-0xFF7F).
    pub(super) fn write_io_register(&mut self, address: u16, value: u8) {
        match address & 0xFF {
            0x00 => self.write_joyp(value),
            0x01 => self.serial.write_byte(address, value),
            // SC's clock speed bit only exists in CGB mode (Pan Docs); in DMG
            // compatibility mode the serial port keeps its normal clock.
            0x02 if !self.cgb_registers_enabled() => {
                self.serial.write_byte(address, value & !0x02);
            }
            0x02 => self.serial.write_byte(address, value),
            0x10..=0x26 | 0x30..=0x3F => self.apu.write_byte(address, value),
            0x40..=0x45 | 0x47..=0x4B => self.gpu.write_byte(address, value),
            0x46 => self.dma.start_dma(value),
            0x50 if value != 0 && self.in_boot_rom => self.exit_boot_rom(),
            0x4C..=0x7F => self.write_cgb_register(address, value),
            0x0F => self.set_interrupt_flags(value),
            0x04 => self.reset_divider(true),
            0x05..=0x07 => self.timers.write_byte(address, value),
            _ => (),
        }
    }

    /// Reads IF (0xFF0F), the interrupt flag register. Each interrupt source
    /// keeps its own request flag; this gathers them into one byte. The upper
    /// three bits always read as 1.
    ///
    /// The CPU checks for pending interrupts with this while the PPU may be
    /// behind. That gives the right answer for every interrupt IE enables:
    /// the PPU is only left behind while it can't request one of those.
    pub(crate) fn interrupt_flags(&self) -> u8 {
        debug_assert!(
            self.ppu_owed_dots <= self.ppu_quiet_dots,
            "the PPU is behind by more than its quiet dots"
        );
        let mut flags = 0;
        if self.gpu.vblank_interrupt() {
            flags |= 0x1;
        }
        if self.gpu.stat_interrupt() {
            flags |= 0x2;
        }
        if self.timers.interrupt {
            flags |= 0x4;
        }
        if self.serial.interrupt() {
            flags |= 0x8;
        }
        if self.joypad.interrupt() {
            flags |= 0x10;
        }
        flags | 0xE0
    }

    /// Clears the request flags of the interrupts whose bits are set in
    /// `bits`, as the CPU does when it handles one. The PPU catches up first,
    /// so it doesn't raise a flag afterwards for dots that came before.
    pub(crate) fn clear_interrupt_request(&mut self, bits: u8) {
        self.catch_up_ppu();
        self.set_interrupt_flags(self.interrupt_flags() & !bits);
    }

    /// Writes IF (0xFF0F), setting or clearing each source's request flag.
    pub(crate) fn set_interrupt_flags(&mut self, flags: u8) {
        self.gpu.set_vblank_interrupt(is_bit_set(flags, 0));
        self.gpu.set_stat_interrupt(is_bit_set(flags, 1));
        self.timers.interrupt = is_bit_set(flags, 2);
        self.serial.set_interrupt(is_bit_set(flags, 3));
        self.joypad.set_interrupt(is_bit_set(flags, 4));
    }

    /// Most CGB registers are only available while the CGB boot ROM runs or
    /// when a CGB game runs. In DMG compatibility mode (a CGB running an
    /// original Game Boy game) they are locked after the boot ROM exits.
    fn cgb_registers_enabled(&self) -> bool {
        self.cgb_mode && (self.in_boot_rom || !self.gpu.in_dmg_compatibility_mode())
    }

    /// The bits of SC (serial control, 0xFF02) that always read as 1. Bit 1,
    /// the CGB clock speed bit, is only readable when CGB registers are on.
    fn unused_serial_control_bits(&self) -> u8 {
        if self.cgb_registers_enabled() {
            0x7C
        } else {
            0x7E
        }
    }

    /// Reads a register in 0xFF4C-0xFF7F, the range the Game Boy Color adds
    /// (VRAM and work RAM banks, palettes, VRAM DMA, KEY1 speed switch and a
    /// few undocumented ones). Reads 0xFF on a DMG.
    fn read_cgb_register(&self, address: u16) -> u8 {
        if !self.cgb_mode {
            return 0xFF;
        }

        let enabled = self.cgb_registers_enabled();
        match address & 0xFF {
            0x4C if self.in_boot_rom => self.gpu.read_byte(address),
            0x4D if enabled => self.speed_switch.key1(),
            0x4F if enabled => self.gpu.read_byte(address),
            0x4F => 0xFE,
            0x55 if enabled => self.hdma.read_byte(address),
            0x68 | 0x6A => self.gpu.read_byte(address) | 0x40,
            0x69 | 0x6B | 0x6C if enabled => self.gpu.read_byte(address),
            0x70 if enabled => self.svbk | 0xF8,
            0x72 | 0x73 => self.undocumented_registers[(address & 0xFF) as usize - 0x72],
            0x74 if enabled => self.undocumented_registers[2],
            0x75 => self.undocumented_registers[3] | 0x8F,
            0x76 => self.apu.pcm12(),
            0x77 => self.apu.pcm34(),
            _ => 0xFF,
        }
    }

    /// Writes a register in 0xFF4C-0xFF7F. Ignored on a DMG.
    fn write_cgb_register(&mut self, address: u16, value: u8) {
        if !self.cgb_mode {
            return;
        }

        let enabled = self.cgb_registers_enabled();
        match address & 0xFF {
            0x4C if self.in_boot_rom => self.gpu.write_byte(address, value),
            0x4D if enabled => self.speed_switch.set_key1(value),
            0x4F if enabled => self.gpu.write_byte(address, value),
            0x51..=0x55 if enabled => {
                self.hdma.write_byte(address, value);
                if address == 0xFF55
                    && self.hdma.hblank_transfer_waiting()
                    && self.gpu.in_hblank_or_off()
                {
                    self.hdma.set_hblank_started(true);
                }
            }
            0x68..=0x6C if enabled => self.gpu.write_byte(address, value),
            0x70 if enabled => self.svbk = value,
            0x72 | 0x73 => self.undocumented_registers[(address & 0xFF) as usize - 0x72] = value,
            0x74 if enabled => self.undocumented_registers[2] = value,
            0x75 => self.undocumented_registers[3] = value & 0x70,
            _ => (),
        }
    }
}
