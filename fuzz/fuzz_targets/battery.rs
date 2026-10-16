//! Any battery save (cartridge RAM of any length and any cartridge clock)
//! loads without a panic on every cartridge type, and the game runs and
//! saves a state that loads again.
#![no_main]

use libfuzzer_sys::fuzz_target;
use nubo_core::{RtcClock, RtcState};
use nubo_fuzz::{booted, effects, minimal_rom, model_from_byte, run_frames};

/// ROM only, MBC1, MBC2, MBC3, MBC5, MBC7, HuC1 and the rest, with and
/// without RAM, a battery or a clock, and one code no cartridge uses.
const CARTRIDGE_TYPES: [u8; 14] = [
    0x00, 0x01, 0x03, 0x05, 0x06, 0x0F, 0x10, 0x11, 0x13, 0x19, 0x1B, 0x1E, 0x22, 0xFF,
];

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 + RtcState::BYTES {
        return;
    }
    let cartridge_type = CARTRIDGE_TYPES[usize::from(data[0]) % CARTRIDGE_TYPES.len()];
    let ram_size = data[1] % 6;
    let model = model_from_byte(data[2]);
    let clock = if data[3] & 1 == 0 {
        RtcClock::WallClock
    } else {
        RtcClock::EmulatedTime
    };
    let rtc = RtcState::from_bytes(&data[4..4 + RtcState::BYTES]);
    let ram = &data[4 + RtcState::BYTES..];
    let rom = minimal_rom(cartridge_type, 1, ram_size, true);
    let Ok(mut emulator) = booted(model).clone_with_rom(&rom, effects()) else {
        return;
    };
    emulator.set_cartridge_ram(ram);
    if let Some(rtc) = rtc {
        emulator.set_rtc_state(&rtc, clock);
    }
    run_frames(&mut emulator, 2);
    let _ = emulator.rtc_state(RtcClock::WallClock);
    let _ = emulator.rtc_state(RtcClock::EmulatedTime);
    let _ = emulator.take_rumble_level();
    let state = emulator.save_state();
    emulator
        .load_state(&state)
        .expect("a state the emulator saved loads again");
});
