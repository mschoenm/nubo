use core::fmt::Debug;
use crate::address_bus::mbc3::RTCState; 

pub trait CartridgeEffects {
    fn current_time_millis(&self) -> f64;
    fn load_rtc_state(&self, key: &str) -> Option<RTCState>;
    fn save_rtc_state(&self, key: &str, state: &RTCState);
    fn load_ram(&self, key: &str) -> Option<Vec<u8>>;
    fn save_ram(&self, key: &str, ram: &[u8]);
}

pub(super) struct EmptyCartridgeEffects;

impl CartridgeEffects for EmptyCartridgeEffects {
    fn current_time_millis(&self) -> f64 {
        0.0
    }

    fn load_rtc_state(&self, _: &str) -> Option<RTCState> {
        None
    }

    fn save_rtc_state(&self, _: &str, _: &RTCState) {}

    fn load_ram(&self, _: &str) -> Option<Vec<u8>> {
        None
    }

    fn save_ram(&self, _: &str, _: &[u8]) {}
}

impl Debug for dyn CartridgeEffects {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "CartridgeEffects")
    }
}
pub(crate) fn empty_cartridge_effects() -> Box<dyn CartridgeEffects> {
    Box::new(EmptyCartridgeEffects {})
}