//! Any ROM or zip file loads or fails with an error, and any program runs
//! without a panic. The first byte picks the model.
#![no_main]

use libfuzzer_sys::fuzz_target;
use nubo_core::{Emulator, Key};
use nubo_fuzz::{model_from_byte, run_frames, start};

fuzz_target!(|data: &[u8]| {
    let Some((&selector, rom)) = data.split_first() else {
        return;
    };
    let _ = Emulator::read_rom_header(rom);
    let Some(mut emulator) = start(model_from_byte(selector), rom) else {
        return;
    };
    emulator.press_key(Key::Start);
    run_frames(&mut emulator, 2);
    let state = emulator.save_state();
    emulator
        .load_state(&state)
        .expect("a state the emulator saved loads again");
    let _ = emulator.take_frame();
    let _ = emulator.cartridge_ram();
});
