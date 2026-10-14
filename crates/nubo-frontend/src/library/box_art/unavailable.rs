//! Builds without the `desktop` feature, such as the browser build, download
//! no box art.

use std::path::Path;

/// Without the `desktop` feature there are no game lists.
#[derive(Debug, Default)]
pub struct GameList;

/// Fails: without the `desktop` feature there are no game lists to read.
pub fn load_game_list() -> Result<GameList, String> {
    Err(String::from("box art isn't available in the browser"))
}

/// Finds nothing: without the `desktop` feature no box art is downloaded.
pub fn find(
    _rom_path: &Path,
    _supports_cgb: bool,
    _games: &GameList,
) -> Result<Option<Vec<u8>>, String> {
    Ok(None)
}
