//! Pictures for `cargo xtask screenshots`: reading and writing PNG files,
//! putting the pictures of a screenshot's variants side by side, and
//! marking where two pictures differ.

use super::lists::{Layout, Screenshot};
use crate::Result;
use image::RgbaImage;
use std::path::{Path, PathBuf};

/// Reads the PNG file at `path` as RGBA pixels.
pub fn read(path: &Path) -> Result<RgbaImage> {
    Ok(image::open(path)
        .map_err(|error| format!("couldn't read {}: {error}", path.display()))?
        .into_rgba8())
}

/// Writes `picture` to `path` as a PNG file, creating its folder.
pub fn write(path: &Path, picture: &RgbaImage) -> Result {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)
            .map_err(|error| format!("couldn't create {}: {error}", folder.display()))?;
    }
    picture
        .save(path)
        .map_err(|error| format!("couldn't write {}: {error}", path.display()))
}

/// The picture of `screenshot` from the pictures the app took for it, one
/// per tile: that picture, or for variants all of them laid out as the
/// screenshot says, each centered in a cell as big as the biggest.
pub fn compose(screenshot: &Screenshot, tiles: &[PathBuf]) -> Result<RgbaImage> {
    let pictures = tiles
        .iter()
        .map(|path| read(path))
        .collect::<Result<Vec<_>>>()?;
    if screenshot.variants.is_empty()
        && let [picture] = pictures.as_slice()
    {
        return Ok(picture.clone());
    }

    let count = u32::try_from(pictures.len()).map_err(|error| error.to_string())?;
    let columns = match screenshot.layout {
        Layout::Row => count,
        Layout::Column => 1,
        Layout::Grid => screenshot.columns.unwrap_or(2).max(1),
    };
    let rows = count.div_ceil(columns);
    let cell_width = pictures.iter().map(RgbaImage::width).max().unwrap_or(0);
    let cell_height = pictures.iter().map(RgbaImage::height).max().unwrap_or(0);
    let (gap, padding) = (screenshot.gap, screenshot.padding);
    let width = 2 * padding + columns * cell_width + (columns - 1) * gap;
    let height = 2 * padding + rows * cell_height + (rows - 1) * gap;

    let mut canvas = RgbaImage::from_pixel(width, height, image::Rgba(screenshot.background()));
    for (index, picture) in (0..).zip(&pictures) {
        let (column, row) = (index % columns, index / columns);
        let x = padding + column * (cell_width + gap) + (cell_width - picture.width()) / 2;
        let y = padding + row * (cell_height + gap) + (cell_height - picture.height()) / 2;
        image::imageops::overlay(&mut canvas, picture, i64::from(x), i64::from(y));
    }
    Ok(canvas)
}

/// `new` with every pixel that differs from `old` painted red, and the
/// others faded, so the differences stand out. Pictures of different sizes
/// differ everywhere outside the part they share.
pub fn difference(old: &RgbaImage, new: &RgbaImage) -> RgbaImage {
    RgbaImage::from_fn(new.width(), new.height(), |x, y| {
        let pixel = *new.get_pixel(x, y);
        let same = x < old.width() && y < old.height() && *old.get_pixel(x, y) == pixel;
        if same {
            let [red, green, blue, alpha] = pixel.0;
            let fade = |channel: u8| 128 + channel / 2;
            image::Rgba([fade(red), fade(green), fade(blue), alpha])
        } else {
            image::Rgba([255, 0, 0, 255])
        }
    })
}
