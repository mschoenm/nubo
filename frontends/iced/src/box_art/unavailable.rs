//! The browser build downloads no box art.

use std::path::Path;

/// The browser build has no game lists.
#[derive(Debug, Default)]
pub struct GameList;

pub fn load_game_list() -> Result<GameList, String> {
    Err(String::from("box art isn't available in the browser"))
}

#[expect(clippy::unnecessary_wraps, reason = "matches the desktop build")]
pub fn find(
    _rom_path: &Path,
    _supports_cgb: bool,
    _games: &GameList,
) -> Result<Option<Vec<u8>>, String> {
    Ok(None)
}
