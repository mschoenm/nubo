//! Watches the library folders, so games added to them (or removed from
//! them) show up right away, and the files of the recently played games
//! outside them, so a deleted one leaves "Continue playing" right away too
//! (the `desktop` feature). It uses the operating system's own change
//! notifications, through `notify-debouncer-full`, so it costs nothing
//! while nothing changes. Linux also reports files and folders that are
//! only opened or read, as the library's own scans do; those are ignored,
//! or every scan would start the next one.

use super::WatchEvent;
use crate::rom::EXTENSIONS;
use crate::settings::LibraryFolder;
use notify_debouncer_full::notify::event::{AccessKind, AccessMode};
use notify_debouncer_full::notify::{self, EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, NoCache, new_debouncer_opt};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Changes are collected for this long before the library hears about
/// them, so copying a game in leads to one rescan instead of dozens.
const QUIET_TIME: Duration = Duration::from_secs(1);

/// Watches the library folders until it is dropped; see [`watch`].
pub struct Watcher {
    /// Collects the operating system's change notifications on a thread of
    /// its own. Dropping it stops watching.
    _debouncer: Debouncer<RecommendedWatcher, NoCache>,
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
/// subfolders where the folder says so, and to the files of `games` (the
/// recently played games outside those folders, a handful), by calling
/// `report` on the watcher's own thread, which may block until the app
/// takes the event. A game's file is watched through the folder it is in,
/// where only that file counts, so other files there cause no rescans.
/// Watching goes on until the returned watcher is dropped, such as when the
/// app stops listening because the folders changed.
///
/// What goes wrong while starting (a folder that can't be watched, or no
/// watcher at all) is returned instead of reported, so the caller can pass
/// it on without blocking: `report` never runs on the caller's thread.
pub fn watch(
    folders: &[LibraryFolder],
    games: &[PathBuf],
    report: impl Fn(WatchEvent) + Send + 'static,
) -> Watching {
    let (watched_folders, watched_games) = (folders.to_vec(), games.to_vec());
    // No cache of file IDs: it only pairs up renames, and filling it would
    // read every library folder each time watching starts.
    let debouncer = new_debouncer_opt(
        QUIET_TIME,
        None,
        move |result: DebounceEventResult| {
            let event = match result {
                Ok(events)
                    if events.iter().any(|event| {
                        changes(event.kind)
                            && event
                                .paths
                                .iter()
                                .any(|path| affects(path, &watched_folders, &watched_games))
                    }) =>
                {
                    WatchEvent::Changed
                }
                Ok(_) => return,
                Err(errors) => WatchEvent::Failed(
                    errors
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
            };
            report(event);
        },
        NoCache,
        notify::Config::default(),
    );

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

        if let Err(error) = debouncer.watch(&folder.path, mode) {
            failures.push(error.to_string());
        }
    }

    for parent in game_folders(folders, games) {
        if let Err(error) = debouncer.watch(&parent, RecursiveMode::NonRecursive) {
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

/// The folders to watch for the files of `games`: each game's folder once,
/// unless a library folder's watch already sees it. Watching a folder again
/// would replace that watch, and could stop it from seeing subfolders.
fn game_folders(folders: &[LibraryFolder], games: &[PathBuf]) -> Vec<PathBuf> {
    let mut parents: Vec<PathBuf> = Vec::new();

    for parent in games.iter().filter_map(|game| game.parent()) {
        let seen = folders.iter().any(|folder| {
            parent == folder.path || (folder.include_subfolders && parent.starts_with(&folder.path))
        });
        if !seen && parent.is_dir() && !parents.iter().any(|known| known == parent) {
            parents.push(parent.to_path_buf());
        }
    }

    parents
}

/// Whether an event of `kind` can mean something changed. Opening a file or
/// folder, reading it and closing it unchanged change nothing, and the
/// library's own scans do all three; a file closed after writing does
/// count, as the end of a game being copied in.
fn changes(kind: EventKind) -> bool {
    match kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        _ => true,
    }
}

/// Whether a change at `path` can affect the library: a ROM or folder in a
/// library folder, or in its subfolders where it includes them (see
/// `matters`), or one of the `games` outside them, or the folder one of
/// those is in, if it was removed itself.
fn affects(path: &Path, folders: &[LibraryFolder], games: &[PathBuf]) -> bool {
    let in_library_folder = |folder: &LibraryFolder| {
        if folder.include_subfolders {
            path.starts_with(&folder.path)
        } else {
            path == folder.path || path.parent() == Some(folder.path.as_path())
        }
    };

    games
        .iter()
        .any(|game| game == path || game.parent() == Some(path))
        || (matters(path) && folders.iter().any(in_library_folder))
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
    use super::{QUIET_TIME, WatchEvent, affects, changes, game_folders, matters, watch};
    use crate::settings::LibraryFolder;
    use notify_debouncer_full::notify::EventKind;
    use notify_debouncer_full::notify::event::{
        AccessKind, AccessMode, CreateKind, ModifyKind, RemoveKind,
    };
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;

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

    fn folder(path: &str, include_subfolders: bool) -> LibraryFolder {
        LibraryFolder {
            path: PathBuf::from(path),
            include_subfolders,
        }
    }

    #[test]
    fn only_the_played_files_count_in_their_folders() {
        let folders = [folder("/games", true)];
        let games = [PathBuf::from("/downloads/Mario.gb")];

        assert!(affects(Path::new("/downloads/Mario.gb"), &folders, &games));
        assert!(affects(Path::new("/downloads"), &folders, &games));
        assert!(affects(Path::new("/games/Tetris.gb"), &folders, &games));
        assert!(!affects(Path::new("/downloads/Zelda.gb"), &folders, &games));
    }

    #[test]
    fn subfolders_count_only_in_folders_that_include_them() {
        let folders = [folder("/games", false)];
        let games = [PathBuf::from("/games/old/Mario.gb")];

        assert!(affects(Path::new("/games/Tetris.gb"), &folders, &games));
        assert!(affects(Path::new("/games/old/Mario.gb"), &folders, &games));
        assert!(!affects(Path::new("/games/old/Zelda.gb"), &folders, &games));
    }

    #[test]
    fn a_folder_a_library_folder_sees_isnt_watched_again() {
        let root = std::env::temp_dir().join(format!("nubo-watcher-{}", std::process::id()));
        let (games_folder, elsewhere) = (root.join("games"), root.join("elsewhere"));
        std::fs::create_dir_all(games_folder.join("old")).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let folders = [LibraryFolder {
            path: games_folder.clone(),
            include_subfolders: true,
        }];
        let games = [
            games_folder.join("old").join("Tetris.gb"),
            elsewhere.join("Mario.gb"),
            elsewhere.join("Zelda.gb"),
            root.join("gone").join("Kirby.gb"),
        ];

        let watched = game_folders(&folders, &games);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(watched, [elsewhere]);
    }

    #[test]
    fn opening_and_reading_change_nothing() {
        assert!(!changes(EventKind::Access(AccessKind::Open(
            AccessMode::Any
        ))));
        assert!(!changes(EventKind::Access(AccessKind::Read)));
        assert!(!changes(EventKind::Access(AccessKind::Close(
            AccessMode::Read
        ))));
        assert!(changes(EventKind::Access(AccessKind::Close(
            AccessMode::Write
        ))));
        assert!(changes(EventKind::Create(CreateKind::File)));
        assert!(changes(EventKind::Modify(ModifyKind::Any)));
        assert!(changes(EventKind::Remove(RemoveKind::Folder)));
    }

    #[test]
    fn a_scan_reading_the_folders_doesnt_start_another_but_a_new_game_does() {
        let root = std::env::temp_dir().join(format!("nubo-watcher-reads-{}", std::process::id()));
        std::fs::create_dir_all(root.join("more")).unwrap();
        // macOS's temporary folder is behind a symbolic link, and its change
        // notifications name the real path.
        let root = std::fs::canonicalize(root).unwrap();
        std::fs::write(root.join("Tetris.gb"), [0; 16]).unwrap();
        let folders = [LibraryFolder {
            path: root.clone(),
            include_subfolders: true,
        }];
        let (sender, events) = mpsc::channel();
        let watching = watch(&folders, &[], move |event| {
            let _ = sender.send(event);
        });
        assert!(watching.failures.is_empty(), "{:?}", watching.failures);
        std::thread::sleep(QUIET_TIME / 2);

        for folder in [root.clone(), root.join("more")] {
            std::fs::read_dir(folder).unwrap().for_each(drop);
        }
        std::fs::read(root.join("Tetris.gb")).unwrap();
        let after_reading = events.recv_timeout(QUIET_TIME * 3);

        std::fs::write(root.join("more").join("Zelda.gb"), [0; 16]).unwrap();
        let after_writing = events.recv_timeout(QUIET_TIME * 5);

        drop(watching);
        std::fs::remove_dir_all(&root).unwrap();
        assert!(after_reading.is_err(), "reading reported {after_reading:?}");
        assert!(
            matches!(after_writing, Ok(WatchEvent::Changed)),
            "writing reported {after_writing:?}"
        );
    }
}
