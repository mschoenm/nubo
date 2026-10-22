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
//!
//! `write_flathub_files` writes the same manifest for Flathub into
//! `target/flathub/`, building a commit on GitHub instead of the working tree:
//! the files that go into Nubo's repository on Flathub (`docs/FLATHUB.md`).

use super::{
    APP_ID, LINUX_FILES, Result, build_commit, build_commit_date, licenses, run, target_dir,
    workspace_root,
};
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const APP_NAME: &str = "Nubo";
const EXECUTABLE: &str = "nubo";
/// The version of the freedesktop runtime (the libraries the app runs on
/// inside the sandbox) and of its SDK and Rust extension: the newest on
/// Flathub, which `build_flatpak` checks, since a Flathub build would use the
/// same.
const RUNTIME_VERSION: &str = "26.08";
/// The runtime's id on Flathub.
const RUNTIME: &str = "org.freedesktop.Platform";
const FLATHUB_REPO: &str = "https://dl.flathub.org/repo/flathub.flatpakrepo";
const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";
/// The folder inside the build that holds the files written here.
const GENERATED_FILES: &str = "flatpak-files";
/// Where Flathub's build gets Nubo's source from.
const REPOSITORY_URL: &str = "https://github.com/mschoenm/nubo.git";
/// The target the Flathub build's license files are written for. Flathub
/// builds for x86_64 and ARM from the same files, and the app uses the same
/// crates on both.
const FLATHUB_LICENSE_TARGET: &str = "x86_64-unknown-linux-gnu";

/// What the app may reach outside its sandbox.
const FINISH_ARGS: [&str; 12] = [
    // The window, drawn with the graphics card.
    "--share=ipc",
    "--socket=wayland",
    "--socket=fallback-x11",
    "--device=dri",
    // Sound, which goes from ALSA through the sound server.
    "--socket=pulseaudio",
    // Controllers, which gilrs finds by watching /dev/input inside a Flatpak.
    // Flatpak before 1.15.6 doesn't know the input device and reads every
    // device instead, which is what Flathub's linter asks for.
    "--device=input",
    "--device-if=all:!has-input-device",
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
    let generated_files = write_build_files(&work_dir, &licenses::host_target()?)?;
    let manifest = format!("{APP_ID}.json");
    let source = json!({
        "type": "dir",
        "path": workspace_root(),
        "skip": ["target", ".git"],
    });
    write_json(
        &work_dir.join(&manifest),
        &manifest_contents(source, generated_files, Some(&rust_version()?)),
    )?;

    // flatpak-builder installs the runtime for this user, from a Flathub
    // remote of this user's.
    run(Command::new("flatpak").args([
        "remote-add",
        "--user",
        "--if-not-exists",
        "flathub",
        FLATHUB_REPO,
    ]))?;
    check_runtime_is_newest()?;
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

/// Writes the Flatpak manifest for Flathub into `target/flathub/`, returning
/// that folder: the manifest, the crate list and the files the build
/// installs, all of which go into Nubo's repository on Flathub. The manifest
/// builds the commit checked out here from GitHub, with its release tag when
/// it has one, so the working tree must have no changes and the commit must
/// be pushed.
pub fn write_flathub_files() -> Result<PathBuf> {
    let commit = build_commit();
    if commit.is_empty() || commit.ends_with("-dirty") {
        return Err(String::from(
            "Flathub builds a commit from GitHub: commit and push the changes first",
        ));
    }
    let tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    let tagged = Command::new("git")
        .args(["tag", "--points-at", "HEAD"])
        .current_dir(workspace_root())
        .output()
        .is_ok_and(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|line| line == tag)
        });

    let flathub_dir = target_dir().join("flathub");
    if flathub_dir.exists() {
        fs::remove_dir_all(&flathub_dir).map_err(|error| error.to_string())?;
    }
    let generated_files = write_build_files(&flathub_dir, FLATHUB_LICENSE_TARGET)?;
    let source = git_source(&commit, tagged.then_some(tag.as_str()));
    write_json(
        &flathub_dir.join(format!("{APP_ID}.json")),
        &manifest_contents(source, generated_files, None),
    )?;
    if !tagged {
        println!(
            "warning: {commit} isn't tagged {tag}; submit a release to Flathub, not a commit \
             between releases"
        );
    }
    Ok(flathub_dir)
}

/// The source of a commit on GitHub, with its tag when it has one: what
/// Flathub builds.
fn git_source(commit: &str, tag: Option<&str>) -> Value {
    let mut source = json!({ "type": "git", "url": REPOSITORY_URL });
    if let Some(tag) = tag {
        source["tag"] = json!(tag);
    }
    source["commit"] = json!(commit);
    source
}

/// Writes what the manifest takes besides Nubo's source into `work_dir`: the
/// icon and the license files for `target` in `GENERATED_FILES`, and the
/// crate list in `cargo-sources.json`. Returns the files in
/// `GENERATED_FILES` as flatpak-builder sources, one for each file, since
/// Flathub doesn't take folders as sources.
fn write_build_files(work_dir: &Path, target: &str) -> Result<Vec<Value>> {
    let files_dir = work_dir.join(GENERATED_FILES);
    if files_dir.exists() {
        fs::remove_dir_all(&files_dir).map_err(|error| error.to_string())?;
    }
    fs::create_dir_all(&files_dir).map_err(|error| error.to_string())?;

    let icon = files_dir.join(format!("{APP_ID}.svg"));
    fs::write(&icon, nubo_logo::svg())
        .map_err(|error| format!("failed to write {}: {error}", icon.display()))?;
    licenses::write_license_files(&files_dir.join("licenses"), EXECUTABLE, target)?;
    write_json(
        &work_dir.join("cargo-sources.json"),
        &Value::Array(cargo_sources()?),
    )?;
    let mut files = Vec::new();
    for folder in [
        GENERATED_FILES.to_owned(),
        format!("{GENERATED_FILES}/licenses"),
    ] {
        let mut names: Vec<String> = fs::read_dir(work_dir.join(&folder))
            .map_err(|error| error.to_string())?
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_file())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        files.extend(names.iter().map(|name| file_source(&folder, name)));
    }
    Ok(files)
}

/// The flatpak-builder source of the file `name` in `folder`, both relative
/// to the manifest, which the build finds at the same place.
fn file_source(folder: &str, name: &str) -> Value {
    json!({ "type": "file", "path": format!("{folder}/{name}"), "dest": folder })
}

/// Fails when Flathub has a newer freedesktop runtime than `RUNTIME_VERSION`,
/// so the Flatpak moves to each new runtime as soon as it is out, like an app
/// on Flathub should.
fn check_runtime_is_newest() -> Result {
    let output = Command::new("flatpak")
        .args([
            "remote-ls",
            "--user",
            "--runtime",
            "--columns=application,branch",
            "flathub",
        ])
        .output()
        .map_err(|error| format!("failed to run flatpak remote-ls: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "flatpak remote-ls failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    match newest_runtime(&String::from_utf8_lossy(&output.stdout)) {
        Some(newest) if runtime_release(&newest) > runtime_release(RUNTIME_VERSION) => {
            Err(format!(
                "Flathub has the freedesktop runtime {newest}, newer than the \
                 {RUNTIME_VERSION} the Flatpak is built on: set RUNTIME_VERSION in \
                 xtask/src/flatpak.rs to {newest}"
            ))
        }
        Some(_) => Ok(()),
        None => Err(format!("Flathub lists no {RUNTIME} runtime")),
    }
}

/// The newest release of the freedesktop runtime in `listing`, the
/// applications and branches `flatpak remote-ls` printed.
fn newest_runtime(listing: &str) -> Option<String> {
    listing
        .lines()
        .filter_map(|line| {
            let mut columns = line.split_whitespace();
            (columns.next()? == RUNTIME).then_some(columns.next()?)
        })
        .filter(|branch| runtime_release(branch).is_some())
        .max_by_key(|branch| runtime_release(branch))
        .map(String::from)
}

/// A runtime branch such as `26.08` as numbers that sort by release, or
/// `None` for a branch that isn't a release.
fn runtime_release(branch: &str) -> Option<(u32, u32)> {
    let (year, month) = branch.split_once('.')?;
    Some((year.parse().ok()?, month.parse().ok()?))
}

/// The `rust-version` in the root `Cargo.toml`, such as `1.98`.
fn rust_version() -> Result<String> {
    let path = workspace_root().join("Cargo.toml");
    let manifest: toml::Table = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?
        .parse()
        .map_err(|error| format!("{} is not valid TOML: {error}", path.display()))?;
    manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("rust-version"))
        .and_then(toml::Value::as_str)
        .map(String::from)
        .ok_or_else(|| format!("{} has no workspace rust-version", path.display()))
}

/// The build command that prints the SDK's Rust and fails unless its version
/// is `rust_version` (compared by major and minor version), so
/// `rust-version` follows the Rust that Flathub builds the app with.
fn check_rust_version_command(rust_version: &str) -> String {
    let minor_version: Vec<&str> = rust_version.split('.').take(2).collect();
    let minor_version = minor_version.join(".");
    format!(
        "rustc --version && \
         [ \"$(rustc --version | cut -d ' ' -f 2 | cut -d . -f 1-2)\" = {minor_version} ] || \
         {{ echo 'error: the SDK builds with the Rust above, which Flathub builds the app \
         with too, but rust-version in Cargo.toml is {rust_version}: set it to the Rust \
         above' >&2; exit 1; }}"
    )
}

/// The Flatpak manifest: the runtime, the sandbox's permissions and how to
/// build and install the app from `source` (Nubo's source, as a
/// flatpak-builder source) and `generated_files` (from `write_build_files`)
/// with the SDK's Rust. With `rust_version`, the build
/// fails unless the SDK's Rust is that version; Flathub's build leaves that
/// check out, since a newer Rust in the SDK still builds Nubo.
fn manifest_contents(
    source: Value,
    generated_files: Vec<Value>,
    rust_version: Option<&str>,
) -> Value {
    let linux_files = format!("{LINUX_FILES}/{APP_ID}");
    let mut build_commands: Vec<String> = rust_version
        .map(check_rust_version_command)
        .into_iter()
        .collect();
    build_commands.extend([
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
    ]);
    let mut sources = vec![source];
    sources.extend(generated_files);
    sources.push(json!("cargo-sources.json"));
    json!({
        "id": APP_ID,
        "runtime": RUNTIME,
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
                    "NUBO_COMMIT_DATE": build_commit_date(),
                },
            },
            "build-commands": build_commands,
            "sources": sources,
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

#[cfg(test)]
mod tests;
