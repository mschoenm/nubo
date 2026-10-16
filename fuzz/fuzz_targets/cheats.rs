//! Any cheat text registers or fails with an error, and registered cheats
//! never make the game panic. Every line is one code, tried as a GameShark
//! code on even lines and a Game Genie code on odd ones.
#![no_main]

use libfuzzer_sys::fuzz_target;
use nubo_core::{validate_gamegenie_code, validate_gameshark_code};
use nubo_fuzz::{game, model_from_byte, run_frames, start};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let mut emulator = start(model_from_byte(0), game()).expect("the benchmark game loads");
    for (index, line) in text.lines().enumerate() {
        let _ = validate_gameshark_code(line);
        let _ = validate_gamegenie_code(line);
        let id = index.to_string();
        if index % 2 == 0 {
            let _ = emulator.register_gameshark_cheat(&id, line);
        } else {
            let _ = emulator.register_gamegenie_cheat(&id, line);
        }
    }
    run_frames(&mut emulator, 1);
    emulator.unregister_cheat("0");
    run_frames(&mut emulator, 1);
});
