//! Builds without the `desktop` or `web` feature, such as the RetroArch
//! core's, download no box art.

use super::names::GameList;
use std::path::Path;

/// Fails: without the `desktop` or `web` feature there are no game lists to
/// read.
pub fn load_game_list() -> Result<GameList, String> {
    Err(String::from("this build downloads no box art"))
}

/// Finds nothing: without the `desktop` or `web` feature no box art is
/// downloaded.
pub fn find(
    _rom_path: &Path,
    _supports_cgb: bool,
    _games: &GameList,
) -> Result<Option<Vec<u8>>, String> {
    Ok(None)
}
