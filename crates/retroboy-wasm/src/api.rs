use retroboy_core::emulator::Emulator;
use retroboy_core::emulator::CartridgeHeader;
use retroboy_core::emulator::Key;
use crate::emulator_settings::EmulatorSettings;
use crate::rom_metadata::{RomMetadata, RomMetadataResult};
use crate::save_state_result::SaveStateResult;
use crate::wasm_cartridge_effects::WasmCartridgeEffects;
use crate::wasm_rtc_state::WasmRTCState;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    pub fn log(s: &str);

    #[wasm_bindgen(js_name = canvasRender)]
    pub fn canvas_render(frame_buffer: &[u8]); 

    #[wasm_bindgen(js_name = playAudioSamples)]
    pub fn play_audio_samples(left_samples: &[f32], right_samples: &[f32]);

    #[wasm_bindgen(js_name = currentTimeMillis)]
    pub fn current_time_millis() -> f64;

    #[wasm_bindgen(js_name = loadRTCState)]
    pub fn load_rtc_state(key: &str) -> Option<WasmRTCState>;

    #[wasm_bindgen(js_name = saveRTCState)]
    pub fn save_rtc_state(key: &str, value: WasmRTCState);

    #[wasm_bindgen(js_name = loadRam)]
    pub fn load_ram(key: &str) -> Option<Vec<u8>>;

    #[wasm_bindgen(js_name = saveRam)]
    pub fn save_ram(key: &str, value: &[u8]);
}

fn initialize_web_emulator() -> Emulator {
    Emulator::new(false)
}

thread_local! {
    pub static EMULATOR: RefCell<Emulator> = RefCell::new(initialize_web_emulator());
}

extern crate console_error_panic_hook;

fn as_rom_metadata(header: CartridgeHeader) -> RomMetadata {
    RomMetadata::new(header.title, header.has_battery)
}

#[wasm_bindgen(js_name = initializeEmulator)]
pub fn initialize_emulator(rom_buffer: &[u8], settings: EmulatorSettings) -> RomMetadataResult {
    EMULATOR.with(|emulator_cell: &RefCell<Emulator>| {
        console_error_panic_hook::set_once();

        let mut emulator = emulator_cell.borrow_mut();

        emulator.set_cgb_mode(settings.mode().as_str() == "CGB");

        emulator.set_sample_rate(settings.audio_sample_rate());

        let wasm_cartridge_effects= WasmCartridgeEffects {};

        match emulator.load_rom(rom_buffer, Box::new(wasm_cartridge_effects)) {
            Ok(result) => {
                log("Emulator initialized!");
                RomMetadataResult::new(None, Some(as_rom_metadata(result)))
            }
            Err(error) => {
                log(&format!("Error loading ROM: {}", error.to_string()));
                RomMetadataResult::new(Some(error.to_string()), None)
            }
        }
    })
}

#[wasm_bindgen(js_name = resetEmulator)]
pub fn reset_emulator() {
    EMULATOR.with(|emulator_cell| {
         emulator_cell.replace(initialize_web_emulator());
    })
}

#[wasm_bindgen(js_name = stepUntilNextAudioBuffer)]
pub fn step_until_next_audio_buffer() {
    EMULATOR.with(|emulator_cell| {
        let mut emulator = emulator_cell.borrow_mut();

        let audio_buffers = emulator.step_until_next_audio_buffer();
        let left_samples_slice = audio_buffers.0;
        let right_samples_slice = audio_buffers.1;

        play_audio_samples(left_samples_slice, right_samples_slice);

        if let Some(frame) = emulator.take_frame() {
            canvas_render(frame);
        }
    })
}

const UP_CODE: &str = "Up";
const DOWN_CODE: &str = "Down";
const LEFT_CODE: &str = "Left";
const RIGHT_CODE: &str = "Right";
const START_CODE: &str = "Start";
const SELECT_CODE: &str = "Select";
const B_CODE: &str = "B";
const A_CODE: &str = "A";

fn as_maybe_key(key_code: &str) -> Option<Key> {
    match key_code {
        UP_CODE => Some(Key::Up),
        DOWN_CODE => Some(Key::Down),
        LEFT_CODE => Some(Key::Left),
        RIGHT_CODE => Some(Key::Right),
        START_CODE => Some(Key::Start),
        SELECT_CODE => Some(Key::Select),
        B_CODE => Some(Key::B),
        A_CODE => Some(Key::A),
        _ => None
    }
}

#[wasm_bindgen(js_name = pressKey)]
pub fn press_key(key_code: &str) {
    as_maybe_key(key_code).map(|key| {
        EMULATOR.with(|emulator_cell| {
            let mut emulator = emulator_cell.borrow_mut();
            emulator.handle_key_press(&key);
        })
    });
}


#[wasm_bindgen(js_name = releaseKey)]
pub fn release_key(key_code: &str) {
    as_maybe_key(key_code).map(|key| {
        EMULATOR.with(|emulator_cell| {
            let mut emulator = emulator_cell.borrow_mut();
            emulator.handle_key_release(&key);
        })
    });
}

#[wasm_bindgen(js_name = validateGamesharkCode)]
pub fn validate_gameshark_code(cheat: &str) -> Option<String> {
    retroboy_core::emulator::validate_gameshark_code(cheat)
}

#[wasm_bindgen(js_name = validateGamegenieCode)]
pub fn validate_gamegenie_code(cheat: &str) -> Option<String> {
    retroboy_core::emulator::validate_gamegenie_code(cheat)
}

#[wasm_bindgen(js_name = registerGamesharkCheat)]
pub fn register_gameshark_cheat(cheat_id: &str, cheat: &str) -> Option<String> {
    EMULATOR.with(|emulator_cell| {
        let mut emulator = emulator_cell.borrow_mut();
        emulator.register_gameshark_cheat(cheat_id, cheat)
    })
}

#[wasm_bindgen(js_name = registerGamegenieCheat)]
pub fn register_gamegenie_cheat(cheat_id: &str, cheat: &str) -> Option<String> {
    EMULATOR.with(|emulator_cell| {
        let mut emulator = emulator_cell.borrow_mut();
        emulator.register_gamegenie_cheat(cheat_id, cheat)
    })
}

#[wasm_bindgen(js_name = unregisterCheat)]
pub fn unregister_cheat(cheat_id: &str) {
    EMULATOR.with(|emulator_cell| {
        let mut emulator = emulator_cell.borrow_mut();
        emulator.unregister_cheat(cheat_id);
    })
}

#[wasm_bindgen(js_name = encodeSaveState)]
pub fn encode_save_state() -> SaveStateResult {
    EMULATOR.with(|emulator_cell| {
        let emulator = emulator_cell.borrow();
        match emulator.encode_save_state() {
            Ok(state_bytes) => SaveStateResult::new(None, Some(state_bytes)),
            Err(e) => SaveStateResult::new(Some(e.to_string()), None)
        }
    })
}

#[wasm_bindgen(js_name = applySaveState)]
pub fn apply_save_state(data: &[u8]) -> Option<String> {
    EMULATOR.with(|emulator_cell| {
        let mut emulator = emulator_cell.borrow_mut();
        match emulator.apply_save_state(data) {
            Ok(_) => None,
            Err(e) => Some(e.to_string())
        }
    })
}
