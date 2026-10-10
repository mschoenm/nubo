use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct WasmRTCState {
    milliseconds: u16,
    seconds: u8,
    minutes: u8,
    hours: u8,
    days: u16,
    base_timestamp: f64,
    halted: bool,
    day_carry: bool
}

#[wasm_bindgen]
impl WasmRTCState {
    #[wasm_bindgen(constructor)]
    pub fn new(
        milliseconds: u16,
        seconds: u8,
        minutes: u8,
        hours: u8,
        days: u16,
        base_timestamp: f64,
        halted: bool,
        day_carry: bool
    ) -> WasmRTCState {
        WasmRTCState {
            milliseconds,
            seconds,
            minutes,
            hours,
            days,
            base_timestamp,
            halted,
            day_carry
        }
    }

    #[wasm_bindgen(getter)]
    pub fn milliseconds(&self) -> u16 {
        self.milliseconds
    }

    #[wasm_bindgen(getter)]
    pub fn seconds(&self) -> u8 {
        self.seconds
    }

    #[wasm_bindgen(getter)]
    pub fn minutes(&self) -> u8 {
        self.minutes
    }

    #[wasm_bindgen(getter)]
    pub fn hours(&self) -> u8 {
        self.hours
    }

    #[wasm_bindgen(getter)]
    pub fn days(&self) -> u16 {
        self.days
    }

    #[wasm_bindgen(getter)]
    pub fn base_timestamp(&self) -> f64 {
        self.base_timestamp
    }

    #[wasm_bindgen(getter)]
    pub fn halted(&self) -> bool {
        self.halted
    }

    #[wasm_bindgen(getter)]
    pub fn day_carry(&self) -> bool {
        self.day_carry
    }
}