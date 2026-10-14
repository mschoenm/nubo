//! Builds the RetroArch core (`frontends/nubo-libretro`) for this computer:
//! a folder and a zip named `Nubo-libretro-<os>-<arch>` in `target/bundle/`,
//! holding the core under the file name RetroArch looks for
//! (`nubo_libretro.so`, `nubo_libretro.dylib` or `nubo_libretro.dll`, where
//! Cargo writes `libnubo_libretro.so` and `libnubo_libretro.dylib`), its info
//! file and the license files.
//!
//! On macOS the core is universal: built for Apple silicon and for Intel and
//! joined with `lipo`. On Windows it carries its own C runtime, so it needs no
//! Visual C++ redistributable next to RetroArch. The zip is written with the
//! platform's own tool: `zip` on Linux, `ditto` on macOS and Windows' `tar`.

use super::{Result, cargo, licenses, run, target_dir, workspace_root};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const PACKAGE: &str = "nubo-libretro";
/// The library's name, as Cargo and RetroArch spell it without a prefix.
const LIBRARY: &str = "nubo_libretro";
const INFO_FILE: &str = "frontends/nubo-libretro/nubo_libretro.info";
/// The two Macs the universal core runs on.
const MAC_TARGETS: [&str; 2] = ["aarch64-apple-darwin", "x86_64-apple-darwin"];

/// Builds the core in release mode and writes
/// `target/bundle/Nubo-libretro-<os>-<arch>/` and its zip, returning the zip.
pub fn build_libretro_core() -> Result<PathBuf> {
    let (os, arch) = platform()?;
    let name = format!("Nubo-libretro-{os}-{arch}");
    let bundle_dir = target_dir().join("bundle");
    let folder = bundle_dir.join(&name);
    if folder.exists() {
        fs::remove_dir_all(&folder).map_err(|error| error.to_string())?;
    }
    fs::create_dir_all(&folder).map_err(|error| error.to_string())?;

    let core = folder.join(format!("{LIBRARY}{}", env::consts::DLL_SUFFIX));
    if cfg!(target_os = "macos") {
        build_universal_mac_core(&core)?;
    } else {
        let mut build = cargo();
        build.args(["build", "--release", "--locked", "--package", PACKAGE]);
        if cfg!(target_env = "msvc") {
            build.env("RUSTFLAGS", "-C target-feature=+crt-static");
        }
        run(&mut build)?;
        copy(&built_library(&target_dir().join("release")), &core)?;
    }
    copy(
        &workspace_root().join(INFO_FILE),
        &folder.join(format!("{LIBRARY}.info")),
    )?;
    licenses::write_license_files(&folder, PACKAGE, &licenses::host_target()?)?;

    let zip = bundle_dir.join(format!("{name}.zip"));
    if zip.exists() {
        fs::remove_file(&zip).map_err(|error| error.to_string())?;
    }
    write_zip(&bundle_dir, &name, &zip)?;
    Ok(zip)
}

/// The names of this computer's system and processor in the zip's name:
/// `linux-x86_64`, `linux-aarch64`, `windows-x64`, `windows-arm64` or
/// `macos-universal`.
fn platform() -> Result<(&'static str, &'static str)> {
    match (env::consts::OS, env::consts::ARCH) {
        ("linux", arch @ ("x86_64" | "aarch64")) => Ok(("linux", arch)),
        ("windows", "x86_64") => Ok(("windows", "x64")),
        ("windows", "aarch64") => Ok(("windows", "arm64")),
        ("macos", _) => Ok(("macos", "universal")),
        (os, arch) => Err(format!(
            "the RetroArch core isn't packaged for {os} on {arch}"
        )),
    }
}

/// The library Cargo built into `release_dir`.
fn built_library(release_dir: &Path) -> PathBuf {
    release_dir.join(format!(
        "{}{LIBRARY}{}",
        env::consts::DLL_PREFIX,
        env::consts::DLL_SUFFIX
    ))
}

/// Builds the core for both Macs and joins them into `core` with `lipo`.
fn build_universal_mac_core(core: &Path) -> Result {
    run(Command::new("rustup")
        .args(["target", "add"])
        .args(MAC_TARGETS)
        .current_dir(workspace_root()))?;
    let mut lipo = Command::new("lipo");
    lipo.arg("-create").arg("-output").arg(core);
    for target in MAC_TARGETS {
        run(cargo().args([
            "build",
            "--release",
            "--locked",
            "--package",
            PACKAGE,
            "--target",
            target,
        ]))?;
        lipo.arg(built_library(&target_dir().join(target).join("release")));
    }
    run(&mut lipo)
}

fn copy(from: &Path, to: &Path) -> Result {
    fs::copy(from, to)
        .map(|_| ())
        .map_err(|error| format!("failed to copy {}: {error}", from.display()))
}

/// Zips the folder `name` in `parent` into `zip`, so it unpacks into a
/// folder of that name.
fn write_zip(parent: &Path, name: &str, zip: &Path) -> Result {
    if cfg!(target_os = "windows") {
        // Windows' own tar (bsdtar) writes zip files; the GNU tar that Git
        // for Windows puts on the path can't, so this names it in full.
        let system_root = env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        run(
            Command::new(Path::new(&system_root).join("System32").join("tar.exe"))
                .args(["-a", "-c", "-f"])
                .arg(zip)
                .arg("-C")
                .arg(parent)
                .arg(name),
        )
    } else if cfg!(target_os = "macos") {
        run(Command::new("ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(parent.join(name))
            .arg(zip))
    } else {
        run(Command::new("zip")
            .args(["-q", "-r"])
            .arg(zip)
            .arg(name)
            .current_dir(parent))
    }
}
