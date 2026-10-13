//! Project tasks run with `cargo xtask <command>`: building the Iced
//! frontend for the browser and serving it locally, building the macOS app
//! bundle, the Flatpak and the Windows installer, writing the license files
//! that go with them, and checking a release's version and notes.

use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode};
use wasm_bindgen_cli_support::Bindgen;

const WASM_TARGET: &str = "wasm32-unknown-unknown";
const WASM_PROFILE: &str = "wasm-release";
const ICED_PACKAGE: &str = "nubo";
const ICED_ARTIFACT: &str = "nubo.wasm";
const WEB_ADDRESS: &str = "127.0.0.1:8080";
/// The app's id: the macOS bundle identifier and the Flatpak's id. Linux
/// desktops match the app's window to its menu entry by it too (the Iced
/// frontend sets the same id on its window).
const APP_ID: &str = "io.github.mschoenm.nubo";
/// The folder with the Linux menu entry and the AppStream description (what
/// app stores show), which the Flatpak installs.
const LINUX_FILES: &str = "frontends/iced/linux";

const USAGE: &str = "\
Usage: cargo xtask <command>

Commands:
  web           Build the Iced frontend for the browser and serve it at http://127.0.0.1:8080
  build-web     Build the Iced frontend for the browser into target/web, ready to publish
  bundle        Build Nubo.app, the macOS app with its icon, into target/bundle (macOS only)
  flatpak       Build the desktop app as a Flatpak, install it for this user and write it into
                target/bundle as a .flatpak file to share (Linux with flatpak-builder only)
  windows       Build the Windows installer (with Inno Setup 6) and a zip to run without
                installing into target/bundle (Windows only)
  icon <path>   Write the Nubo logo to <path>: an SVG if it ends in .svg, a Windows icon if it
                ends in .ico, else a 256 pixel PNG
  licenses <folder>
                Write Nubo's license files and the licenses of the crates the desktop app is built
                from into <folder> (the app bundle, the Flatpak and the web version include them)
  release-notes [tag]
                Check that the newest release in the AppStream file is the version in Cargo.toml
                (and that the tag, such as v1.2.3, names it) and print its notes as Markdown
  benchmark-roms
                Download the free homebrew games the nightly CI benchmarks into
                target/benchmark-roms and check their checksums
  review-units [next --state <rotation.json> --date <YYYY-MM-DD> --commit <sha>]
                List the parts the nightly review splits the code into, or pick tonight's part
                and record it in the rotation's state
  metrics append <metrics.jsonl> <night.json> | metrics report <metrics.jsonl>
                Add a night to the nightly metrics log, or write the Nightly metrics wiki page
  help          Show this message";

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

/// Builds `package` for WebAssembly and returns the path of the `.wasm` file.
fn build_wasm_package(package: &str, artifact: &str) -> Result<PathBuf> {
    ensure_wasm_target()?;

    run(cargo()
        .args([
            "build",
            "--profile",
            WASM_PROFILE,
            "--package",
            package,
            "--target",
            WASM_TARGET,
        ])
        .current_dir(workspace_root()))?;

    Ok(target_dir()
        .join(WASM_TARGET)
        .join(WASM_PROFILE)
        .join(artifact))
}

/// Builds the Iced frontend for WebAssembly next to its `index.html` and the
/// license files, and returns the directory to serve.
fn build_iced_web() -> Result<PathBuf> {
    let input = build_wasm_package(ICED_PACKAGE, ICED_ARTIFACT)?;
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
        .join("iced")
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
    let command = env::args().nth(1);

    let result = match command.as_deref() {
        Some("web") => build_iced_web().and_then(|root| serve(&root)),
        Some("build-web") => build_iced_web().map(|root| {
            println!("Built the web version in {}", root.display());
        }),
        Some("bundle") => bundle::build_app_bundle().map(|app| {
            println!(
                "Built {}. Drag it to the Applications folder to install it.",
                app.display()
            );
        }),
        Some("flatpak") => flatpak::build_flatpak().map(|bundle| {
            println!(
                "Installed the Nubo Flatpak for this user: start it from the app grid or with \
                 `flatpak run {APP_ID}`, and `flatpak uninstall --user {APP_ID}` removes it. \
                 Built {} too, which others install by opening it.",
                bundle.display()
            );
        }),
        Some("windows") => windows::build_windows_downloads().map(|(installer, zip)| {
            println!(
                "Built {}, which installs Nubo for the current user, and {}, which runs without \
                 installing.",
                installer.display(),
                zip.display()
            );
        }),
        Some("icon") => match env::args().nth(2) {
            Some(path) => bundle::write_icon(Path::new(&path), 256),
            None => Err(format!("icon needs a path to write to\n\n{USAGE}")),
        },
        Some("licenses") => match env::args().nth(2) {
            Some(folder) => licenses::host_target().and_then(|target| {
                licenses::write_license_files(Path::new(&folder), ICED_PACKAGE, &target)
            }),
            None => Err(format!("licenses needs a folder to write to\n\n{USAGE}")),
        },
        Some("release-notes") => {
            release::release_notes(env::args().nth(2).as_deref()).map(|notes| print!("{notes}"))
        }
        Some("benchmark-roms") => benchmark_roms::ensure_benchmark_roms().map(|folder| {
            println!("The benchmark games are in {}", folder.display());
        }),
        Some("review-units") => review_units::run(&env::args().skip(2).collect::<Vec<_>>()),
        Some("metrics") => metrics::run(&env::args().skip(2).collect::<Vec<_>>()),
        Some("help") | None => {
            println!("{USAGE}");
            Ok(())
        }
        Some(unknown) => Err(format!("unknown command '{unknown}'\n\n{USAGE}")),
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
    use super::build_commit;

    #[test]
    fn the_build_commit_is_a_commit_hash() {
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
mod bundle;
mod flatpak;
mod licenses;
mod metrics;
mod release;
mod review_units;
mod windows;
