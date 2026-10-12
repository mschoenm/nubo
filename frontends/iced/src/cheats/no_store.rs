//! The browser build keeps no cheats yet.

use super::Cheat;
use std::path::Path;

#[expect(clippy::unnecessary_wraps, reason = "matches the file store")]
pub fn load(_rom_path: &Path) -> Result<Vec<Cheat>, String> {
    Ok(Vec::new())
}

pub fn save(_rom_path: &Path, _cheats: &[Cheat]) -> Result<(), String> {
    Err(String::from("cheats aren't available in the browser"))
}
