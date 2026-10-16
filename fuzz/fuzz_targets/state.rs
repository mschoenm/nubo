//! Any save state of the benchmark game loads or fails with an error; one
//! that loads runs, and saves again into a state that loads. Seed it with
//! the game's kept states (see AGENTS.md).
#![no_main]

use libfuzzer_sys::fuzz_target;
use nubo_core::SaveStateHeader;
use nubo_fuzz::{game, run_frames, start};

fuzz_target!(|data: &[u8]| {
    let Ok(header) = SaveStateHeader::read(data) else {
        return;
    };
    let mut emulator = start(header.model, game()).expect("the benchmark game loads");
    if emulator.load_state(data).is_err() {
        return;
    }
    run_frames(&mut emulator, 2);
    let again = emulator.save_state();
    emulator
        .load_state(&again)
        .expect("a state the emulator saved loads again");
});
