//! Small test cartridges this tool builds itself, one for each kind of memory
//! bank controller (MBC: the chip on a cartridge that switches which part of
//! its ROM and RAM the console sees), so the save states of every controller
//! are kept without needing a game for each. Each one sets up as much of the
//! hardware as it can (sound on all four channels, the timer, a serial
//! transfer, the background, the window and objects, OAM DMA, the
//! controller's banks and RAM, and on the Game Boy Color its palettes, banks,
//! VRAM DMA and double speed) and then waits for interrupts in a loop.
//!
//! A save state only loads into the game it was made with, so a cartridge's
//! bytes must never change once states of it are kept: add a new cartridge
//! instead. `the_cartridges_never_change` pins them.

use nubo_core::Model;

/// A test cartridge, and the models its states are made on.
pub struct TestCartridge {
    /// The file name its states are named after, such as `mbc1.gb`.
    pub file: &'static str,
    pub models: &'static [Model],
    title: &'static str,
    /// The cartridge type byte of the header (0x0147), which picks the
    /// controller.
    cartridge_type: u8,
    /// 0x80 for a game that also runs on the Game Boy, 0xC0 for one that
    /// only runs on the Game Boy Color, 0x00 for a Game Boy game.
    cgb_flag: u8,
    /// The RAM size byte of the header (0x0149).
    ram_size: u8,
    controller: Controller,
}

/// The memory bank controllers, which each set up their registers their own
/// way.
#[derive(Clone, Copy)]
enum Controller {
    None,
    Mbc1,
    Mbc2,
    Mbc3,
    Mbc5,
    Huc1,
}

const DMG_AND_CGB: &[Model] = &[Model::Dmg, Model::CgbE];
const DMG: &[Model] = &[Model::Dmg];
const CGB: &[Model] = &[Model::CgbE];

/// No RAM, 8 KiB of RAM and the controller's own RAM (MBC2).
const NO_RAM: u8 = 0x00;
const RAM_8_KIB: u8 = 0x02;

pub const CARTRIDGES: [TestCartridge; 6] = [
    TestCartridge {
        file: "rom-only.gb",
        models: DMG_AND_CGB,
        title: "NUBO ROM ONLY",
        cartridge_type: 0x00,
        cgb_flag: 0x00,
        ram_size: NO_RAM,
        controller: Controller::None,
    },
    TestCartridge {
        file: "mbc1.gb",
        models: DMG,
        title: "NUBO MBC1",
        cartridge_type: 0x03,
        cgb_flag: 0x00,
        ram_size: RAM_8_KIB,
        controller: Controller::Mbc1,
    },
    TestCartridge {
        file: "mbc2.gb",
        models: DMG,
        title: "NUBO MBC2",
        cartridge_type: 0x06,
        cgb_flag: 0x00,
        ram_size: NO_RAM,
        controller: Controller::Mbc2,
    },
    TestCartridge {
        file: "mbc3.gbc",
        models: CGB,
        title: "NUBO MBC3",
        cartridge_type: 0x10,
        cgb_flag: 0x80,
        ram_size: RAM_8_KIB,
        controller: Controller::Mbc3,
    },
    TestCartridge {
        file: "mbc5.gbc",
        models: CGB,
        title: "NUBO MBC5",
        cartridge_type: 0x1E,
        cgb_flag: 0xC0,
        ram_size: RAM_8_KIB,
        controller: Controller::Mbc5,
    },
    TestCartridge {
        file: "huc1.gb",
        models: DMG,
        title: "NUBO HUC1",
        cartridge_type: 0xFF,
        cgb_flag: 0x00,
        ram_size: RAM_8_KIB,
        controller: Controller::Huc1,
    },
];

/// Four 16 KiB banks.
const ROM_SIZE: usize = 0x10000;
const ROM_SIZE_BYTE: u8 = 0x01;
const PROGRAM_START: usize = 0x0150;
/// Where the OAM DMA routine is kept, to be copied to HRAM: the CPU can't
/// read the ROM while OAM DMA copies from work RAM on a Game Boy.
const OAM_DMA_ROUTINE_ADDRESS: u16 = 0x3F00;
/// Starts OAM DMA from the page in A and waits the 160 M-cycles it takes.
const OAM_DMA_ROUTINE: [u8; 7] = [
    0xE0, 0x46, // LDH (0x46), A
    0x3E, 0x28, // LD A, 40
    0x3D, // wait: DEC A
    0x20, 0xFD, // JR NZ, wait
];
const INTERRUPT_VECTORS: [usize; 5] = [0x40, 0x48, 0x50, 0x58, 0x60];
const RETI: u8 = 0xD9;
const RET: u8 = 0xC9;

impl TestCartridge {
    /// The cartridge's ROM.
    pub fn rom(&self) -> Vec<u8> {
        let mut rom = vec![0; ROM_SIZE];
        for (bank, bytes) in rom.chunks_mut(0x4000).enumerate().skip(1) {
            for (offset, byte) in bytes.iter_mut().enumerate() {
                *byte = (bank as u8).wrapping_mul(0x40) ^ (offset as u8);
            }
        }
        for vector in INTERRUPT_VECTORS {
            rom[vector] = RETI;
        }
        let routine = usize::from(OAM_DMA_ROUTINE_ADDRESS);
        rom[routine..routine + OAM_DMA_ROUTINE.len()].copy_from_slice(&OAM_DMA_ROUTINE);
        rom[routine + OAM_DMA_ROUTINE.len()] = RET;

        // NOP; JP 0x0150
        rom[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
        // A made-up logo: the boot ROMs Nubo ships don't check it.
        for (offset, byte) in rom[0x0104..0x0134].iter_mut().enumerate() {
            *byte = (offset as u8).wrapping_mul(37) ^ 0x5A;
        }
        rom[0x0134..0x0134 + self.title.len()].copy_from_slice(self.title.as_bytes());
        rom[0x0143] = self.cgb_flag;
        rom[0x0147] = self.cartridge_type;
        rom[0x0148] = ROM_SIZE_BYTE;
        rom[0x0149] = self.ram_size;
        rom[0x014D] = rom[0x0134..0x014D]
            .iter()
            .fold(0u8, |sum, byte| sum.wrapping_sub(*byte).wrapping_sub(1));

        let program = self.program();
        rom[PROGRAM_START..PROGRAM_START + program.len()].copy_from_slice(&program);
        assert!(PROGRAM_START + program.len() < routine);

        let checksum = rom
            .iter()
            .enumerate()
            .filter(|&(address, _)| address != 0x014E && address != 0x014F)
            .fold(0u16, |sum, (_, &byte)| sum.wrapping_add(u16::from(byte)));
        rom[0x014E..0x0150].copy_from_slice(&checksum.to_be_bytes());
        rom
    }

    /// The program, from 0x0150 on.
    fn program(&self) -> Vec<u8> {
        let cgb = self.cgb_flag & 0x80 != 0;
        let mut program = Program::default();
        program.bytes(&[0xF3]); // DI
        program.bytes(&[0x31, 0xFE, 0xFF]); // LD SP, 0xFFFE
        if self.cgb_flag == 0xC0 {
            // Arm the speed switch and switch to double speed.
            program.write_register(0x4D, 0x01);
            program.bytes(&[0x10, 0x00]); // STOP
        }

        // Turn the screen off in the vertical blank to fill the VRAM.
        program.bytes(&[0xF0, 0x44, 0xFE, 0x90, 0x20, 0xFA]); // wait: LDH A, (LY); CP 144; JR NZ, wait
        program.write_register(0x40, 0x00);
        program.fill(0x8000, 0, 0x3C, 0x47);
        program.fill(0x8100, 0, 0x81, 0x29);
        program.fill(0x9800, 0, 0x00, 0x03);
        program.fill(0x9C00, 64, 0x10, 0x01);
        if cgb {
            program.write_register(0x4F, 0x01);
            program.fill(0x9800, 0, 0x00, 0x09);
            program.fill(0x8000, 64, 0xAA, 0x11);
            program.write_register(0x4F, 0x00);
            program.write_register(0x68, 0x80);
            program.repeat_register(0x69, 64, 0x00, 0x35);
            program.write_register(0x6A, 0x80);
            program.repeat_register(0x6B, 64, 0x1F, 0x2B);
            program.write_register(0x70, 0x03);
            program.write(0xD123, 0x5A);
            for (register, value) in [(0x72, 0x5A), (0x73, 0xA5), (0x74, 0x3C), (0x75, 0x70)] {
                program.write_register(register, value);
            }
        }

        // Sound on all four channels, which keep playing: channel 1 sweeps
        // down, channels 2 and 4 get louder up to full volume, channel 3
        // plays its wave.
        program.write_register(0x26, 0x80);
        program.write_register(0x24, 0x77);
        program.write_register(0x25, 0xFF);
        for (register, value) in [(0x10, 0x1D), (0x11, 0x80), (0x12, 0xA0), (0x13, 0x00)] {
            program.write_register(register, value);
        }
        program.write_register(0x14, 0x87);
        for (register, value) in [(0x16, 0x40), (0x17, 0x5F), (0x18, 0x40)] {
            program.write_register(register, value);
        }
        program.write_register(0x19, 0x86);
        program.write_register(0x1A, 0x00);
        program.fill(0xFF30, 16, 0x1F, 0x1D);
        for (register, value) in [(0x1A, 0x80), (0x1B, 0x10), (0x1C, 0x20), (0x1D, 0x80)] {
            program.write_register(register, value);
        }
        program.write_register(0x1E, 0x85);
        for (register, value) in [(0x21, 0x3D), (0x22, 0x5B)] {
            program.write_register(register, value);
        }
        program.write_register(0x23, 0x80);

        // Objects, copied to OAM by OAM DMA from work RAM.
        program.fill(0xC000, 160, 0x18, 0x0B);
        let [low, high] = OAM_DMA_ROUTINE_ADDRESS.to_le_bytes();
        program.bytes(&[0x21, 0x80, 0xFF]); // LD HL, 0xFF80
        program.bytes(&[0x11, low, high]); // LD DE, routine
        program.bytes(&[0x0E, OAM_DMA_ROUTINE.len() as u8 + 1]); // LD C, length
        program.bytes(&[0x1A, 0x22, 0x13, 0x0D, 0x20, 0xFA]); // copy: LD A, (DE); LD (HL+), A; INC DE; DEC C; JR NZ, copy
        program.bytes(&[0x3E, 0xC0, 0xCD, 0x80, 0xFF]); // LD A, 0xC0; CALL 0xFF80

        self.set_up_controller(&mut program);

        for (register, value) in [
            (0x47, 0xE4),                          // BGP
            (0x48, 0xD2),                          // OBP0
            (0x49, 0x1B),                          // OBP1
            (0x42, 0x10),                          // SCY
            (0x43, 0x08),                          // SCX
            (0x4A, 0x40),                          // WY
            (0x4B, 0x37),                          // WX
            (0x45, 0x45),                          // LYC
            (0x41, 0x40),                          // STAT: interrupt on LY = LYC
            (0x06, 0x80),                          // TMA
            (0x07, 0x05),                          // TAC: 262,144 Hz
            (0x00, 0x20),                          // JOYP: the direction keys
            (0x02, if cgb { 0x83 } else { 0x81 }), // SC: send on the internal clock
            (0x90, 0x77),                          // HRAM
            (0x0F, 0x00),                          // IF
            (0xFF, 0x1F),                          // IE: every interrupt
            (0x40, 0xF3),                          // LCDC: screen, window and objects on
        ] {
            program.write_register(register, value);
        }
        if cgb {
            // An HBlank VRAM DMA of six blocks from 0x4000 to 0x8100.
            for (register, value) in [(0x51, 0x40), (0x52, 0x00), (0x53, 0x81), (0x54, 0x00)] {
                program.write_register(register, value);
            }
            program.write_register(0x55, 0x85);
        }

        program.bytes(&[0xFB]); // EI
        program.bytes(&[0x04, 0x78, 0xEA, 0xF0, 0xC1, 0x76, 0x18, 0xF8]); // loop: INC B; LD A, B; LD (0xC1F0), A; HALT; JR loop
        program.0
    }

    /// Enables the controller's RAM, writes it and switches banks.
    fn set_up_controller(&self, program: &mut Program) {
        match self.controller {
            Controller::None => {}
            Controller::Mbc1 => {
                program.write(0x0000, 0x0A);
                program.write(0x2000, 0x02);
                program.write(0x6000, 0x01);
                program.write(0xA000, 0x77);
                program.write(0xA123, 0x88);
            }
            Controller::Mbc2 => {
                // Address bit 8 picks the register: clear for RAM enable, set
                // for the ROM bank.
                program.write(0x0000, 0x0A);
                program.write(0x2100, 0x03);
                program.write(0xA000, 0x0E);
                program.write(0xA1FF, 0x05);
            }
            Controller::Mbc3 => {
                program.write(0x0000, 0x0A);
                program.write(0x2000, 0x03);
                // Set the clock's seconds and hours, then latch it.
                program.write(0x4000, 0x08);
                program.write(0xA000, 0x15);
                program.write(0x4000, 0x0A);
                program.write(0xA000, 0x05);
                program.write(0x6000, 0x00);
                program.write(0x6000, 0x01);
                program.write(0x4000, 0x00);
                program.write(0xA000, 0x77);
            }
            Controller::Mbc5 => {
                program.write(0x0000, 0x0A);
                program.write(0x2000, 0x03);
                program.write(0x3000, 0x00);
                // RAM bank 0 with the rumble motor on.
                program.write(0x4000, 0x08);
                program.write(0xA000, 0x77);
            }
            Controller::Huc1 => {
                program.write(0x0000, 0x0A);
                program.write(0x2000, 0x02);
                program.write(0x4000, 0x00);
                program.write(0xA000, 0x77);
            }
        }
    }
}

/// Machine code, written a few instructions at a time.
#[derive(Default)]
struct Program(Vec<u8>);

impl Program {
    fn bytes(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    /// Writes `value` to the I/O register or HRAM byte at 0xFF00 + `register`.
    fn write_register(&mut self, register: u8, value: u8) {
        self.bytes(&[0x3E, value, 0xE0, register]); // LD A, value; LDH (register), A
    }

    /// Writes `value` to `address`.
    fn write(&mut self, address: u16, value: u8) {
        let [low, high] = address.to_le_bytes();
        self.bytes(&[0x3E, value, 0xEA, low, high]); // LD A, value; LD (address), A
    }

    /// Writes `count` bytes from `address` on (256 for 0), starting with
    /// `first` and adding `step` each time.
    fn fill(&mut self, address: u16, count: u8, first: u8, step: u8) {
        let [low, high] = address.to_le_bytes();
        self.bytes(&[0x21, low, high]); // LD HL, address
        self.bytes(&[0x0E, count, 0x3E, first]); // LD C, count; LD A, first
        self.bytes(&[0x22, 0xC6, step, 0x0D, 0x20, 0xFA]); // loop: LD (HL+), A; ADD A, step; DEC C; JR NZ, loop
    }

    /// Writes `count` values to one register, such as a palette data
    /// register that moves on by itself, starting with `first` and adding
    /// `step` each time.
    fn repeat_register(&mut self, register: u8, count: u8, first: u8, step: u8) {
        self.bytes(&[0x0E, count, 0x3E, first]); // LD C, count; LD A, first
        self.bytes(&[0xE0, register, 0xC6, step, 0x0D, 0x20, 0xF9]); // loop: LDH (register), A; ADD A, step; DEC C; JR NZ, loop
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FNV-1a, to pin the cartridges' bytes.
    fn fingerprint(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xCBF2_9CE4_8422_2325, |hash, &byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01B3)
        })
    }

    /// Kept save states only load into the cartridge they were made with.
    #[test]
    fn the_cartridges_never_change() {
        let fingerprints: Vec<(&str, u64)> = CARTRIDGES
            .iter()
            .map(|cartridge| (cartridge.file, fingerprint(&cartridge.rom())))
            .collect();
        assert_eq!(
            fingerprints,
            [
                ("rom-only.gb", 0x2F15_51F3_5C66_D5EF),
                ("mbc1.gb", 0xD4CE_636B_18A8_FCEF),
                ("mbc2.gb", 0xE5B8_5E49_7C3C_99BF),
                ("mbc3.gbc", 0x7BF4_3B9A_734E_4213),
                ("mbc5.gbc", 0xEA8E_41AF_F21E_142B),
                ("huc1.gb", 0x0C00_FA0D_2E9D_A869),
            ]
        );
    }
}
