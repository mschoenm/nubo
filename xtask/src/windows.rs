//! Builds the Windows downloads: an MSI made with the WiX Toolset that
//! installs Nubo for the current user (no administrator rights needed), with
//! a Start menu entry and optionally "Open with Nubo" for ROM files and a
//! desktop shortcut, and a zip of the same files to run without installing.
//! Windows Installer uninstalls it, so Nubo has no uninstaller of its own to
//! sign. The installer script is `frontends/nubo/windows/nubo.wxs`.

use super::{Result, cargo, licenses, release, run, target_dir, workspace_root};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const APP_NAME: &str = "Nubo";
const EXECUTABLE: &str = "nubo";
const INSTALLER_SCRIPT: &str = "frontends/nubo/windows/nubo.wxs";
/// How to get the WiX Toolset, for the message when it isn't found.
const WIX_INSTALL: &str = "install it with `dotnet tool install --global wix --version 7.0.0` \
                           and accept its license with `wix eula accept wix7`, or set WIX to \
                           its wix.exe";

/// The size of the banner at the top of the installer's pages, in pixels:
/// 370 by 44 dialog units at the usual display scale, as WiX's own dialogs use.
const BANNER_WIDTH: u32 = 493;
const BANNER_HEIGHT: u32 = 58;
/// The logo on the banner's right, and the room around it.
const BANNER_LOGO_SIZE: u32 = 44;
const BANNER_LOGO_MARGIN: u32 = 7;

/// What `cargo xtask windows` was asked for.
#[derive(Debug, Default, PartialEq, Eq)]
struct Options {
    /// The date of a nightly build (`--nightly <YYYY-MM-DD>`), which goes in
    /// the files' names and in the name Windows shows for the installed app.
    nightly: Option<String>,
    /// Another version than `Cargo.toml`'s (`--version <x.y.z>`), to build
    /// older and newer installers that test updates and downgrades.
    version: Option<String>,
}

impl Options {
    fn parse(args: &[String]) -> Result<Options> {
        let mut options = Options::default();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let value = args
                .next()
                .ok_or_else(|| format!("{arg} needs a value"))?
                .clone();
            match arg.as_str() {
                "--nightly" if release::is_date(&value) => options.nightly = Some(value),
                "--nightly" => {
                    return Err(format!("--nightly needs a YYYY-MM-DD date, not {value}"));
                }
                "--version" => options.version = Some(value),
                _ => return Err(format!("unknown option {arg}")),
            }
        }
        Ok(options)
    }
}

/// The names one build of the downloads goes by.
#[derive(Debug, PartialEq, Eq)]
struct Naming {
    /// The version Windows Installer compares, such as `0.7.0`.
    version: String,
    /// The app's name in Windows' list of installed apps: `Nubo`, or
    /// `Nubo (nightly 2026-10-14)`.
    product_name: String,
    /// The version as the installer's pages show it: `0.7.0`, or
    /// `0.7.0 (nightly 2026-10-14)`.
    version_text: String,
    /// The files' names without extension, such as `Nubo-0.7.0-windows-x64`
    /// or `Nubo-nightly-2026-10-14-windows-x64`.
    base_name: String,
}

impl Naming {
    fn new(options: &Options, arch: &str) -> Result<Naming> {
        let version = options
            .version
            .clone()
            .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned());
        if !is_msi_version(&version) {
            return Err(format!(
                "Windows Installer needs a version of three numbers, at most 255.255.65535, \
                 not {version}"
            ));
        }
        Ok(match &options.nightly {
            Some(date) => Naming {
                product_name: format!("{APP_NAME} (nightly {date})"),
                version_text: format!("{version} (nightly {date})"),
                base_name: format!("{APP_NAME}-nightly-{date}-windows-{arch}"),
                version,
            },
            None => Naming {
                product_name: APP_NAME.to_owned(),
                version_text: version.clone(),
                base_name: format!("{APP_NAME}-{version}-windows-{arch}"),
                version,
            },
        })
    }
}

/// Builds the app in release mode and writes
/// `target/bundle/Nubo-<version>-windows-<arch>.msi` and
/// `target/bundle/Nubo-<version>-windows-<arch>.zip` (`Nubo-nightly-<date>-…`
/// with `--nightly <date>`), returning both paths. Needs Windows with the WiX
/// Toolset.
pub fn build_windows_downloads(args: &[String]) -> Result<(PathBuf, PathBuf)> {
    let options = Options::parse(args)?;
    if !cfg!(target_os = "windows") {
        return Err(String::from(
            "the Windows downloads can only be built on Windows",
        ));
    }
    let arch = match env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => return Err(format!("Windows on {other} isn't supported")),
    };
    let naming = Naming::new(&options, arch)?;
    let wix = wix()?;

    run(cargo().args(["build", "--release", "--package", EXECUTABLE]))?;

    // The files both downloads hold, in a folder named after the app so the
    // zip unpacks into one.
    let windows_dir = target_dir().join("windows");
    let staging = windows_dir.join(APP_NAME);
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

    // The installer script finds these next to the staged files.
    let icon = windows_dir.join(format!("{APP_NAME}.ico"));
    fs::write(&icon, nubo_logo::ico())
        .map_err(|error| format!("failed to write {}: {error}", icon.display()))?;
    let banner = windows_dir.join("Banner.bmp");
    fs::write(&banner, banner_bitmap())
        .map_err(|error| format!("failed to write {}: {error}", banner.display()))?;

    let bundle_dir = target_dir().join("bundle");
    fs::create_dir_all(&bundle_dir).map_err(|error| error.to_string())?;
    let installer = bundle_dir.join(format!("{}.msi", naming.base_name));
    run(Command::new(&wix)
        .args(["build", "-nologo", "-arch", arch])
        .arg("-d")
        .arg(format!("Version={}", naming.version))
        .arg("-d")
        .arg(format!("ProductName={}", naming.product_name))
        .arg("-d")
        .arg(format!("VersionText={}", naming.version_text))
        .arg("-bindpath")
        .arg(&staging)
        .arg("-bindpath")
        .arg(&windows_dir)
        .arg("-pdbtype")
        .arg("none")
        .arg("-out")
        .arg(&installer)
        .arg(workspace_root().join(INSTALLER_SCRIPT)))?;

    let zip = bundle_dir.join(format!("{}.zip", naming.base_name));
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
            .arg(&windows_dir)
            .arg(APP_NAME),
    )?;

    Ok((installer, zip))
}

/// The WiX Toolset's command line: `WIX` from the environment when set, else
/// `wix` from the path, where `dotnet tool install --global wix` puts it.
fn wix() -> Result<PathBuf> {
    if let Some(path) = env::var_os("WIX") {
        return Ok(PathBuf::from(path));
    }
    if Command::new("wix").arg("--version").output().is_ok() {
        return Ok(PathBuf::from("wix"));
    }
    Err(format!("the WiX Toolset was not found; {WIX_INSTALL}"))
}

/// Whether `version` is one Windows Installer can compare: three numbers,
/// the first two at most 255 and the third at most 65535. (It would take a
/// fourth, but ignores it when comparing.)
fn is_msi_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    let limits = [255, 255, 65535];
    parts.len() == limits.len()
        && parts.iter().zip(limits).all(|(part, limit)| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && part.parse::<u32>().is_ok_and(|number| number <= limit)
        })
}

/// The banner at the top of the installer's pages as a 24-bit BMP file: white,
/// with the logo on the right, under the page's title.
fn banner_bitmap() -> Vec<u8> {
    let logo = nubo_logo::render(BANNER_LOGO_SIZE, 0.0);
    let logo_left = BANNER_WIDTH - BANNER_LOGO_SIZE - BANNER_LOGO_MARGIN;
    let logo_top = (BANNER_HEIGHT - BANNER_LOGO_SIZE) / 2;

    // Each row is padded to a multiple of 4 bytes.
    let row_size = (BANNER_WIDTH * 3).next_multiple_of(4);
    let pixels_size = row_size * BANNER_HEIGHT;
    let mut bitmap = Vec::with_capacity((54 + pixels_size) as usize);
    // BITMAPFILEHEADER: "BM", the file's size, reserved, where the pixels are.
    bitmap.extend_from_slice(b"BM");
    bitmap.extend_from_slice(&(54 + pixels_size).to_le_bytes());
    bitmap.extend_from_slice(&0_u32.to_le_bytes());
    bitmap.extend_from_slice(&54_u32.to_le_bytes());
    // BITMAPINFOHEADER: its size, width, height (positive: bottom row first),
    // one plane, 24 bits per pixel, no compression, the pixels' size, 96 DPI
    // (3780 pixels per meter), no palette.
    bitmap.extend_from_slice(&40_u32.to_le_bytes());
    bitmap.extend_from_slice(&BANNER_WIDTH.to_le_bytes());
    bitmap.extend_from_slice(&BANNER_HEIGHT.to_le_bytes());
    bitmap.extend_from_slice(&1_u16.to_le_bytes());
    bitmap.extend_from_slice(&24_u16.to_le_bytes());
    bitmap.extend_from_slice(&0_u32.to_le_bytes());
    bitmap.extend_from_slice(&pixels_size.to_le_bytes());
    for _ in 0..2 {
        bitmap.extend_from_slice(&3780_u32.to_le_bytes());
    }
    bitmap.extend_from_slice(&[0; 8]);

    for row in (0..BANNER_HEIGHT).rev() {
        let row_start = bitmap.len();
        for column in 0..BANNER_WIDTH {
            let mut color = [255_u8; 3];
            let in_logo = (logo_left..logo_left + BANNER_LOGO_SIZE).contains(&column)
                && (logo_top..logo_top + BANNER_LOGO_SIZE).contains(&row);
            if in_logo {
                let index =
                    (((row - logo_top) * BANNER_LOGO_SIZE + (column - logo_left)) * 4) as usize;
                let alpha = u32::from(logo[index + 3]);
                for (channel, value) in color.iter_mut().enumerate() {
                    // The logo's color over white, by its coverage.
                    let blended =
                        (u32::from(logo[index + channel]) * alpha + 255 * (255 - alpha)) / 255;
                    *value = u8::try_from(blended).unwrap_or(u8::MAX);
                }
            }
            let [red, green, blue] = color;
            bitmap.extend_from_slice(&[blue, green, red]);
        }
        bitmap.resize(row_start + row_size as usize, 0);
    }
    bitmap
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn a_release_is_named_after_its_version() {
        let naming = Naming::new(&Options::parse(&[]).unwrap(), "x64").unwrap();
        let version = env!("CARGO_PKG_VERSION");
        assert_eq!(
            naming,
            Naming {
                version: version.to_owned(),
                product_name: "Nubo".to_owned(),
                version_text: version.to_owned(),
                base_name: format!("Nubo-{version}-windows-x64"),
            }
        );
    }

    #[test]
    fn a_nightly_build_carries_its_date_in_its_names() {
        let options = Options::parse(&args("--nightly 2026-10-14 --version 1.2.3")).unwrap();
        let naming = Naming::new(&options, "arm64").unwrap();
        assert_eq!(
            naming,
            Naming {
                version: "1.2.3".to_owned(),
                product_name: "Nubo (nightly 2026-10-14)".to_owned(),
                version_text: "1.2.3 (nightly 2026-10-14)".to_owned(),
                base_name: "Nubo-nightly-2026-10-14-windows-arm64".to_owned(),
            }
        );
    }

    #[test]
    fn wrong_options_are_refused() {
        assert!(Options::parse(&args("--nightly 14-10-2026")).is_err());
        assert!(Options::parse(&args("--nightly")).is_err());
        assert!(Options::parse(&args("--quick")).is_err());
        let options = Options::parse(&args("--version 1.0.0-beta")).unwrap();
        assert!(Naming::new(&options, "x64").is_err());
    }

    #[test]
    fn only_versions_windows_installer_can_compare_are_accepted() {
        for good in ["0.7.0", "255.255.65535", "1.0.10"] {
            assert!(is_msi_version(good), "{good}");
        }
        for bad in [
            "0.7",
            "0.7.0.1",
            "256.0.0",
            "0.0.65536",
            "1.0.0-beta",
            "1..0",
            "+1.0.0",
        ] {
            assert!(!is_msi_version(bad), "{bad}");
        }
    }

    #[test]
    fn the_banner_is_a_white_bitmap_with_the_logo_on_the_right() {
        let bitmap = banner_bitmap();
        let row_size = (BANNER_WIDTH * 3).next_multiple_of(4) as usize;
        assert_eq!(&bitmap[..2], b"BM");
        assert_eq!(bitmap.len(), 54 + row_size * BANNER_HEIGHT as usize);
        let pixel = |column: u32, row_from_top: u32| {
            let start =
                54 + row_size * (BANNER_HEIGHT - 1 - row_from_top) as usize + column as usize * 3;
            [bitmap[start], bitmap[start + 1], bitmap[start + 2]]
        };
        let logo_left = BANNER_WIDTH - BANNER_LOGO_SIZE - BANNER_LOGO_MARGIN;
        let logo_top = (BANNER_HEIGHT - BANNER_LOGO_SIZE) / 2;
        let mut colored = 0;
        for row in 0..BANNER_HEIGHT {
            for column in 0..BANNER_WIDTH {
                let in_logo = (logo_left..logo_left + BANNER_LOGO_SIZE).contains(&column)
                    && (logo_top..logo_top + BANNER_LOGO_SIZE).contains(&row);
                if pixel(column, row) != [255; 3] {
                    assert!(in_logo, "colored outside the logo at {column}, {row}");
                    colored += 1;
                }
            }
        }
        assert!(colored > (BANNER_LOGO_SIZE * BANNER_LOGO_SIZE / 4) as usize);
    }
}
