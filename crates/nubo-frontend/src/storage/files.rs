//! Files in the per-user folders of the desktop build, from the
//! `directories` crate: the config folder holds `settings.toml`, and the data
//! folder everything else.
//!
//! | | Config folder | Data folder |
//! |---|---|---|
//! | Linux | `$XDG_CONFIG_HOME/nubo` (usually `~/.config/nubo`) | `$XDG_DATA_HOME/nubo` (usually `~/.local/share/nubo`) |
//! | macOS | `~/Library/Application Support/Nubo` | `~/Library/Application Support/Nubo` |
//! | Windows | `%APPDATA%\Nubo\config` | `%APPDATA%\Nubo\data` |
//!
//! In the data folder, battery saves go to `saves/`, cheats to `cheats/`,
//! save states to `states/` (see [`crate::save_states`]) and the library's
//! cache next to them.

use super::{Entry, Storage};
use directories::ProjectDirs;
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

/// Nubo's per-user folders, or `None` when the operating system has none
/// for us, for example because the home directory is unknown.
pub fn project_directories() -> Option<ProjectDirs> {
    ProjectDirs::from("", "", "Nubo")
}

/// [`Storage`] in files: `settings.toml` in the config folder, and battery
/// saves and cheats in the data folder.
#[derive(Debug, Clone, Default)]
pub struct Files {
    /// Where `settings.toml` goes, or `None` if there is no such folder.
    config_folder: Option<PathBuf>,
    /// Where everything else goes, or `None` if there is no such folder.
    data_folder: Option<PathBuf>,
}

impl Files {
    /// The player's own folders (see [`project_directories`]).
    pub fn for_user() -> Files {
        let directories = project_directories();
        Files {
            config_folder: directories
                .as_ref()
                .map(|directories| directories.config_dir().to_path_buf()),
            data_folder: directories.map(|directories| directories.data_dir().to_path_buf()),
        }
    }

    /// Files in the given folders instead of the player's, for tests and
    /// measurements that mustn't touch the player's data. `None` stands for
    /// a system without that folder.
    pub fn in_folders(config_folder: Option<PathBuf>, data_folder: Option<PathBuf>) -> Files {
        Files {
            config_folder,
            data_folder,
        }
    }

    /// The file that holds `entry`, or why there is none.
    pub fn path(&self, entry: Entry<'_>) -> Result<PathBuf, String> {
        let (folder, subfolder, missing) = match entry {
            Entry::Settings => (&self.config_folder, None, "no config folder for us"),
            Entry::BatteryRam(_) | Entry::BatteryClock(_) => (
                &self.data_folder,
                Some("saves"),
                "no data folder to keep saves in",
            ),
            Entry::Cheats(_) => (&self.data_folder, Some("cheats"), "no data folder for us"),
            Entry::LastGame => return Err(String::from("the desktop keeps no last game")),
        };
        let folder = folder
            .as_ref()
            .ok_or_else(|| format!("the system has {missing}"))?;

        Ok(match subfolder {
            Some(subfolder) => folder.join(subfolder).join(entry.file_name()),
            None => folder.join(entry.file_name()),
        })
    }
}

impl Storage for Files {
    fn read(&self, entry: Entry<'_>) -> Result<Option<Vec<u8>>, String> {
        match fs::read(self.path(entry)?) {
            Ok(contents) => Ok(Some(contents)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn write(&self, entry: Entry<'_>, contents: &[u8]) -> Result<(), String> {
        write_atomically(&self.path(entry)?, contents).map_err(|error| error.to_string())
    }

    fn location(&self, entry: Entry<'_>) -> String {
        self.path(entry)
            .unwrap_or_else(|_| entry.file_name())
            .display()
            .to_string()
    }
}

/// Replaces the contents of `path`, creating its folder if needed. It writes
/// a temporary file first and renames it, so a crash halfway never leaves a
/// half-written file.
pub fn write_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut temporary_name = path.as_os_str().to_owned();
    temporary_name.push(".tmp");
    let temporary_path = PathBuf::from(temporary_name);

    path.parent().map_or(Ok(()), fs::create_dir_all)?;
    fs::write(&temporary_path, contents)?;
    fs::rename(&temporary_path, path)
}

/// Encodes a picture of `width` by `height` RGBA pixels, such as a frame of
/// the Game Boy screen, as a PNG image.
pub fn encode_png(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, String> {
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(pixels, width, height, ExtendedColorType::Rgba8)
        .map_err(|error| error.to_string())?;
    Ok(png)
}

/// Decodes a PNG to its width, height and RGBA pixels.
pub fn decode_png(png: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let picture = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .ok()?
        .to_rgba8();
    Some((picture.width(), picture.height(), picture.into_raw()))
}
