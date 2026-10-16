//! Builds `Nubo.app`, the macOS app bundle, so the app has its own icon in
//! the Dock and can be put in the Applications folder.

use super::{APP_ID, Result, cargo, icon, licenses, run, target_dir};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const APP_NAME: &str = "Nubo";
const EXECUTABLE: &str = "nubo";

/// The empty border Apple's app icons leave around their shape: an 824 pixel
/// square on a 1024 pixel canvas.
const MACOS_ICON_MARGIN: f32 = 100.0 / 1024.0;

/// The pictures an `.iconset` folder needs: the file name and its size in
/// pixels (the `@2x` versions are for Retina screens).
const ICONSET: [(&str, u32); 10] = [
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
];

/// Builds the app in release mode and puts it in `target/bundle/Nubo.app`
/// with its icon and license files, returning where the bundle is. Needs
/// macOS, for its `iconutil` tool.
pub fn build_macos_app() -> Result<PathBuf> {
    if !cfg!(target_os = "macos") {
        return Err(String::from("app bundles can only be built on macOS"));
    }

    run(cargo().args(["build", "--release", "--package", EXECUTABLE]))?;

    let bundle_dir = target_dir().join("bundle");
    let app = bundle_dir.join(format!("{APP_NAME}.app"));
    // Put the bundle together under a name that isn't an app and rename it
    // at the end: macOS registers a `.app` folder as soon as its Info.plist
    // appears, and would keep showing a bundle caught before its icon as
    // having none.
    let staging = bundle_dir.join(format!("{APP_NAME}.app-staging"));
    let contents = staging.join("Contents");
    for folder in [&app, &staging] {
        if folder.exists() {
            fs::remove_dir_all(folder).map_err(|error| error.to_string())?;
        }
    }
    for folder in ["MacOS", "Resources"] {
        fs::create_dir_all(contents.join(folder)).map_err(|error| error.to_string())?;
    }

    let binary = target_dir().join("release").join(EXECUTABLE);
    fs::copy(&binary, contents.join("MacOS").join(EXECUTABLE))
        .map_err(|error| format!("failed to copy {}: {error}", binary.display()))?;
    fs::write(contents.join("Info.plist"), info_plist()).map_err(|error| error.to_string())?;
    licenses::write_license_files(
        &contents.join("Resources"),
        EXECUTABLE,
        &licenses::host_target()?,
    )?;

    let iconset = bundle_dir.join(format!("{APP_NAME}.iconset"));
    write_iconset(&iconset)?;
    run(Command::new("iconutil")
        .arg("--convert")
        .arg("icns")
        .arg(&iconset)
        .arg("--output")
        .arg(contents.join("Resources").join(format!("{APP_NAME}.icns"))))?;
    fs::remove_dir_all(&iconset).map_err(|error| error.to_string())?;
    fs::rename(&staging, &app).map_err(|error| error.to_string())?;

    // Sign the bundle for this Mac only ("ad hoc"), so macOS treats the
    // copied binary and its resources as one app.
    if let Err(error) = run(Command::new("codesign")
        .args(["--force", "--deep", "--sign", "-"])
        .arg(&app))
    {
        eprintln!("warning: the app bundle isn't signed: {error}");
    }

    Ok(app)
}

/// Writes the pictures of an `.iconset` folder for `iconutil`.
fn write_iconset(iconset: &Path) -> Result {
    fs::create_dir_all(iconset).map_err(|error| error.to_string())?;

    for (file_name, size) in ICONSET {
        icon::write_png(&iconset.join(file_name), size, MACOS_ICON_MARGIN)?;
    }
    Ok(())
}

/// The bundle's description for macOS: its name, program, icon and version.
fn info_plist() -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>{APP_NAME}</string>
    <key>CFBundleDisplayName</key>
    <string>{APP_NAME}</string>
    <key>CFBundleIdentifier</key>
    <string>{APP_ID}</string>
    <key>CFBundleExecutable</key>
    <string>{EXECUTABLE}</string>
    <key>CFBundleIconFile</key>
    <string>{APP_NAME}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleVersion</key>
    <string>{version}</string>
    <key>CFBundleShortVersionString</key>
    <string>{version}</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.games</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
"#
    )
}
