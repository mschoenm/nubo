//! Files in the per-user folders of the desktop build.

use crate::app::{SCREEN_HEIGHT, SCREEN_WIDTH};
use directories::ProjectDirs;
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Nubo's per-user folders, or `None` when the operating system has none
/// for us, for example because the home directory is unknown.
pub fn project_directories() -> Option<ProjectDirs> {
    ProjectDirs::from("", "", "Nubo")
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

/// Encodes a frame of the Game Boy screen (RGBA pixels) as a PNG image.
pub fn encode_png(pixels: &[u8]) -> Result<Vec<u8>, String> {
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(
            pixels,
            SCREEN_WIDTH,
            SCREEN_HEIGHT,
            ExtendedColorType::Rgba8,
        )
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
