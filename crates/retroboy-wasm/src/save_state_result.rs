use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct SaveStateResult {
    error: Option<String>,
    save_state: Option<Vec<u8>>
}

#[wasm_bindgen]
impl SaveStateResult {
    #[wasm_bindgen(constructor)]
    pub fn new(error: Option<String>, save_state: Option<Vec<u8>>) -> SaveStateResult {
        SaveStateResult {
            error,
            save_state
        }
    }
    #[wasm_bindgen(getter)]
    pub fn error(&self) -> Option<String> {
        self.error.clone()
    }

    #[wasm_bindgen(getter, js_name=saveState)]
    pub fn save_state(&self) -> Option<Vec<u8>> {
        self.save_state.clone()
    }
}