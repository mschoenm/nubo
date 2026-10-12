//! Builds the Windows downloads: a setup program made with Inno Setup that
//! installs Nubo for the current user (no administrator rights needed), with
//! a Start menu entry, an uninstaller and optionally "Open with Nubo" for
//! ROM files, and a zip of the same files to run without installing. The
//! installer script is `frontends/iced/windows/nubo.iss`.

use super::{Result, cargo, licenses, run, target_dir, workspace_root};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const APP_NAME: &str = "Nubo";
const EXECUTABLE: &str = "nubo";
const INSTALLER_SCRIPT: &str = "frontends/iced/windows/nubo.iss";
/// Where Inno Setup's command line compiler is when it was installed with
/// its own setup program.
const INNO_SETUP_FOLDER: &str = "Inno Setup 6";

/// Builds the app in release mode and writes
/// `target/bundle/Nubo-<version>-windows-<arch>-setup.exe` and
/// `target/bundle/Nubo-<version>-windows-<arch>.zip`, returning both paths.
/// Needs Windows with Inno Setup 6.
pub fn build_windows_downloads() -> Result<(PathBuf, PathBuf)> {
    if !cfg!(target_os = "windows") {
        return Err(String::from(
            "the Windows downloads can only be built on Windows",
        ));
    }
    let (arch, inno_architectures) = match env::consts::ARCH {
        "x86_64" => ("x64", "x64compatible"),
        "aarch64" => ("arm64", "arm64"),
        other => return Err(format!("Windows on {other} isn't supported")),
    };
    let inno_compiler = inno_setup_compiler()?;

    run(cargo().args(["build", "--release", "--package", EXECUTABLE]))?;

    // The files both downloads hold, in a folder named after the app so the
    // zip unpacks into one.
    let staging = target_dir().join("windows").join(APP_NAME);
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|error| error.to_string())?;
    }
    fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
    let binary = target_dir()
        .join("release")
        .join(format!("{EXECUTABLE}.exe"));
    fs::copy(&binary, staging.join(format!("{EXECUTABLE}.exe")))
        .map_err(|error| format!("failed to copy {}: {error}", binary.display()))?;
    licenses::write_license_files(&staging, EXECUTABLE, &licenses::host_target()?)?;

    let icon = target_dir().join("windows").join(format!("{APP_NAME}.ico"));
    fs::write(&icon, nubo_logo::ico())
        .map_err(|error| format!("failed to write {}: {error}", icon.display()))?;

    let bundle_dir = target_dir().join("bundle");
    fs::create_dir_all(&bundle_dir).map_err(|error| error.to_string())?;
    let version = env!("CARGO_PKG_VERSION");
    let base_name = format!("{APP_NAME}-{version}-windows-{arch}");

    let installer_name = format!("{base_name}-setup");
    run(Command::new(&inno_compiler)
        .arg("/Qp")
        .arg(format!("/DAppVersion={version}"))
        .arg(define("SourceDir", &staging))
        .arg(define("IconFile", &icon))
        .arg(define("OutputDir", &bundle_dir))
        .arg(format!("/DOutputBaseFilename={installer_name}"))
        .arg(format!("/DArchitectures={inno_architectures}"))
        .arg(workspace_root().join(INSTALLER_SCRIPT)))?;

    let zip = bundle_dir.join(format!("{base_name}.zip"));
    if zip.exists() {
        fs::remove_file(&zip).map_err(|error| error.to_string())?;
    }
    // Windows' own tar (bsdtar) writes zip files; the GNU tar that Git for
    // Windows puts on the path can't, so this names it in full.
    let system_root = env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    run(
        Command::new(Path::new(&system_root).join("System32").join("tar.exe"))
            .arg("-a")
            .arg("-c")
            .arg("-f")
            .arg(&zip)
            .arg("-C")
            .arg(target_dir().join("windows"))
            .arg(APP_NAME),
    )?;

    Ok((bundle_dir.join(format!("{installer_name}.exe")), zip))
}

/// An Inno Setup `/D` option that defines `name` as `path`.
fn define(name: &str, path: &Path) -> String {
    format!("/D{name}={}", path.display())
}

/// Inno Setup's command line compiler: `ISCC` from the environment when set,
/// else the one its setup program installs, else `iscc` from the path.
fn inno_setup_compiler() -> Result<PathBuf> {
    if let Some(path) = env::var_os("ISCC") {
        return Ok(PathBuf::from(path));
    }
    let installed = ["ProgramFiles(x86)", "ProgramFiles"]
        .into_iter()
        .filter_map(env::var_os)
        .map(|folder| Path::new(&folder).join(INNO_SETUP_FOLDER).join("ISCC.exe"))
        .find(|path| path.exists());
    if let Some(path) = installed {
        return Ok(path);
    }
    if Command::new("iscc").arg("/?").output().is_ok() {
        return Ok(PathBuf::from("iscc"));
    }
    Err(String::from(
        "Inno Setup 6 was not found; install it from https://jrsoftware.org/isinfo.php or set \
         ISCC to its ISCC.exe",
    ))
}
