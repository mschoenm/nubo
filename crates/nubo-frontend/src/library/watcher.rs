//! Watches the library folders, so games added to them (or removed from
//! them) show up right away (the `desktop` feature). It uses the operating
//! system's own change notifications, through `notify-debouncer-mini`, so it
//! costs nothing while nothing changes.

use super::WatchEvent;
use crate::settings::LibraryFolder;
use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};
use std::path::Path;
use std::time::Duration;

/// Changes are collected for this long before the library hears about
/// them, so copying a game in leads to one rescan instead of dozens.
const QUIET_TIME: Duration = Duration::from_secs(1);

/// The file extensions of ROMs and zipped ROMs.
const EXTENSIONS: [&str; 3] = ["gb", "gbc", "zip"];

/// Watches the library folders until it is dropped; see [`watch`].
pub struct Watcher {
    /// Collects the operating system's change notifications on a thread of
    /// its own. Dropping it stops watching.
    _debouncer: Debouncer<RecommendedWatcher>,
}

/// What starting to watch the library folders gave (see [`watch`]).
pub struct Watching {
    /// Watches until it is dropped, or `None` when watching couldn't start
    /// at all.
    pub watcher: Option<Watcher>,
    /// Why folders can't be watched, or why watching couldn't start, for
    /// the caller to report as [`WatchEvent::Failed`].
    pub failures: Vec<String>,
}

/// Reports changes to ROMs and folders in `folders`, including their
/// subfolders where the folder says so, by calling `report` on the
/// watcher's own thread, which may block until the app takes the event.
/// Watching goes on until the returned watcher is dropped, such as when the
/// app stops listening because the folders changed.
///
/// What goes wrong while starting (a folder that can't be watched, or no
/// watcher at all) is returned instead of reported, so the caller can pass
/// it on without blocking: `report` never runs on the caller's thread.
pub fn watch(folders: &[LibraryFolder], report: impl Fn(WatchEvent) + Send + 'static) -> Watching {
    let debouncer = new_debouncer(QUIET_TIME, move |result: DebounceEventResult| {
        let event = match result {
            Ok(events) if events.iter().any(|event| matters(&event.path)) => WatchEvent::Changed,
            Ok(_) => return,
            Err(error) => WatchEvent::Failed(error.to_string()),
        };
        report(event);
    });

    let mut debouncer = match debouncer {
        Ok(debouncer) => debouncer,
        Err(error) => {
            return Watching {
                watcher: None,
                failures: vec![error.to_string()],
            };
        }
    };
    let mut failures = Vec::new();

    for folder in folders.iter().filter(|folder| folder.path.is_dir()) {
        let mode = if folder.include_subfolders {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };

        if let Err(error) = debouncer.watcher().watch(&folder.path, mode) {
            failures.push(error.to_string());
        }
    }

    Watching {
        watcher: Some(Watcher {
            _debouncer: debouncer,
        }),
        failures,
    }
}

/// Whether a change at `path` can affect the library: a ROM, or a folder
/// (which may hold ROMs). Other files, such as a browser's downloads in
/// progress or hidden files, are ignored, so a busy folder doesn't keep
/// causing rescans.
fn matters(path: &Path) -> bool {
    let hidden = path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with('.'));
    if hidden {
        return false;
    }

    match path.extension() {
        Some(extension) => EXTENSIONS
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known)),
        // A removed folder can't be asked whether it was one, so anything
        // without an extension counts.
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::matters;
    use std::path::Path;

    #[test]
    fn roms_and_folders_matter() {
        assert!(matters(Path::new("/games/Tetris.gb")));
        assert!(matters(Path::new("/games/Zelda.GBC")));
        assert!(matters(Path::new("/games/Pokemon.zip")));
        assert!(matters(Path::new("/games/New folder")));
    }

    #[test]
    fn other_and_hidden_files_dont() {
        assert!(!matters(Path::new("/games/notes.txt")));
        assert!(!matters(Path::new("/games/Tetris.gb.part")));
        assert!(!matters(Path::new("/games/.DS_Store")));
        assert!(!matters(Path::new("/games/.hidden.gb")));
    }
}
