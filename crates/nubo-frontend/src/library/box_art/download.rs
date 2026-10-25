//! Finding and downloading box art (the `desktop` feature), with `ureq`. The
//! game lists are kept in `box_art/lists` in the user's data folder.

use super::names::{
    GAME_BOY, GAME_BOY_COLOR, GameList, MAX_DOWNLOAD_SIZE, box_art_url, list_url, shrink, systems,
};
use crate::storage::files::{data_folder, write_atomically};
use nubo_core::extract_rom;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

/// How long a download may take.
const TIMEOUT: Duration = Duration::from_secs(20);

/// The HTTP client, which finds a proxy in the `HTTPS_PROXY` variable and
/// trusts the certificates the system trusts, as browsers do.
static AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .user_agent(concat!("Nubo/", env!("CARGO_PKG_VERSION")))
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            )
            .build(),
    )
});

/// Reads the No-Intro game lists, downloading the ones that aren't saved
/// yet.
pub fn load_game_list() -> Result<GameList, String> {
    let folder = lists_folder()?;
    let mut games = GameList::default();

    for system in [GAME_BOY, GAME_BOY_COLOR] {
        let path = folder.join(format!("{system}.dat"));
        let list = match fs::read_to_string(&path) {
            Ok(list) => list,
            Err(_) => {
                let bytes = download(&list_url(system))?.ok_or("the game list wasn't found")?;
                write_atomically(&path, &bytes).map_err(|error| error.to_string())?;
                String::from_utf8_lossy(&bytes).into_owned()
            }
        };
        games.add(&list);
    }

    Ok(games)
}

/// Looks up the box art of the ROM at `rom_path` and returns it as a small
/// PNG, or `None` if the collection has none. Fails only when the download
/// itself fails, such as without internet, so the game can be tried again
/// later.
pub fn find(
    rom_path: &Path,
    supports_cgb: bool,
    games: &GameList,
) -> Result<Option<Vec<u8>>, String> {
    let file_stem = rom_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();

    let names = games.names_for(&file_stem, || {
        let data = fs::read(rom_path).ok()?;
        Some(crc32fast::hash(&extract_rom(&data).ok()?))
    });

    for name in &names {
        for system in systems(supports_cgb) {
            if let Some(picture) = download(&box_art_url(system, name))? {
                return Ok(shrink(&picture));
            }
        }
    }

    Ok(None)
}

/// Downloads `url`, returning `None` if the server doesn't have it.
fn download(url: &str) -> Result<Option<Vec<u8>>, String> {
    match AGENT.get(url).call() {
        Ok(response) => {
            let mut bytes = Vec::new();
            response
                .into_body()
                .into_reader()
                .take(MAX_DOWNLOAD_SIZE)
                .read_to_end(&mut bytes)
                .map_err(|error| error.to_string())?;
            Ok(Some(bytes))
        }
        Err(ureq::Error::StatusCode(404)) => Ok(None),
        Err(ureq::Error::StatusCode(code)) => Err(format!("the server answered {code}")),
        Err(error) => Err(error.to_string()),
    }
}

/// The folder the downloaded game lists are kept in: `box_art/lists` in the
/// player's data folder.
fn lists_folder() -> Result<PathBuf, String> {
    data_folder()
        .map(|folder| folder.join("box_art").join("lists"))
        .ok_or_else(|| String::from("the system has no data folder for us"))
}
