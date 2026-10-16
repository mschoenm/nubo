//! Builds Nubo for this computer and installs it here, replacing the Nubo
//! that is installed: the app in `/Applications` on macOS, the Flatpak for
//! this user on Linux and the MSI for this user on Windows, all without
//! questions or windows. A running Nubo is closed first, the way closing its
//! window does, so it saves where the player left off.
//!
//! What gets built: the working tree as it is when it is the newest
//! `origin/main` without changes, and otherwise what the player picks: the
//! working tree anyway, or the newest `origin/main`. `--build` picks without
//! asking, and also takes a version, a tag, a branch or a commit. Anything
//! but the working tree is built in a git worktree of its own under
//! `target/install/`, so the working tree is never touched, by that
//! version's own `cargo xtask install`, which is why only versions that have
//! this task can be installed this way.

use super::{Result, build_commit, flatpak, macos, run, target_dir, windows, workspace_root};
use clap::Args;
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

/// The branch "the newest version" means.
const MAIN: &str = "origin/main";
/// A version whose tree has this file has this task, and can be installed
/// from a worktree.
const INSTALL_TASK_FILE: &str = "xtask/src/install.rs";
/// How long a running Nubo gets to save and close before it is stopped by
/// force.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);
/// Where the app is installed on macOS.
const MACOS_APP: &str = "/Applications/Nubo.app";
/// The installed app's program on macOS, as `ps` names its process.
const MACOS_EXECUTABLE: &str = "/Applications/Nubo.app/Contents/MacOS/nubo";
/// The Flatpak's program as Linux names the executable of its process (the
/// path inside the sandbox), which no other program has.
const FLATPAK_EXECUTABLE: &str = "/app/bin/nubo";
/// Closes the installed Nubo on Windows, as clicking its close button does,
/// and stops it by force if it is still running after `CLOSE_TIMEOUT`.
/// Prints how many were running, then `forced` for each one stopped by force.
/// Any error ends it with a failure.
const WINDOWS_CLOSE_SCRIPT: &str = r"
$ErrorActionPreference = 'Stop'
$path = Join-Path $env:LOCALAPPDATA 'Programs\Nubo\nubo.exe'
$running = @(Get-Process -Name nubo -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $path })
Write-Output $running.Count
foreach ($process in $running) { [void]$process.CloseMainWindow() }
foreach ($process in $running) {
    if (-not $process.WaitForExit(TIMEOUT)) {
        Stop-Process -Id $process.Id -Force
        Write-Output 'forced'
    }
}
";

/// What `cargo xtask install` was asked for. The doc comments on its fields
/// are the help that `cargo xtask install --help` prints.
#[derive(Args, Debug, Default)]
pub struct Options {
    /// What to build: `checkout` (the working tree as it is, uncommitted changes included),
    /// `main` (the newest origin/main), a version (`0.12` is the newest 0.12.x release,
    /// `0.12.1` that release) or any tag, branch or commit. Without it, the working tree is
    /// built when it is the newest origin/main without changes, and otherwise it asks
    #[arg(long, value_name = "WHAT")]
    build: Option<String>,
}

/// What gets built and installed.
enum Source {
    /// The working tree, as it is.
    Checkout,
    /// Another commit, built in a worktree, and the name it was asked for by.
    Commit { commit: String, name: String },
}

/// Builds what `options` asks for and installs it on this computer.
pub fn install(options: &Options) -> Result {
    if !cfg!(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows"
    )) {
        return Err(String::from(
            "Nubo can only be installed on macOS, Linux and Windows",
        ));
    }
    match choose_source(options)? {
        None => {
            println!("Nothing was installed.");
            Ok(())
        }
        Some(Source::Checkout) => install_checkout(),
        Some(Source::Commit { commit, name }) => install_from_worktree(&commit, &name),
    }
}

/// Works out what to build: fetches from GitHub first, so `origin/main` is
/// the newest, unless the working tree was asked for. `None` when the
/// player cancels.
fn choose_source(options: &Options) -> Result<Option<Source>> {
    if options.build.as_deref() == Some("checkout") {
        return Ok(Some(Source::Checkout));
    }
    println!("Fetching from GitHub...");
    let fetched = git(&["fetch", "--quiet", "--tags", "--force", "origin"]);
    if let Err(error) = &fetched {
        eprintln!("warning: couldn't fetch from GitHub, so {MAIN} may not be the newest: {error}");
    }

    let what = match options.build.as_deref() {
        Some(what) => what.to_owned(),
        None if fetched.is_ok() && head_is(&git(&["rev-parse", MAIN])?)? => {
            return Ok(Some(Source::Checkout));
        }
        None => match ask()? {
            Some(Choice::Checkout) => return Ok(Some(Source::Checkout)),
            Some(Choice::Main) => String::from("main"),
            None => return Ok(None),
        },
    };

    let (reference, name) = resolve(&what)?;
    let commit = git(&["rev-parse", "--verify", &format!("{reference}^{{commit}}")])
        .map_err(|_| format!("there is no tag, branch or commit called {what}"))?;
    if head_is(&commit)? {
        Ok(Some(Source::Checkout))
    } else {
        Ok(Some(Source::Commit { commit, name }))
    }
}

/// Whether the working tree is `commit` without changes to tracked files.
fn head_is(commit: &str) -> Result<bool> {
    Ok(git(&["rev-parse", "HEAD"])? == commit
        && git(&["status", "--porcelain", "--untracked-files=no"])?.is_empty())
}

/// What the player picked when asked.
enum Choice {
    Checkout,
    Main,
}

/// Asks whether to build the working tree or the newest `origin/main`, or
/// fails with the options to pass when there is no terminal to ask in (such
/// as when an agent runs this). `None` when the player cancels.
fn ask() -> Result<Option<Choice>> {
    if !io::stdin().is_terminal() {
        return Err(format!(
            "the working tree isn't the newest {MAIN} without changes. Run again with \
             `--build checkout` to build the working tree as it is, or `--build main` to build \
             the newest {MAIN}"
        ));
    }
    let branch = git(&["branch", "--show-current"])?;
    let local = if branch.is_empty() {
        String::from("the local copy")
    } else {
        format!("the local copy ({branch})")
    };
    println!("What should be built?");
    println!("  1  {local}, as it is");
    println!("  2  the newest {MAIN}");
    print!("Build 1 or 2 (anything else cancels): ");
    io::stdout().flush().map_err(|error| error.to_string())?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|error| error.to_string())?;
    Ok(match answer.trim() {
        "1" => Some(Choice::Checkout),
        "2" => Some(Choice::Main),
        _ => None,
    })
}

/// The git reference `what` names, and the name to show for it: `main` is
/// `origin/main`, a version is its release's tag, and anything else is
/// passed on to git as it is.
fn resolve(what: &str) -> Result<(String, String)> {
    if what == "main" {
        return Ok((MAIN.to_owned(), MAIN.to_owned()));
    }
    match parse_version(what) {
        Some(version) => {
            let tags = git(&["tag", "--list", "v*"])?;
            let tag = newest_release(tags.lines(), &version)
                .ok_or_else(|| format!("there is no release {what}"))?;
            Ok((format!("refs/tags/{tag}"), tag))
        }
        None => Ok((what.to_owned(), what.to_owned())),
    }
}

/// The numbers of a version such as `0.12`, `0.12.1` or `v0.12.1`, or `None`
/// when `text` isn't one.
fn parse_version(text: &str) -> Option<Vec<u32>> {
    let numbers = text.strip_prefix('v').unwrap_or(text);
    let parts = numbers
        .split('.')
        .map(|part| {
            Some(part)
                .filter(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
                .and_then(|part| part.parse().ok())
        })
        .collect::<Option<Vec<u32>>>()?;
    (1..=3).contains(&parts.len()).then_some(parts)
}

/// The newest release tag (`vX.Y.Z`) among `tags` whose version starts with
/// `version`'s numbers: `0.12` picks the newest `0.12.x`.
fn newest_release<'a>(tags: impl Iterator<Item = &'a str>, version: &[u32]) -> Option<String> {
    tags.filter_map(|tag| {
        let numbers = parse_version(tag).filter(|numbers| numbers.len() == 3)?;
        numbers
            .starts_with(version)
            .then(|| (numbers, tag.to_owned()))
    })
    .max()
    .map(|(_, tag)| tag)
}

/// Builds the working tree's package for this system, closes a running Nubo
/// and installs the package.
fn install_checkout() -> Result {
    if cfg!(target_os = "macos") {
        let app = macos::build_macos_app()?;
        close_running_nubo()?;
        replace_macos_app(&app)?;
    } else if cfg!(target_os = "windows") {
        let (installer, _) = windows::build_windows_downloads(&windows::Options::default())?;
        close_running_nubo()?;
        run(Command::new("msiexec")
            .arg("/i")
            .arg(&installer)
            .args(["/qn", "/norestart"]))?;
    } else {
        // flatpak-builder installs as it builds. A running Nubo keeps the
        // version it started with until it closes, so it is closed after.
        flatpak::build_flatpak()?;
        close_running_nubo()?;
    }
    println!(
        "Installed Nubo {} ({}).",
        env!("CARGO_PKG_VERSION"),
        build_commit()
    );
    Ok(())
}

/// Builds and installs `commit` (asked for as `name`) in a worktree under
/// `target/install/`, with that version's own `cargo xtask install`. Its
/// build folder is kept there too, so later installs build faster; it isn't
/// the working tree's, since the running xtask can't be replaced on Windows.
fn install_from_worktree(commit: &str, name: &str) -> Result {
    if git(&["cat-file", "-e", &format!("{commit}:{INSTALL_TASK_FILE}")]).is_err() {
        return Err(format!(
            "{name} is from before `cargo xtask install` existed, so it can't be installed \
             this way"
        ));
    }
    // Absolute, since git runs in the workspace root and cargo in the
    // worktree, while a relative `CARGO_TARGET_DIR` is relative to here.
    let folder = std::path::absolute(target_dir().join("install"))
        .map_err(|error| format!("failed to find the target folder: {error}"))?;
    let worktree = folder.join("worktree");
    remove_worktree(&worktree);
    git(&[
        "worktree",
        "add",
        "--detach",
        &worktree.to_string_lossy(),
        commit,
    ])?;

    println!("Building {name} in {}...", worktree.display());
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let installed = run(Command::new(cargo)
        .args(["xtask", "install", "--build", "checkout"])
        .current_dir(&worktree)
        .env("CARGO_TARGET_DIR", folder.join("target"))
        .env_remove("NUBO_COMMIT"));
    remove_worktree(&worktree);
    installed
}

/// Removes the worktree at `path` if there is one, files and all.
fn remove_worktree(path: &Path) {
    let _ = git(&["worktree", "remove", "--force", &path.to_string_lossy()]);
    if path.exists() {
        let _ = fs::remove_dir_all(path);
    }
    let _ = git(&["worktree", "prune"]);
}

/// Closes the installed Nubo if it is running, the way closing its window
/// does, so it saves where the player left off, and stops it by force if it
/// hasn't closed after `CLOSE_TIMEOUT`. On macOS and Linux it is sent
/// SIGTERM, which Nubo handles like its close button (versions before that
/// just end).
fn close_running_nubo() -> Result {
    if cfg!(target_os = "windows") {
        return close_on_windows();
    }
    let running = running_nubo()?;
    if running.is_empty() {
        return Ok(());
    }
    println!("Closing the running Nubo...");
    for process in &running {
        let output = Command::new("kill")
            .args(["-TERM", process])
            .output()
            .map_err(|error| format!("failed to run kill: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "couldn't ask Nubo (process {process}) to close: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
    }
    let deadline = Instant::now() + CLOSE_TIMEOUT;
    while running.iter().any(|process| is_running(process)) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
    }
    for process in running.iter().filter(|process| is_running(process)) {
        eprintln!(
            "warning: Nubo didn't close within {} seconds, so it was stopped",
            CLOSE_TIMEOUT.as_secs()
        );
        let _ = Command::new("kill").args(["-KILL", process]).status();
    }
    Ok(())
}

/// The process ids of the installed Nubo this user runs on macOS or Linux:
/// the app in `/Applications`, or the Flatpak. A Nubo started from a build
/// folder isn't one, and neither is another user's, which this user can't
/// close (Linux doesn't show this user their programs' paths either).
fn running_nubo() -> Result<Vec<String>> {
    if cfg!(target_os = "macos") {
        let output = Command::new("ps")
            .args(["-xo", "pid=,comm="])
            .output()
            .map_err(|error| format!("failed to run ps: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "couldn't list the running programs: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.trim().split_once(' '))
            .filter(|(_, program)| program.trim() == MACOS_EXECUTABLE)
            .map(|(process, _)| process.to_owned())
            .collect())
    } else {
        let processes = fs::read_dir("/proc").map_err(|error| error.to_string())?;
        Ok(processes
            .flatten()
            .filter(|entry| {
                fs::read_link(entry.path().join("exe"))
                    .is_ok_and(|program| program == Path::new(FLATPAK_EXECUTABLE))
            })
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect())
    }
}

/// Whether the process `id` is still running.
fn is_running(id: &str) -> bool {
    Command::new("kill")
        .args(["-0", id])
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Closes the installed Nubo on Windows with `WINDOWS_CLOSE_SCRIPT`.
fn close_on_windows() -> Result {
    let script = WINDOWS_CLOSE_SCRIPT.replace("TIMEOUT", &CLOSE_TIMEOUT.as_millis().to_string());
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|error| format!("failed to run PowerShell: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "couldn't close the running Nubo: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let lines = String::from_utf8_lossy(&output.stdout);
    let mut lines = lines.lines().map(str::trim);
    if lines.next().is_some_and(|count| count != "0") {
        println!("Closed the running Nubo.");
    }
    if lines.any(|line| line == "forced") {
        eprintln!(
            "warning: Nubo didn't close within {} seconds, so it was stopped",
            CLOSE_TIMEOUT.as_secs()
        );
    }
    Ok(())
}

/// Puts `app` in place of the installed app in `/Applications`. It is
/// copied next to it under a name that isn't an app first, so macOS never
/// sees half an app.
fn replace_macos_app(app: &Path) -> Result {
    let installed = Path::new(MACOS_APP);
    let incoming = PathBuf::from(format!("{MACOS_APP}-installing"));
    if incoming.exists() {
        fs::remove_dir_all(&incoming).map_err(|error| error.to_string())?;
    }
    run(Command::new("ditto").arg(app).arg(&incoming))?;
    if installed.exists() {
        fs::remove_dir_all(installed)
            .map_err(|error| format!("failed to remove the installed {MACOS_APP}: {error}"))?;
    }
    fs::rename(&incoming, installed)
        .map_err(|error| format!("failed to install {MACOS_APP}: {error}"))
}

/// Runs git in the workspace and returns what it printed, trimmed, or what
/// it said went wrong.
fn git(args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace_root())
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

#[cfg(test)]
mod tests;
