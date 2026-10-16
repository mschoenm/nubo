//! Finding and downloading box art in the browser (the `web` feature), with
//! the browser's `fetch`. The pictures come from the libretro thumbnail
//! collection's copy on GitHub, which lets pages read them, and the game
//! lists are kept in the browser's IndexedDB as `box_art/lists/<system>.dat`
//! (see `storage::indexed_db`).

use super::names::{
    GAME_BOY, GAME_BOY_COLOR, GameList, MAX_DOWNLOAD_SIZE, github_box_art_url, list_url, shrink,
    systems,
};
use crate::storage::indexed_db;
use std::path::Path;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// Reads the No-Intro game lists, downloading the ones that aren't kept
/// yet.
pub async fn load_game_list() -> Result<GameList, String> {
    let mut games = GameList::default();

    for system in [GAME_BOY, GAME_BOY_COLOR] {
        let key = format!("box_art/lists/{system}.dat");
        let list = match indexed_db::read(&key) {
            Some(record) => String::from_utf8_lossy(&record.bytes).into_owned(),
            None => {
                let bytes = download(&list_url(system))
                    .await?
                    .ok_or("the game list wasn't found")?;
                indexed_db::write(&key, &bytes)?;
                String::from_utf8_lossy(&bytes).into_owned()
            }
        };
        games.add(&list);
    }

    Ok(games)
}

/// Looks up the box art of the game at `rom_path` and returns it as a small
/// PNG, or `None` if the collection has none. Fails only when the download
/// itself fails, such as without internet, so the game can be tried again
/// later.
pub async fn find(
    rom_path: &Path,
    supports_cgb: bool,
    games: &GameList,
) -> Result<Option<Vec<u8>>, String> {
    let file_stem = rom_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();

    let checksum = if games.needs_checksum(&file_stem) {
        crate::rom::load(rom_path.to_path_buf())
            .await
            .ok()
            .and_then(|rom| nubo_core::extract_rom(&rom.bytes).ok())
            .map(|rom| crc32fast::hash(&rom))
    } else {
        None
    };
    let names = games.names_for(&file_stem, || checksum);

    for name in &names {
        for system in systems(supports_cgb) {
            if let Some(picture) = download(&github_box_art_url(system, name)).await? {
                return Ok(shrink(&picture));
            }
        }
    }

    Ok(None)
}

/// Downloads `url`, returning `None` if the server doesn't have it.
async fn download(url: &str) -> Result<Option<Vec<u8>>, String> {
    let window = web_sys::window().ok_or("there is no window")?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|_| String::from("the server couldn't be reached"))?
        .dyn_into()
        .map_err(|_| String::from("the browser gave no answer"))?;

    match response.status() {
        200 => {}
        404 => return Ok(None),
        code => return Err(format!("the server answered {code}")),
    }
    let buffer = response
        .array_buffer()
        .map_err(|_| String::from("the answer couldn't be read"))?;
    let bytes = JsFuture::from(buffer)
        .await
        .map_err(|_| String::from("the answer couldn't be read"))?;
    let bytes = js_sys::Uint8Array::new(&bytes);
    if u64::from(bytes.length()) > MAX_DOWNLOAD_SIZE {
        return Err(String::from("the answer was too big"));
    }
    Ok(Some(bytes.to_vec()))
}
