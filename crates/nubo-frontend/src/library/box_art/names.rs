//! What finding box art needs on every platform: the official names of
//! games from the No-Intro game lists, the addresses of their pictures in the
//! libretro thumbnail collection, and shrinking a downloaded picture.

#[cfg(any(feature = "desktop", feature = "web"))]
use image::ImageFormat;
#[cfg(any(feature = "desktop", feature = "web"))]
use image::imageops::FilterType;
use std::collections::{HashMap, HashSet};
#[cfg(any(feature = "desktop", feature = "web"))]
use std::io::Cursor;

/// The systems the libretro collections are split into, by their names
/// there.
#[cfg(any(feature = "desktop", feature = "web", test))]
pub const GAME_BOY: &str = "Nintendo - Game Boy";
#[cfg(any(feature = "desktop", feature = "web", test))]
pub const GAME_BOY_COLOR: &str = "Nintendo - Game Boy Color";

/// Characters that libretro's picture file names replace with `_`.
#[cfg(any(feature = "desktop", feature = "web", test))]
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

    /// Whether a ROM file called `file_stem` (without its extension) is
    /// named neither like a game nor like a file in the list, so its
    /// checksum is needed to find its game.
    pub fn needs_checksum(&self, file_stem: &str) -> bool {
        !self.names.contains(file_stem) && !self.by_file_name.contains_key(file_stem)
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
        if self.needs_checksum(file_stem) {
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

/// The address of the box art called `name` in the `system` collection on
/// thumbnails.libretro.com, where the desktop build downloads it.
#[cfg(any(feature = "desktop", test))]
pub fn box_art_url(system: &str, name: &str) -> String {
    format!(
        "https://thumbnails.libretro.com/{}/Named_Boxarts/{}.png",
        percent_encode(system),
        percent_encode(&picture_name(name))
    )
}

/// The address of the box art called `name` in the `system` collection's
/// copy on GitHub, where the browser build downloads it: browsers only let a
/// page read pictures from servers that allow it, which GitHub's does.
#[cfg(any(all(feature = "web", not(feature = "desktop")), test))]
pub fn github_box_art_url(system: &str, name: &str) -> String {
    format!(
        "https://raw.githubusercontent.com/libretro-thumbnails/{}/master/Named_Boxarts/{}.png",
        system.replace(' ', "_"),
        percent_encode(&picture_name(name))
    )
}

/// The address of the No-Intro game list of `system`.
#[cfg(any(feature = "desktop", feature = "web", test))]
pub fn list_url(system: &str) -> String {
    format!("{LISTS_URL}/{}.dat", percent_encode(system))
}

/// The file name (without `.png`) libretro gives the picture of the game
/// called `name`, with the characters file names can't have replaced.
#[cfg(any(feature = "desktop", feature = "web", test))]
fn picture_name(name: &str) -> String {
    name.chars()
        .map(|character| {
            if REPLACED_CHARACTERS.contains(&character) {
                '_'
            } else {
                character
            }
        })
        .collect()
}

/// Writes every byte that isn't a letter, digit or one of `-._~` as `%XX`,
/// as a URL path needs.
#[cfg(any(feature = "desktop", feature = "web", test))]
pub fn percent_encode(text: &str) -> String {
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
#[cfg(any(feature = "desktop", feature = "web", test))]
pub fn systems(supports_cgb: bool) -> [&'static str; 2] {
    if supports_cgb {
        [GAME_BOY_COLOR, GAME_BOY]
    } else {
        [GAME_BOY, GAME_BOY_COLOR]
    }
}

/// Where the libretro database keeps the No-Intro game lists.
#[cfg(any(feature = "desktop", feature = "web", test))]
const LISTS_URL: &str =
    "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro";
/// Box art is shrunk to fit in a square this big, which is plenty for a
/// library card and keeps memory use low.
#[cfg(any(feature = "desktop", feature = "web"))]
const PICTURE_SIZE: u32 = 256;
/// Nothing this app downloads is anywhere near this big.
#[cfg(any(feature = "desktop", feature = "web"))]
pub const MAX_DOWNLOAD_SIZE: u64 = 16 * 1024 * 1024;

/// Shrinks a picture to fit in `PICTURE_SIZE` and saves it as PNG, or
/// returns `None` if it isn't a picture this app can read.
#[cfg(any(feature = "desktop", feature = "web"))]
pub fn shrink(picture: &[u8]) -> Option<Vec<u8>> {
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

#[cfg(test)]
mod tests;
