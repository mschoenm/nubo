use wasm_bindgen::prelude::*;

#[derive(Clone)]
#[wasm_bindgen]
pub struct EmulatorSettings {
    mode: String,
    audio_sample_rate: u32,
}

#[wasm_bindgen]
impl EmulatorSettings {
    #[wasm_bindgen(constructor)]
    pub fn new(mode: String,
               audio_sample_rate: u32) -> EmulatorSettings {
        EmulatorSettings {
            mode,
            audio_sample_rate
        }
    }
    
    #[wasm_bindgen(getter)]
    pub fn mode(&self) -> String {
        self.mode.clone()
    }

    #[wasm_bindgen(getter, js_name = audioSampleRate)]
    pub fn audio_sample_rate(&self) -> u32 {
        self.audio_sample_rate
    }
}
