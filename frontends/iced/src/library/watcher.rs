//! Watches the library folders, so games added to them (or removed from
//! them) show up right away (desktop build). It uses the operating system's
//! own change notifications, so it costs nothing while nothing changes.

use super::WatchEvent;
use crate::settings::LibraryFolder;
use iced::Subscription;
use iced::futures::{SinkExt, executor};
use notify_debouncer_mini::notify::RecursiveMode;
use notify_debouncer_mini::{DebounceEventResult, new_debouncer};
use std::path::Path;
use std::time::Duration;

/// Changes are collected for this long before the library hears about
/// them, so copying a game in leads to one rescan instead of dozens.
const QUIET_TIME: Duration = Duration::from_secs(1);

/// The file extensions of ROMs and zipped ROMs.
const EXTENSIONS: [&str; 3] = ["gb", "gbc", "zip"];

/// Reports changes to ROMs and folders in `folders`, including their
/// subfolders where the folder says so. It starts over when the folders
/// change.
pub fn subscription(folders: &[LibraryFolder]) -> Subscription<WatchEvent> {
    Subscription::run_with(folders.to_vec(), |folders| {
        let folders = folders.clone();

        iced::stream::channel(16, async move |mut output| {
            let sender = output.clone();
            let debouncer = new_debouncer(QUIET_TIME, move |result: DebounceEventResult| {
                let event = match result {
                    Ok(events) if events.iter().any(|event| matters(&event.path)) => {
                        WatchEvent::Changed
                    }
                    Ok(_) => return,
                    Err(error) => WatchEvent::Failed(error.to_string()),
                };
                let _ = executor::block_on(sender.clone().send(event));
            });

            let mut debouncer = match debouncer {
                Ok(debouncer) => debouncer,
                Err(error) => {
                    let _ = output.send(WatchEvent::Failed(error.to_string())).await;
                    return;
                }
            };

            for folder in folders.iter().filter(|folder| folder.path.is_dir()) {
                let mode = if folder.include_subfolders {
                    RecursiveMode::Recursive
                } else {
                    RecursiveMode::NonRecursive
                };

                if let Err(error) = debouncer.watcher().watch(&folder.path, mode) {
                    let _ = output.send(WatchEvent::Failed(error.to_string())).await;
                }
            }

            // Watching stops when the app stops listening, which drops the
            // debouncer.
            std::future::pending::<()>().await;
            drop(debouncer);
        })
    })
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
