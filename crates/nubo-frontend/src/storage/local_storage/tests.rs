//! Tests for the browser's keys. They only check the names, which must stay
//! the same so players' saves in the browser keep loading; reading and
//! writing needs a browser.

use super::*;
use std::path::Path;

#[test]
fn settings_keep_their_key() {
    assert_eq!(
        LocalStorage::key(Entry::Settings).unwrap(),
        "nubo:settings.toml"
    );
}

#[test]
fn battery_saves_are_keyed_by_the_rom_name() {
    let rom_path = Path::new("Pocket Game.gbc");

    assert_eq!(
        LocalStorage::key(Entry::BatteryRam(rom_path)).unwrap(),
        "nubo:Pocket Game.sav"
    );
    assert_eq!(
        LocalStorage::key(Entry::BatteryClock(rom_path)).unwrap(),
        "nubo:Pocket Game.rtc"
    );
}

#[test]
fn cheats_are_not_kept() {
    let error = LocalStorage::key(Entry::Cheats(Path::new("game.gb"))).unwrap_err();

    assert_eq!(error, "cheats aren't available in the browser");
}
