use retroboy_core::emulator::CartridgeEffects;
use retroboy_core::emulator::RTCState;
use crate::api::{current_time_millis, load_rtc_state, save_rtc_state, load_ram, save_ram};
use crate::wasm_rtc_state::WasmRTCState;

pub struct WasmCartridgeEffects;

impl CartridgeEffects for WasmCartridgeEffects {
    fn current_time_millis(&self) -> f64 {
        current_time_millis()
    }

    fn load_rtc_state(&self, key: &str) -> Option<RTCState> {
        load_rtc_state(key).map(|state| {
            RTCState {
                milliseconds: state.milliseconds() as u16,
                seconds: state.seconds(),
                minutes: state.minutes(),
                hours: state.hours(),
                days: state.days(),
                base_timestamp: state.base_timestamp(),
                halted: state.halted(),
                day_carry: state.day_carry()
            }
        })
    }

    fn save_rtc_state(&self, key: &str, state: &RTCState) {
        save_rtc_state(key, WasmRTCState::new(
            state.milliseconds as u16,
            state.seconds,
            state.minutes,
            state.hours,
            state.days,
            state.base_timestamp,
            state.halted,
            state.day_carry
        ));
    }

    fn load_ram(&self, key: &str) -> Option<Vec<u8>> {
        load_ram(key)
    }

    fn save_ram(&self, key: &str, ram: &[u8]) {
        save_ram(key, ram);
    }
}
