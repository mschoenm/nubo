use wasm_bindgen::prelude::*;

#[derive(Clone)]
#[wasm_bindgen]
pub struct RomMetadata {
    title: String,
    has_battery: bool
}

#[wasm_bindgen]
impl RomMetadata {
    #[wasm_bindgen(constructor)]
    pub fn new(title: String, has_battery: bool) -> RomMetadata {
        RomMetadata {
            title,
            has_battery
        }
    }

    #[wasm_bindgen(getter)]
    pub fn title(&self) -> String {
        self.title.clone()
    }

    #[wasm_bindgen(getter, js_name = hasBattery)]
    pub fn has_battery(&self) -> bool {
        self.has_battery
    }
}

#[wasm_bindgen]
pub struct RomMetadataResult {
    error: Option<String>,
    metadata: Option<RomMetadata>
}

#[wasm_bindgen]
impl RomMetadataResult {
    #[wasm_bindgen(constructor)]
    pub fn new(error: Option<String>, metadata: Option<RomMetadata>) -> RomMetadataResult {
        RomMetadataResult {
            error,
            metadata
        }
    }
    
    #[wasm_bindgen(getter)]
    pub fn error(&self) -> Option<String> {
        self.error.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn metadata(&self) -> Option<RomMetadata> {
        self.metadata.clone()
    }
}