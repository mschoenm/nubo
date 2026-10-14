//! Writes the Nubo logo as an SVG, a Windows icon or a PNG, for
//! `cargo xtask icon` and the macOS app's icon.

use super::Result;
use image::ImageFormat;
use std::fs;
use std::path::Path;

/// Writes the logo to `path` without a border: as an SVG when the path ends
/// in `.svg`, as a Windows icon with every size when it ends in `.ico`, and
/// otherwise as a PNG `size` pixels wide.
pub fn write_icon(path: &Path, size: u32) -> Result {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    let contents = match extension.as_deref() {
        Some("svg") => nubo_logo::svg().into_bytes(),
        Some("ico") => nubo_logo::ico(),
        _ => return write_png(path, size, 0.0),
    };
    fs::write(path, contents)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

/// Writes the logo as a PNG `size` pixels wide, leaving `margin` (a fraction
/// of the size) empty around it.
pub fn write_png(path: &Path, size: u32, margin: f32) -> Result {
    image::save_buffer_with_format(
        path,
        &nubo_logo::render(size, margin),
        size,
        size,
        image::ExtendedColorType::Rgba8,
        ImageFormat::Png,
    )
    .map_err(|error| format!("failed to write {}: {error}", path.display()))
}
