//! Builds the desktop app as a Flatpak, installs it for the current user to
//! try it in the sandbox Flathub apps run in, and exports it as a single
//! `.flatpak` file for giving the app to other people: opening the file in a
//! software center installs the app with its menu entry and icon, and
//! uninstalling it removes them again.
//!
//! flatpak-builder builds it from the working tree with the freedesktop
//! runtime and its Rust extension, which it installs from Flathub the first
//! time. Flatpak builds can't download anything, so every crate in
//! `Cargo.lock` is listed as a source that flatpak-builder downloads first,
//! like Flathub's flatpak-cargo-generator does. The manifest, that list, the
//! icon and the license files are written to `target/flatpak/`; the menu entry
//! and the AppStream description come from the repository.

use super::{APP_ID, LINUX_FILES, Result, build_commit, licenses, run, target_dir, workspace_root};
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const APP_NAME: &str = "Nubo";
const EXECUTABLE: &str = "nubo";
/// The version of the freedesktop runtime (the libraries the app runs on
/// inside the sandbox) and of its SDK and Rust extension.
const RUNTIME_VERSION: &str = "26.08";
const FLATHUB_REPO: &str = "https://dl.flathub.org/repo/flathub.flatpakrepo";
const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";
/// The folder inside the build that holds the files written here.
const GENERATED_FILES: &str = "flatpak-files";

/// What the app may reach outside its sandbox.
const FINISH_ARGS: [&str; 11] = [
    // The window, drawn with the graphics card.
    "--share=ipc",
    "--socket=wayland",
    "--socket=fallback-x11",
    "--device=dri",
    // Sound, which goes from ALSA through the sound server.
    "--socket=pulseaudio",
    // Controllers, which gilrs finds by watching /dev/input inside a Flatpak.
    "--device=input",
    // Box art downloads.
    "--share=network",
    // ROM folders anywhere in the home folder or on external drives, which
    // the library reads and watches.
    "--filesystem=home:ro",
    "--filesystem=/media:ro",
    "--filesystem=/run/media:ro",
    // Screenshots.
    "--filesystem=xdg-pictures/Nubo:create",
];

/// Builds the Flatpak in `target/flatpak`, installs it for the current user
/// and writes it to `target/bundle/Nubo-<version>-linux-<architecture>.flatpak`,
/// returning where that file is. Needs Linux with flatpak-builder.
pub fn build_flatpak() -> Result<PathBuf> {
    if !cfg!(target_os = "linux") {
        return Err(String::from("Flatpaks can only be built on Linux"));
    }
    if Command::new("flatpak-builder")
        .arg("--version")
        .output()
        .is_err()
    {
        return Err(String::from(
            "flatpak-builder was not found; install it with your package manager",
        ));
    }

    let work_dir = target_dir().join("flatpak");
    let files_dir = work_dir.join(GENERATED_FILES);
    if files_dir.exists() {
        fs::remove_dir_all(&files_dir).map_err(|error| error.to_string())?;
    }
    fs::create_dir_all(&files_dir).map_err(|error| error.to_string())?;

    let icon = files_dir.join(format!("{APP_ID}.svg"));
    fs::write(&icon, nubo_logo::svg())
        .map_err(|error| format!("failed to write {}: {error}", icon.display()))?;
    licenses::write_license_files(
        &files_dir.join("licenses"),
        EXECUTABLE,
        &licenses::host_target()?,
    )?;
    write_json(
        &work_dir.join("cargo-sources.json"),
        &Value::Array(cargo_sources()?),
    )?;
    let manifest = format!("{APP_ID}.json");
    write_json(&work_dir.join(&manifest), &manifest_contents())?;

    // flatpak-builder installs the runtime for this user, from a Flathub
    // remote of this user's.
    run(Command::new("flatpak").args([
        "remote-add",
        "--user",
        "--if-not-exists",
        "flathub",
        FLATHUB_REPO,
    ]))?;
    run(Command::new("flatpak-builder")
        .args([
            "--user",
            "--install",
            "--install-deps-from=flathub",
            "--force-clean",
            "--repo=repo",
            "--state-dir=state",
            "build",
        ])
        .arg(&manifest)
        .current_dir(&work_dir))?;

    // The file names Flathub as the place to get the runtime from, so
    // installing it on another computer downloads the runtime too.
    let bundle_dir = target_dir().join("bundle");
    fs::create_dir_all(&bundle_dir).map_err(|error| error.to_string())?;
    let bundle = bundle_dir.join(format!(
        "{APP_NAME}-{}-linux-{}.flatpak",
        env!("CARGO_PKG_VERSION"),
        env::consts::ARCH
    ));
    run(Command::new("flatpak")
        .args(["build-bundle", "--runtime-repo", FLATHUB_REPO])
        .arg(work_dir.join("repo"))
        .arg(&bundle)
        .arg(APP_ID))?;

    Ok(bundle)
}

/// The Flatpak manifest: the runtime, the sandbox's permissions and how to
/// build and install the app.
fn manifest_contents() -> Value {
    let linux_files = format!("{LINUX_FILES}/{APP_ID}");
    json!({
        "id": APP_ID,
        "runtime": "org.freedesktop.Platform",
        "runtime-version": RUNTIME_VERSION,
        "sdk": "org.freedesktop.Sdk",
        "sdk-extensions": ["org.freedesktop.Sdk.Extension.rust-stable"],
        "command": EXECUTABLE,
        "finish-args": FINISH_ARGS,
        "modules": [{
            "name": EXECUTABLE,
            "buildsystem": "simple",
            "build-options": {
                "append-path": "/usr/lib/sdk/rust-stable/bin",
                "env": {
                    "CARGO_HOME": format!("/run/build/{EXECUTABLE}/cargo"),
                    "CARGO_NET_OFFLINE": "true",
                    "NUBO_COMMIT": build_commit(),
                },
            },
            "build-commands": [
                format!("cargo build --release --locked --package {EXECUTABLE}"),
                format!("install -D --mode 755 target/release/{EXECUTABLE} /app/bin/{EXECUTABLE}"),
                format!(
                    "install -D --mode 644 {linux_files}.desktop \
                     /app/share/applications/{APP_ID}.desktop"
                ),
                format!(
                    "install -D --mode 644 {linux_files}.metainfo.xml \
                     /app/share/metainfo/{APP_ID}.metainfo.xml"
                ),
                format!(
                    "install -D --mode 644 {GENERATED_FILES}/{APP_ID}.svg \
                     /app/share/icons/hicolor/scalable/apps/{APP_ID}.svg"
                ),
                format!(
                    "install -D --mode 644 --target-directory /app/share/licenses/{APP_ID} \
                     {GENERATED_FILES}/licenses/*"
                ),
            ],
            "sources": [
                {
                    "type": "dir",
                    "path": workspace_root(),
                    "skip": ["target", ".git"],
                },
                {
                    "type": "dir",
                    "path": GENERATED_FILES,
                    "dest": GENERATED_FILES,
                },
                "cargo-sources.json",
            ],
        }],
    })
}

/// The crates in `Cargo.lock` as flatpak-builder sources: each crate's
/// archive from crates.io, unpacked into a vendor folder next to the checksum
/// file cargo expects there, and a cargo configuration that takes crates from
/// that folder instead of crates.io.
fn cargo_sources() -> Result<Vec<Value>> {
    let lock_path = workspace_root().join("Cargo.lock");
    let lock: toml::Table = fs::read_to_string(&lock_path)
        .map_err(|error| format!("failed to read {}: {error}", lock_path.display()))?
        .parse()
        .map_err(|error| format!("failed to parse {}: {error}", lock_path.display()))?;
    let packages = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| String::from("Cargo.lock lists no packages"))?;

    let mut sources = Vec::new();
    for package in packages {
        let field = |key: &str| package.get(key).and_then(toml::Value::as_str);
        let (Some(name), Some(version)) = (field("name"), field("version")) else {
            return Err(String::from(
                "Cargo.lock has a package without a name or version",
            ));
        };
        match field("source") {
            // The workspace's own crates.
            None => continue,
            Some(CRATES_IO) => {}
            Some(other) => {
                return Err(format!(
                    "{name} {version} comes from {other}; only crates.io crates can be listed"
                ));
            }
        }
        let checksum = field("checksum")
            .ok_or_else(|| format!("Cargo.lock has no checksum for {name} {version}"))?;
        let dest = format!("cargo/vendor/{name}-{version}");
        sources.push(json!({
            "type": "archive",
            "archive-type": "tar-gzip",
            "url": format!("https://static.crates.io/crates/{name}/{name}-{version}.crate"),
            "sha256": checksum,
            "dest": dest,
        }));
        sources.push(json!({
            "type": "inline",
            "contents": json!({ "package": checksum, "files": {} }).to_string(),
            "dest": dest,
            "dest-filename": ".cargo-checksum.json",
        }));
    }

    // Relative paths in cargo's configuration start from the folder that
    // holds CARGO_HOME, which is the build folder.
    sources.push(json!({
        "type": "inline",
        "contents": "[source.vendored-sources]\n\
                     directory = \"cargo/vendor\"\n\
                     \n\
                     [source.crates-io]\n\
                     replace-with = \"vendored-sources\"\n",
        "dest": "cargo",
        "dest-filename": "config.toml",
    }));
    Ok(sources)
}

fn write_json(path: &Path, value: &Value) -> Result {
    let contents = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    fs::write(path, contents)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}
