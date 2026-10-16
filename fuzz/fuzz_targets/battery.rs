//! Any battery save (cartridge RAM of any length and any cartridge clock)
//! loads without a panic on every cartridge type, and the game runs and
//! saves a state that loads again.
#![no_main]

use libfuzzer_sys::fuzz_target;
use nubo_core::{Emulator, Model, RtcClock, RtcState};
use nubo_fuzz::{effects, minimal_rom, model_from_byte, run_frames, start};
use std::sync::OnceLock;

/// Every cartridge type code Nubo loads, found by trying them all, so the
/// list follows the cartridges Nubo supports.
fn cartridge_types() -> &'static [u8] {
    static TYPES: OnceLock<Vec<u8>> = OnceLock::new();
    TYPES.get_or_init(|| {
        (0..=u8::MAX)
            .filter(|&code| {
                Emulator::new(Model::Dmg)
                    .load_rom(&minimal_rom(code, 1, 0, true), effects())
                    .is_ok()
            })
            .collect()
    })
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 + RtcState::BYTES {
        return;
    }
    let cartridge_type = cartridge_types()[usize::from(data[0]) % cartridge_types().len()];
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
    let Some(mut emulator) = start(model, &rom) else {
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
