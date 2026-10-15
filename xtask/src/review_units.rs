//! `cargo xtask review-units`: splits the Rust code into parts small enough
//! for the nightly review to read in detail, and picks the next one in
//! rotation (see `docs/NIGHTLY_REVIEW.md`).
//!
//! The parts come from the file tree as it is, so new, moved and deleted
//! files need no list kept up to date. A module (`apu.rs` with its `apu/`
//! folder) is one part when it is small enough; a larger one splits into its
//! own file and its child modules, and small neighbours are packed together
//! again. The rotation remembers when each file was last reviewed, not each
//! part, so regrouping the parts as the code changes loses nothing: a part
//! counts as last reviewed when its least recently reviewed file was. A file
//! moved without changes keeps its date under its new path, so moving
//! folders doesn't send their files to the front or back of the rotation.

use crate::{Result, workspace_root};
use clap::{Args, Subcommand};
use serde_json::{Map, Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

/// The most lines a part has, unless one file alone has more.
const PART_LINES: usize = 1200;
/// The folders whose crates are reviewed, relative to the workspace root.
const CRATE_FOLDERS: [&str; 3] = ["crates", "frontends", "tools"];
/// Folders that never hold reviewed code.
const SKIPPED_FOLDERS: [&str; 2] = ["target", "node_modules"];

/// Files reviewed together in one night.
struct Part {
    /// What the part holds, such as `crates/nubo-core/src/apu`.
    name: String,
    /// The files, relative to the workspace root with `/` separators.
    files: Vec<String>,
    /// How many lines its files hold together.
    lines: usize,
}

/// A Rust module: its file, its folder of child modules, or both.
struct Module {
    /// Its path from the workspace root without `.rs`, such as
    /// `crates/nubo-core/src/apu`.
    name: String,
    /// Its file, such as `apu.rs`, if it has one.
    file: Option<PathBuf>,
    /// Its folder of child modules, such as `apu/`, if it has one.
    folder: Option<PathBuf>,
}

/// What `cargo xtask review-units` can be asked to do besides listing the
/// parts. The doc comments are the help that `--help` prints.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Pick tonight's part, record it in the rotation's state and print what the review should
    /// cover as JSON
    Next(NextOptions),
}

/// The options of `review-units next`.
#[derive(Args, Debug)]
pub struct NextOptions {
    /// The rotation's state file, which is created when it doesn't exist
    #[arg(long, value_name = "ROTATION.JSON")]
    state: PathBuf,
    /// Tonight's date, YYYY-MM-DD
    #[arg(long)]
    date: String,
    /// The commit tonight's review reads, as a hash
    #[arg(long, value_name = "SHA")]
    commit: String,
}

/// Runs the command: none lists the parts, `next` picks tonight's part and
/// records it in the rotation's state.
pub fn run(command: Option<Command>) -> Result {
    match command {
        None => {
            let parts = parts()?;
            for part in &parts {
                println!("{:>6}  {}", part.lines, part.name);
            }
            let lines: usize = parts.iter().map(|part| part.lines).sum();
            println!("{} parts, {lines} lines", parts.len());
            Ok(())
        }
        Some(Command::Next(options)) => {
            println!("{}", next_part(&options)?);
            Ok(())
        }
    }
}

/// Picks tonight's part, records it and the files changed since the last
/// night as reviewed on `options.date` in the state file, and returns what
/// the review should cover as JSON.
///
/// The state is `{"last_commit": "<sha>", "files": {"<path>": "<date>"}}`.
/// A missing state file starts the rotation from the beginning.
fn next_part(options: &NextOptions) -> Result<String> {
    let mut state = match fs::read_to_string(&options.state) {
        Ok(text) => serde_json::from_str::<Value>(&text)
            .map_err(|error| format!("{} is not valid JSON: {error}", options.state.display()))?,
        Err(_) => json!({}),
    };
    let last_commit = state
        .get("last_commit")
        .and_then(Value::as_str)
        .map(String::from);
    let mut reviewed = state
        .get("files")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let changes = match &last_commit {
        Some(last) => rust_changes(last, &options.commit)?,
        None => Vec::new(),
    };
    carry_over_moves(&mut reviewed, &changes);
    forget_missing(&mut reviewed, |file| workspace_root().join(file).is_file());
    let changed_files: Vec<String> = changed_files(&changes)
        .filter(|file| workspace_root().join(file).is_file())
        .collect();
    for file in &changed_files {
        reviewed.insert(file.clone(), Value::from(options.date.as_str()));
    }

    let parts = parts()?;
    let part = parts
        .iter()
        .min_by_key(|part| last_reviewed(part, &reviewed))
        .ok_or("there is no Rust code to review")?;
    for file in &part.files {
        reviewed.insert(file.clone(), Value::from(options.date.as_str()));
    }

    state["last_commit"] = Value::from(options.commit.as_str());
    state["files"] = Value::Object(reviewed);
    let text = serde_json::to_string_pretty(&state).map_err(|error| error.to_string())?;
    fs::write(&options.state, text + "\n")
        .map_err(|error| format!("failed to write {}: {error}", options.state.display()))?;

    let tonight = json!({
        "date": options.date,
        "commit": options.commit,
        "changes_since": last_commit,
        "changed_files": changed_files,
        "part": {"name": part.name, "lines": part.lines, "files": part.files},
    });
    serde_json::to_string_pretty(&tonight).map_err(|error| error.to_string())
}

/// When the least recently reviewed file of `part` was reviewed, or `None`
/// when one of its files never was, which comes first.
fn last_reviewed<'a>(part: &Part, reviewed: &'a Map<String, Value>) -> Option<&'a str> {
    part.files
        .iter()
        .map(|file| reviewed.get(file).and_then(Value::as_str))
        .min()
        .flatten()
}

/// A Rust file that git sees as changed between two commits.
#[derive(Debug, PartialEq)]
enum Change {
    /// Added (also by copying), changed or deleted in place.
    Edited(String),
    /// Moved from `from` to `to`; `unchanged` when its content is the same.
    Moved {
        from: String,
        to: String,
        unchanged: bool,
    },
}

/// The Rust files that changed between the commits `from` and `to`, with
/// moved files found by git's rename detection.
fn rust_changes(from: &str, to: &str) -> Result<Vec<Change>> {
    let output = process::Command::new("git")
        .args([
            "diff",
            "--name-status",
            "-M",
            &format!("{from}..{to}"),
            "--",
            "*.rs",
        ])
        .current_dir(workspace_root())
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git diff {from}..{to} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(parse_name_status(&String::from_utf8_lossy(&output.stdout)))
}

/// Reads the output of `git diff --name-status`: a status letter (`R` with
/// a similarity score for a move, such as `R100`, and `C` with one for a
/// copy) and the paths, separated by tabs. A copy counts as an edit of the
/// new file; the file it was copied from is unchanged.
fn parse_name_status(output: &str) -> Vec<Change> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let status = fields.next()?;
            let path = fields.next()?.to_string();
            if status.starts_with('R') {
                Some(Change::Moved {
                    from: path,
                    to: fields.next()?.to_string(),
                    unchanged: status == "R100",
                })
            } else if status.starts_with('C') {
                Some(Change::Edited(fields.next()?.to_string()))
            } else {
                Some(Change::Edited(path))
            }
        })
        .collect()
}

/// Moves the review date of each moved file in `reviewed` to its new path.
fn carry_over_moves(reviewed: &mut Map<String, Value>, changes: &[Change]) {
    for change in changes {
        if let Change::Moved { from, to, .. } = change
            && let Some(date) = reviewed.remove(from)
        {
            reviewed.insert(to.clone(), date);
        }
    }
}

/// Drops the review dates of files that are gone, such as deleted ones, so
/// the state only holds files that `exists` still finds.
fn forget_missing(reviewed: &mut Map<String, Value>, exists: impl Fn(&str) -> bool) {
    reviewed.retain(|file, _| exists(file));
}

/// The paths of the files whose content changed: everything but files only
/// moved.
fn changed_files(changes: &[Change]) -> impl Iterator<Item = String> + '_ {
    changes.iter().filter_map(|change| match change {
        Change::Edited(path)
        | Change::Moved {
            to: path,
            unchanged: false,
            ..
        } => Some(path.clone()),
        Change::Moved {
            unchanged: true, ..
        } => None,
    })
}

/// Every reviewed crate's code, split into parts in path order.
fn parts() -> Result<Vec<Part>> {
    let root = workspace_root();
    let mut crates = Vec::new();
    for folder in CRATE_FOLDERS {
        for entry in sorted_entries(&root.join(folder))? {
            if entry.join("Cargo.toml").is_file() {
                crates.push(entry);
            }
        }
    }
    crates.push(root.join("xtask"));

    let mut items = Vec::new();
    for folder in crates {
        let module = Module {
            name: relative(&folder),
            file: None,
            folder: Some(folder),
        };
        split(&module, &mut items)?;
    }
    Ok(pack(items))
}

/// Adds `module` as one part to `parts` when it is small enough, otherwise
/// its own file and the parts of its child modules, packed together.
fn split(module: &Module, parts: &mut Vec<Part>) -> Result {
    let files = module_files(module)?;
    let lines = files.iter().map(|(_, lines)| lines).sum();
    if lines <= PART_LINES || files.len() == 1 {
        if !files.is_empty() {
            parts.push(Part {
                name: module.name.clone(),
                files: files.into_iter().map(|(file, _)| file).collect(),
                lines,
            });
        }
        return Ok(());
    }

    let mut items = Vec::new();
    if let Some(file) = &module.file {
        items.push(Part {
            name: relative(file),
            files: vec![relative(file)],
            lines: count_lines(file)?,
        });
    }
    if let Some(folder) = &module.folder {
        for child in child_modules(folder)? {
            split(&child, &mut items)?;
        }
    }
    parts.extend(pack(items));
    Ok(())
}

/// Joins neighbouring parts while they fit in `PART_LINES` together.
fn pack(items: Vec<Part>) -> Vec<Part> {
    let mut packed: Vec<Part> = Vec::new();
    for item in items {
        match packed.last_mut() {
            Some(last) if last.lines + item.lines <= PART_LINES => {
                last.name = format!("{} + {}", last.name, item.name);
                last.files.extend(item.files);
                last.lines += item.lines;
            }
            _ => packed.push(item),
        }
    }
    packed
}

/// The modules in `folder`: each `.rs` file with its folder of the same name
/// if there is one, and each folder without such a file.
fn child_modules(folder: &Path) -> Result<Vec<Module>> {
    let entries = sorted_entries(folder)?;
    let mut modules = Vec::new();
    for entry in &entries {
        let is_rust_file = entry.extension().is_some_and(|extension| extension == "rs");
        if is_rust_file {
            let module_folder = entry.with_extension("");
            modules.push(Module {
                name: relative(&module_folder),
                file: Some(entry.clone()),
                folder: module_folder.is_dir().then_some(module_folder),
            });
        } else if entry.is_dir() && !entry.with_extension("rs").is_file() {
            modules.push(Module {
                name: relative(entry),
                file: None,
                folder: Some(entry.clone()),
            });
        }
    }
    Ok(modules)
}

/// Every `.rs` file of `module` with its line count, in path order.
fn module_files(module: &Module) -> Result<Vec<(String, usize)>> {
    let mut files = Vec::new();
    if let Some(file) = &module.file {
        files.push((relative(file), count_lines(file)?));
    }
    if let Some(folder) = &module.folder {
        for child in child_modules(folder)? {
            files.extend(module_files(&child)?);
        }
    }
    Ok(files)
}

/// The entries of `folder` that may hold code, sorted by path.
fn sorted_entries(folder: &Path) -> Result<Vec<PathBuf>> {
    let mut entries: Vec<PathBuf> = fs::read_dir(folder)
        .map_err(|error| format!("failed to read {}: {error}", folder.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                let name = name.to_string_lossy();
                !name.starts_with('.') && !SKIPPED_FOLDERS.contains(&name.as_ref())
            })
        })
        .collect();
    entries.sort();
    Ok(entries)
}

fn count_lines(file: &Path) -> Result<usize> {
    let text = fs::read_to_string(file)
        .map_err(|error| format!("failed to read {}: {error}", file.display()))?;
    Ok(text.lines().count())
}

/// `path` relative to the workspace root, with `/` separators on every
/// system.
fn relative(path: &Path) -> String {
    let path = path.strip_prefix(workspace_root()).unwrap_or(path);
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests;
