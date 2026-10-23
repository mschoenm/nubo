//! Small Game Boy programs the tests run, each on a cartridge with just
//! enough of a header for Nubo's boot ROMs: the cartridge type, the RAM size,
//! the Game Boy Color flag and the header checksum.

/// Where a cartridge's program starts: the boot ROM jumps to 0x0100, whose
/// `JP 0x0150` skips the header.
const PROGRAM_START: usize = 0x0150;
/// The size of the ROM: two banks of 16 KiB.
const ROM_SIZE: usize = 0x8000;

/// A ROM-only cartridge.
pub const ROM_ONLY: u8 = 0x00;
/// An MBC1 cartridge with battery-backed RAM.
pub const MBC1_RAM_BATTERY: u8 = 0x03;
/// An MBC3 cartridge with a clock and battery-backed RAM.
pub const MBC3_TIMER_RAM_BATTERY: u8 = 0x10;
/// An MBC5 cartridge with a rumble motor.
pub const MBC5_RUMBLE: u8 = 0x1C;
/// The header's RAM size for 2 KiB.
pub const RAM_2_KIB: u8 = 0x01;
/// The header's RAM size for 8 KiB.
pub const RAM_8_KIB: u8 = 0x02;
/// The header's RAM size for 32 KiB.
pub const RAM_32_KIB: u8 = 0x03;
/// The byte at 0x0200 that [`CHEATS`] copies, which a Game Genie code can
/// change.
pub const ROM_BYTE_AT_0200: u8 = 0x11;

/// Reads the D-pad into 0xC000 and the buttons into 0xC001, forever: the
/// low four bits are clear for each one held (right, left, up, down; A, B,
/// Select, Start).
#[rustfmt::skip]
pub const JOYPAD: &[u8] = &[
    0x3E, 0x20, 0xE0, 0x00,  // loop: P1 = 0x20, the D-pad
    0xF0, 0x00, 0xF0, 0x00,  // LDH A, (P1) twice, to let it settle
    0xEA, 0x00, 0xC0,        // LD (0xC000), A
    0x3E, 0x10, 0xE0, 0x00,  // P1 = 0x10, the buttons
    0xF0, 0x00, 0xF0, 0x00,  // LDH A, (P1) twice
    0xEA, 0x01, 0xC0,        // LD (0xC001), A
    0x18, 0xE8,              // JR loop
];

/// Selects the buttons, then counts the joypad interrupts requested at
/// 0xC000, forever: one for each time a button's press pulls its line low.
#[rustfmt::skip]
pub const JOYPAD_INTERRUPTS: &[u8] = &[
    0x3E, 0x10, 0xE0, 0x00,  // P1 = 0x10, the buttons
    0xAF, 0xEA, 0x00, 0xC0,  // 0xC000 = 0
    0xE0, 0x0F,              // IF = 0
    0xF0, 0x0F,              // loop: A = IF
    0xE6, 0x10,              // AND the joypad's bit
    0x28, 0xFA,              // JR Z, loop
    0x21, 0x00, 0xC0, 0x34,  // INC (0xC000)
    0xAF, 0xE0, 0x0F,        // IF = 0
    0x18, 0xF1,              // JR loop
];

/// Enables cartridge RAM, then copies 0xA000 to 0xC000 and counts 0xA001
/// up, forever.
#[rustfmt::skip]
pub const RAM: &[u8] = &[
    0x3E, 0x0A, 0xEA, 0x00, 0x00,  // RAM enable = 0x0A
    0xFA, 0x00, 0xA0,              // loop: LD A, (0xA000)
    0xEA, 0x00, 0xC0,              // LD (0xC000), A
    0x21, 0x01, 0xA0, 0x34,        // LD HL, 0xA001; INC (HL)
    0x18, 0xF4,                    // JR loop
];

/// Enables the MBC3's RAM and clock, then latches the clock and copies its
/// seconds to 0xC000, minutes to 0xC001 and hours to 0xC002, forever.
#[rustfmt::skip]
pub const CLOCK: &[u8] = &[
    0x3E, 0x0A, 0xEA, 0x00, 0x00,  // RAM and clock enable = 0x0A
    0xAF, 0xEA, 0x00, 0x60,        // loop: latch: 0x6000 = 0
    0x3C, 0xEA, 0x00, 0x60,        // then 1
    0x3E, 0x08, 0xEA, 0x00, 0x40,  // select the seconds
    0xFA, 0x00, 0xA0, 0xEA, 0x00, 0xC0,
    0x3E, 0x09, 0xEA, 0x00, 0x40,  // the minutes
    0xFA, 0x00, 0xA0, 0xEA, 0x01, 0xC0,
    0x3E, 0x0A, 0xEA, 0x00, 0x40,  // the hours
    0xFA, 0x00, 0xA0, 0xEA, 0x02, 0xC0,
    0x18, 0xD5,                    // JR loop
];

/// Copies 0xC100 to 0xC000 and the ROM byte at 0x0200 to 0xC001, forever,
/// so cheats on either show.
#[rustfmt::skip]
pub const CHEATS: &[u8] = &[
    0xFA, 0x00, 0xC1, 0xEA, 0x00, 0xC0,  // loop: 0xC000 = (0xC100)
    0xFA, 0x00, 0x02, 0xEA, 0x01, 0xC0,  // 0xC001 = (0x0200)
    0x18, 0xF2,                          // JR loop
];

/// Switches the rumble motor on and waits forever.
#[rustfmt::skip]
pub const RUMBLE: &[u8] = &[
    0x3E, 0x08, 0xEA, 0x00, 0x40,  // 0x4000 = 0x08: the motor on
    0x18, 0xFE,                    // JR -2
];

/// Plays a loud tone on channel 1 and waits forever.
#[rustfmt::skip]
pub const TONE: &[u8] = &[
    0x3E, 0x80, 0xE0, 0x26,  // NR52: sound on
    0x3E, 0x77, 0xE0, 0x24,  // NR50: full volume
    0x3E, 0xFF, 0xE0, 0x25,  // NR51: every channel on both sides
    0x3E, 0xF0, 0xE0, 0x12,  // NR12: loud, no envelope
    0x3E, 0x80, 0xE0, 0x11,  // NR11: half duty
    0xAF, 0xE0, 0x13,        // NR13
    0x3E, 0x87, 0xE0, 0x14,  // NR14: trigger
    0x18, 0xFE,              // JR -2
];

/// A cartridge of `cartridge_type` with `ram_size` (the header byte) that
/// runs `program`, for the Game Boy or, with `cgb`, the Game Boy Color too.
pub fn cartridge(cartridge_type: u8, ram_size: u8, cgb: bool, program: &[u8]) -> Vec<u8> {
    let mut rom = vec![0; ROM_SIZE];
    rom[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]); // NOP; JP 0x0150
    rom[0x0134..0x0134 + 9].copy_from_slice(b"NUBO TEST");
    rom[0x0143] = if cgb { 0x80 } else { 0x00 };
    rom[0x0147] = cartridge_type;
    rom[0x0149] = ram_size;
    rom[0x014D] = rom[0x0134..0x014D]
        .iter()
        .fold(0u8, |sum, byte| sum.wrapping_sub(*byte).wrapping_sub(1));
    rom[PROGRAM_START..PROGRAM_START + program.len()].copy_from_slice(program);
    rom[0x0200] = ROM_BYTE_AT_0200;
    rom
}
