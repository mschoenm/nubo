//! Project tasks run with `cargo xtask <command>`: building the Iced
//! frontend for the browser and serving it locally, building the macOS app
//! bundle, the Flatpak, the Windows installer and the RetroArch core, writing the logo and the
//! license files that go with them, checking a release's version and notes,
//! timing Nubo next to other emulators, the nightly review's tooling,
//! reading the GB Emulator Shootout's published table, and making the
//! documentation's screenshots from recorded scenes.
//!
//! The packaging tasks need their platform's tools, and `compare-emulators`
//! builds other emulators from their C and C++ sources; nothing else needs
//! more than Rust.

use clap::{Parser, Subcommand};
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode};
use wasm_bindgen_cli_support::Bindgen;

const WASM_TARGET: &str = "wasm32-unknown-unknown";
/// The profile the published web version is built with: optimized across all
/// crates, which makes it smaller and faster but takes a while.
const WASM_PROFILE: &str = "wasm-release";
/// The profile `--quick` builds the web version with, a fraction of the time
/// of `WASM_PROFILE`; CI uses it to check that the browser build works.
const QUICK_WASM_PROFILE: &str = "release";
const ICED_PACKAGE: &str = "nubo";
const ICED_ARTIFACT: &str = "nubo.wasm";
const WEB_ADDRESS: &str = "127.0.0.1:8080";
/// The app's id: the macOS bundle identifier and the Flatpak's id. Linux
/// desktops match the app's window to its menu entry by it too (the Iced
/// frontend sets the same id on its window), and the menu entry and AppStream
/// files in `LINUX_FILES` are named after it. This matters because GNOME on
/// Wayland ignores window icons and shows the icon of the installed menu
/// entry that matches the window.
const APP_ID: &str = "io.github.mschoenm.nubo";
/// The folder with the Linux menu entry and the AppStream description (what
/// app stores show), which the Flatpak installs.
const LINUX_FILES: &str = "frontends/nubo/linux";

/// The command line of `cargo xtask`. The doc comments on its tasks and
/// their options are the help that `cargo xtask help` prints.
#[derive(Parser)]
#[command(
    name = "xtask",
    bin_name = "cargo xtask",
    about = "Nubo's project tasks",
    long_about = None,
    arg_required_else_help = true
)]
struct Arguments {
    #[command(subcommand)]
    task: Task,
}

/// The tasks `cargo xtask` runs.
#[derive(Subcommand)]
enum Task {
    /// Build the Iced frontend for the browser and serve it at http://127.0.0.1:8080
    Web {
        /// Build much faster, but bigger and slower to run
        #[arg(long)]
        quick: bool,
    },
    /// Build the Iced frontend for the browser into target/web, ready to publish
    BuildWeb {
        /// Build much faster, but bigger and slower to run
        #[arg(long)]
        quick: bool,
    },
    /// Build Nubo.app, the macOS app with its icon, into target/bundle (macOS only)
    Macos,
    /// Build the desktop app as a Flatpak, install it for this user and write it into
    /// target/bundle as a .flatpak file to share (Linux with flatpak-builder only)
    Flatpak,
    /// Build the Windows installer (an MSI, with the WiX Toolset) and a zip to run without
    /// installing into target/bundle (Windows only)
    Windows(windows::Options),
    /// Build the RetroArch core for this computer into target/bundle as a folder and a zip: the
    /// core under RetroArch's file name, its info file and the licenses
    Libretro,
    /// Write the Nubo logo to a file: an SVG if it ends in .svg, a Windows icon if it ends in
    /// .ico, else a 256 pixel PNG
    Icon {
        /// The file to write
        path: PathBuf,
    },
    /// Write Nubo's license files and the licenses of the crates the desktop app is built from
    /// into a folder (every package includes them)
    Licenses {
        /// The folder to write them into
        folder: PathBuf,
    },
    /// Check that the newest release in the AppStream file is the version in Cargo.toml and
    /// print its notes as Markdown
    ReleaseNotes {
        /// The release's tag, such as v1.2.3, which must name that version too
        tag: Option<String>,
    },
    /// Download the free homebrew games the nightly CI benchmarks into target/benchmark-roms
    /// and check their checksums
    BenchmarkRoms,
    /// Measure Nubo's speed next to other emulators on the benchmark games (or the ROMs
    /// given), building them into target/emulator-comparison (Linux and macOS; needs git, a
    /// C/C++ compiler, make, RGBDS and CMake)
    CompareEmulators(compare_emulators::Arguments),
    /// List the parts the nightly review splits the code into, or pick tonight's part and
    /// record it in the rotation's state
    #[command(subcommand)]
    ReviewUnits(review_units::Command),
    /// Add a night to the nightly metrics log, or write the Nightly metrics wiki page
    #[command(subcommand)]
    Metrics(metrics::Command),
    /// Read the GB Emulator Shootout's published table from its gh-pages branch into
    /// tools/nubo-test-rom-runner/shootout_published.txt
    ShootoutScoreboard {
        /// The commit of the gh-pages branch to read, as a full hash
        #[arg(value_parser = shootout_scoreboard::parse_commit)]
        commit: String,
    },
    /// Make scenes for the screenshots: play a game headless with an input script and save
    /// candidates, or keep one in images/scenes
    #[command(subcommand)]
    Scene(scene::Command),
    /// Make the screenshots images/screenshots.toml lists from their scenes, in a headless
    /// GNOME Shell (Linux; prints what to install)
    Screenshots(screenshots::Arguments),
    /// Take the pictures of a plan in headless GNOME Shells, on the D-Bus session bus
    /// `cargo xtask screenshots` starts (see screenshots/gnome.rs)
    #[command(hide = true)]
    GnomeSession {
        /// The plan, as JSON
        plan: PathBuf,
    },
}

/// Every task reports failures as a message for the user.
type Result<T = ()> = std::result::Result<T, String>;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives inside the workspace")
        .to_path_buf()
}

fn target_dir() -> PathBuf {
    env::var_os("CARGO_TARGET_DIR").map_or_else(|| workspace_root().join("target"), PathBuf::from)
}

/// The cargo that runs xtask, so the same toolchain builds everything, with
/// `NUBO_COMMIT` set to the commit it builds ([`build_commit`]), which save
/// states store.
///
/// The core keeps it in `nubo_core::BUILD_COMMIT`, so the states of every
/// release and nightly build say exactly which build made them. Every build
/// xtask runs gets it, the Flatpak's included; a plain `cargo build` leaves
/// it empty.
fn cargo() -> Command {
    let mut command = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.env("NUBO_COMMIT", build_commit());
    command
}

/// The commit of the working tree, followed by `-dirty` when tracked files
/// have changes that aren't committed, or what `NUBO_COMMIT` already says.
/// Without git it is `GITHUB_SHA` in GitHub Actions, and empty elsewhere.
fn build_commit() -> String {
    if let Some(commit) = env::var_os("NUBO_COMMIT") {
        return commit.to_string_lossy().into_owned();
    }
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(workspace_root())
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    match git(&["rev-parse", "HEAD"]) {
        Some(commit) => match git(&["status", "--porcelain", "--untracked-files=no"]) {
            Some(changes) if !changes.is_empty() => format!("{commit}-dirty"),
            _ => commit,
        },
        None => env::var("GITHUB_SHA").unwrap_or_default(),
    }
}

/// Runs a command and turns a failure to start or a non-zero exit into an
/// error.
fn run(command: &mut Command) -> Result {
    let program = command.get_program().to_string_lossy().into_owned();
    let status = command
        .status()
        .map_err(|error| format!("failed to run {program}: {error}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} exited with {status}"))
    }
}

/// Installs the WebAssembly target with rustup if it is missing.
fn ensure_wasm_target() -> Result {
    let output = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .current_dir(workspace_root())
        .output()
        .map_err(|_| {
            format!("rustup was not found; add the {WASM_TARGET} target to your Rust toolchain")
        })?;

    let installed = String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.trim() == WASM_TARGET);

    if installed {
        Ok(())
    } else {
        println!("Installing the {WASM_TARGET} target...");
        run(Command::new("rustup")
            .args(["target", "add", WASM_TARGET])
            .current_dir(workspace_root()))
    }
}

/// Builds `package` for WebAssembly with the cargo profile `profile` and
/// returns the path of the `.wasm` file.
fn build_wasm_package(package: &str, artifact: &str, profile: &str) -> Result<PathBuf> {
    ensure_wasm_target()?;

    run(cargo()
        .args([
            "build",
            "--profile",
            profile,
            "--package",
            package,
            "--target",
            WASM_TARGET,
        ])
        .current_dir(workspace_root()))?;

    Ok(target_dir().join(WASM_TARGET).join(profile).join(artifact))
}

/// Builds the Iced frontend for WebAssembly next to its `index.html` and the
/// license files, and returns the directory to serve. `quick` (`--quick`)
/// builds with `QUICK_WASM_PROFILE`.
///
/// The JavaScript glue is generated with the `wasm-bindgen-cli-support`
/// library, so neither wasm-pack nor Node.js is needed. It only works with
/// the exact `wasm-bindgen` version the app is built with, so both are
/// pinned to the same version in the root `Cargo.toml` and upgraded
/// together. The result in `target/web` is what `deploy.yml` publishes to
/// GitHub Pages.
fn build_iced_web(quick: bool) -> Result<PathBuf> {
    let profile = if quick {
        QUICK_WASM_PROFILE
    } else {
        WASM_PROFILE
    };
    let input = build_wasm_package(ICED_PACKAGE, ICED_ARTIFACT, profile)?;
    let output_dir = target_dir().join("web");
    // Start from an empty folder, so files from older builds aren't published.
    if output_dir.exists() {
        fs::remove_dir_all(&output_dir)
            .map_err(|error| format!("failed to clear {}: {error}", output_dir.display()))?;
    }

    Bindgen::new()
        .input_path(&input)
        .web(true)
        .map_err(|error| error.to_string())?
        .omit_default_module_path(false)
        .remove_name_section(true)
        .remove_producers_section(true)
        .generate(&output_dir)
        .map_err(|error| format!("failed to generate bindings: {error}"))?;

    let index = workspace_root()
        .join("frontends")
        .join("nubo")
        .join("web")
        .join("index.html");
    fs::copy(&index, output_dir.join("index.html"))
        .map_err(|error| format!("failed to copy {}: {}", index.display(), error))?;
    licenses::write_license_files(&output_dir, ICED_PACKAGE, WASM_TARGET)?;

    Ok(output_dir)
}

/// Serves the files in `root` over HTTP until the process is stopped.
fn serve(root: &Path) -> Result {
    let listener = TcpListener::bind(WEB_ADDRESS)
        .map_err(|error| format!("failed to listen on {WEB_ADDRESS}: {error}"))?;

    println!("Serving the Iced frontend at http://{WEB_ADDRESS}");

    for stream in listener.incoming().flatten() {
        if let Err(error) = respond(stream, root) {
            eprintln!("request failed: {error}");
        }
    }

    Ok(())
}

/// Answers one HTTP request with a file from `root`, or 404 if it doesn't
/// exist or the path tries to leave `root` (with `..`, or an absolute path
/// such as `C:/` on Windows, which `Path::join` would put in place of
/// `root`).
fn respond(mut stream: TcpStream, root: &Path) -> std::io::Result<()> {
    let mut request_line = String::new();
    BufReader::new(&stream).read_line(&mut request_line)?;

    let requested = request_line.split_whitespace().nth(1).unwrap_or("/");
    let requested = requested
        .split('?')
        .next()
        .unwrap_or("/")
        .trim_start_matches('/');
    let requested = if requested.is_empty() {
        "index.html"
    } else {
        requested
    };

    let stays_in_root = Path::new(requested)
        .components()
        .all(|component| matches!(component, Component::Normal(_)));
    let file = stays_in_root
        .then(|| root.join(requested))
        .and_then(|path| fs::read(&path).ok().map(|body| (path, body)));

    let (status, content_type, body) = match file {
        Some((path, body)) => ("200 OK", content_type(&path), body),
        None => ("404 Not Found", "text/plain", b"Not found".to_vec()),
    };

    write!(
        stream,
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        status,
        content_type,
        body.len()
    )?;
    stream.write_all(&body)
}

/// The MIME type for the files the Iced web build contains.
fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        Some("wasm") => "application/wasm",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn main() -> ExitCode {
    let result = match Arguments::parse().task {
        Task::Web { quick } => build_iced_web(quick).and_then(|root| serve(&root)),
        Task::BuildWeb { quick } => build_iced_web(quick).map(|root| {
            println!("Built the web version in {}", root.display());
        }),
        Task::Macos => macos::build_macos_app().map(|app| {
            println!(
                "Built {}. Drag it to the Applications folder to install it.",
                app.display()
            );
        }),
        Task::Flatpak => flatpak::build_flatpak().map(|bundle| {
            println!(
                "Installed the Nubo Flatpak for this user: start it from the app grid or with \
                 `flatpak run {APP_ID}`, and `flatpak uninstall --user {APP_ID}` removes it. \
                 Built {} too, which others install by opening it.",
                bundle.display()
            );
        }),
        Task::Windows(options) => {
            windows::build_windows_downloads(&options).map(|(installer, zip)| {
                println!(
                    "Built {}, which installs Nubo for the current user, and {}, which runs \
                     without installing.",
                    installer.display(),
                    zip.display()
                );
            })
        }
        Task::Libretro => libretro::build_libretro_core().map(|zip| {
            println!(
                "Built {}. Copy the core into RetroArch's cores folder and the .info file into \
                 its info folder.",
                zip.display()
            );
        }),
        Task::Icon { path } => icon::write_icon(&path, 256),
        Task::Licenses { folder } => licenses::host_target()
            .and_then(|target| licenses::write_license_files(&folder, ICED_PACKAGE, &target)),
        Task::ReleaseNotes { tag } => {
            release::release_notes(tag.as_deref()).map(|notes| print!("{notes}"))
        }
        Task::BenchmarkRoms => benchmark_roms::ensure_benchmark_roms().map(|folder| {
            println!("The benchmark games are in {}", folder.display());
        }),
        Task::CompareEmulators(arguments) => compare_emulators::run_comparison(arguments),
        Task::ReviewUnits(command) => review_units::run(command),
        Task::Metrics(command) => metrics::run(command),
        Task::ShootoutScoreboard { commit } => shootout_scoreboard::run(&commit),
        Task::Scene(command) => scene::run(command),
        Task::Screenshots(arguments) => screenshots::run_screenshots(&arguments),
        Task::GnomeSession { plan } => screenshots::gnome::run_session(&plan),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{build_commit, workspace_root};
    use std::env;
    use std::process::Command;

    /// Checks the commit git gives. `NUBO_COMMIT` is passed on as it is, and
    /// outside a git checkout, such as a source archive, there is no commit
    /// to read, so the test has nothing to check then.
    #[test]
    fn the_build_commit_is_a_commit_hash() {
        let in_git = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(workspace_root())
            .output()
            .is_ok_and(|output| output.status.success());
        if env::var_os("NUBO_COMMIT").is_some() || !in_git {
            return;
        }
        let commit = build_commit();
        let hash = commit.strip_suffix("-dirty").unwrap_or(&commit);
        assert_eq!(hash.len(), 40, "{commit}");
        assert!(
            hash.chars().all(|digit| digit.is_ascii_hexdigit()),
            "{commit}"
        );
    }
}

mod benchmark_roms;
mod compare_emulators;
mod flatpak;
mod icon;
mod libretro;
mod licenses;
mod macos;
mod metrics;
mod release;
mod review_units;
mod scene;
mod screenshots;
mod shootout_scoreboard;
mod windows;
