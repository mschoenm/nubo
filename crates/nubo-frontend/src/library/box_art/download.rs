//! Finding and downloading box art (the `desktop` feature), with `ureq`. The game
//! lists are kept in `box_art/lists` in the user's data folder.

use crate::storage::files::{project_directories, write_atomically};
use image::ImageFormat;
use image::imageops::FilterType;
use nubo_core::extract_rom;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

/// The systems the libretro collections are split into, by their names
/// there.
const GAME_BOY: &str = "Nintendo - Game Boy";
const GAME_BOY_COLOR: &str = "Nintendo - Game Boy Color";

/// Characters that libretro's picture file names replace with `_`.
const REPLACED_CHARACTERS: &[char] = &['&', '*', '/', ':', '`', '<', '>', '?', '\\', '|', '"'];

/// The official names of games, from the No-Intro game lists.
#[derive(Debug, Default)]
pub struct GameList {
    /// Every game's official name.
    names: HashSet<String>,
    by_checksum: HashMap<u32, String>,
    /// Official names by ROM file name without extension, which can differ
    /// from the game's name, such as "(Unl)" added at the end.
    by_file_name: HashMap<String, String>,
}

impl GameList {
    /// Adds the games of a No-Intro list in clrmamepro format: a
    /// `game ( ... )` block per game with its `name` and a `rom ( ... )`
    /// line with the file's name and `crc`.
    pub fn add(&mut self, list: &str) {
        let mut game_name: Option<String> = None;

        for line in list.lines().map(str::trim) {
            if line.starts_with("game (") {
                game_name = None;
            } else if let Some(rest) = line.strip_prefix("name ") {
                game_name = quoted(rest).map(str::to_owned);
                if let Some(name) = &game_name {
                    self.names.insert(name.clone());
                }
            } else if let Some(rom) = line.strip_prefix("rom (")
                && let Some(name) = &game_name
            {
                if let Some(checksum) =
                    field(rom, "crc").and_then(|crc| u32::from_str_radix(crc, 16).ok())
                {
                    self.by_checksum.insert(checksum, name.clone());
                }
                if let Some(file_name) = rom
                    .strip_prefix(" name ")
                    .and_then(quoted)
                    .and_then(|file| file.rsplit_once('.'))
                    .map(|(stem, _)| stem)
                {
                    self.by_file_name.insert(file_name.to_owned(), name.clone());
                }
            }
        }
    }

    /// The names a game's picture may have: its file name, then the
    /// official names for that file name and for the ROM's checksum.
    /// `checksum` is only worked out when the file name isn't in the list,
    /// which saves reading ROMs that are named the No-Intro way.
    pub fn names_for(
        &self,
        file_stem: &str,
        checksum: impl FnOnce() -> Option<u32>,
    ) -> Vec<String> {
        let mut names = vec![file_stem.to_owned()];
        let mut add = |name: Option<&String>| {
            if let Some(name) = name
                && !names.contains(name)
            {
                names.push(name.clone());
            }
        };

        add(self.by_file_name.get(file_stem));
        if !self.names.contains(file_stem) && !self.by_file_name.contains_key(file_stem) {
            add(checksum().and_then(|checksum| self.by_checksum.get(&checksum)));
        }
        names
    }
}

/// The text between the first pair of double quotes in `text`.
fn quoted(text: &str) -> Option<&str> {
    let start = text.find('"')? + 1;
    let length = text[start..].find('"')?;
    Some(&text[start..start + length])
}

/// The value after `key` in a line such as `size 32768 crc 46DF91AD md5 …`.
fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let mut words = line.split_whitespace();
    words.find(|&word| word == key)?;
    words.next()
}

/// The address of the box art called `name` in the `system` collection.
fn box_art_url(system: &str, name: &str) -> String {
    let file_name: String = name
        .chars()
        .map(|character| {
            if REPLACED_CHARACTERS.contains(&character) {
                '_'
            } else {
                character
            }
        })
        .collect();

    format!(
        "https://thumbnails.libretro.com/{}/Named_Boxarts/{}.png",
        percent_encode(system),
        percent_encode(&file_name)
    )
}

/// Writes every byte that isn't a letter, digit or one of `-._~` as `%XX`,
/// as a URL path needs.
fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

/// The collections to look in, the one a game most likely belongs to
/// first. Many games that work on both systems are listed as Color games.
fn systems(supports_cgb: bool) -> [&'static str; 2] {
    if supports_cgb {
        [GAME_BOY_COLOR, GAME_BOY]
    } else {
        [GAME_BOY, GAME_BOY_COLOR]
    }
}

/// Where the libretro database keeps the No-Intro game lists.
const LISTS_URL: &str =
    "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro";
/// Box art is shrunk to fit in a square this big, which is plenty for a
/// library card and keeps memory use low.
const PICTURE_SIZE: u32 = 256;
/// Nothing this app downloads is anywhere near this big.
const MAX_DOWNLOAD_SIZE: u64 = 16 * 1024 * 1024;
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
                let url = format!("{LISTS_URL}/{}.dat", percent_encode(system));
                let bytes = download(&url)?.ok_or("the game list wasn't found")?;
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

/// Shrinks a picture to fit in `PICTURE_SIZE` and saves it as PNG, or
/// returns `None` if it isn't a picture this app can read.
fn shrink(picture: &[u8]) -> Option<Vec<u8>> {
    let mut picture = image::load_from_memory(picture).ok()?;
    if picture.width() > PICTURE_SIZE || picture.height() > PICTURE_SIZE {
        picture = picture.resize(PICTURE_SIZE, PICTURE_SIZE, FilterType::Triangle);
    }

    let mut png = Vec::new();
    picture
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .ok()?;
    Some(png)
}

fn lists_folder() -> Result<PathBuf, String> {
    project_directories()
        .map(|directories| directories.data_dir().join("box_art").join("lists"))
        .ok_or_else(|| String::from("the system has no data folder for us"))
}

#[cfg(test)]
mod tests;
