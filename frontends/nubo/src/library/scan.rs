//! Finds the ROMs in the library folders and reads their headers with
//! `Emulator::read_rom_header`.

use super::LibraryEntry;
use nubo_core::{CartridgeHeader, Emulator};
use nubo_frontend::settings::LibraryFolder;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// The file extensions of ROMs and zipped ROMs.
const EXTENSIONS: [&str; 3] = ["gb", "gbc", "zip"];
/// The largest Game Boy ROMs are 8 MiB; bigger files are something else.
const MAX_FILE_SIZE: u64 = 16 * 1024 * 1024;

/// What a scan found.
#[derive(Debug, Clone, Default)]
pub struct ScanResult {
    /// The scan's number, see `Library::scan_generation`.
    pub generation: u64,
    pub entries: Vec<LibraryEntry>,
    /// Library folders that don't exist (any more).
    pub missing_folders: Vec<PathBuf>,
    /// Games kept in the library although their file can't be reached,
    /// because the folder they are on is gone, such as an unplugged drive.
    pub unavailable: Vec<PathBuf>,
    /// The games the scan knew about when it started. Games missing from
    /// `entries` that aren't in here were added while it ran.
    pub checked: HashSet<PathBuf>,
    /// Why the scan stopped before it finished, if it did. It then found
    /// nothing, and the library stays as it was.
    pub error: Option<String>,
}

impl ScanResult {
    /// A scan that stopped before it finished, for `error`.
    pub fn failed(generation: u64, error: String) -> Self {
        ScanResult {
            generation,
            error: Some(error),
            ..ScanResult::default()
        }
    }
}

/// What a quick look at the library's files found, without scanning the
/// folders; see [`check`].
#[derive(Debug, Clone, Default)]
pub struct FileCheck {
    /// Games whose file was deleted: their folder is there, the file isn't.
    pub deleted: Vec<PathBuf>,
    /// Games whose folder is gone, such as an unplugged drive.
    pub unavailable: Vec<PathBuf>,
    /// How many scans the library had taken in when the check started; see
    /// `Library::finish_check`.
    pub finished_scans: u64,
}

/// Looks for ROMs in `folders` and returns an entry for each, reusing the
/// `known` entry of files that didn't change. Recently played games outside
/// the folders stay in the library as long as their file exists. Files that
/// aren't valid ROMs are left out.
///
/// A known game whose file is gone is forgotten, unless the folder it is on
/// is gone too: then it is most likely on a drive that is unplugged, and it
/// is kept (and listed as unavailable) so it comes back as it was.
pub fn scan(generation: u64, folders: &[LibraryFolder], known: Vec<LibraryEntry>) -> ScanResult {
    let mut paths = BTreeMap::new();
    let mut missing_folders = Vec::new();

    for folder in folders {
        if folder.path.is_dir() {
            collect_roms(&folder.path, folder.include_subfolders, &mut paths);
        } else {
            missing_folders.push(folder.path.clone());
        }
    }

    let mut unavailable = Vec::new();
    for entry in &known {
        if paths.contains_key(&entry.path) {
            continue;
        }

        if entry.path.is_file() {
            if entry.last_played.is_some() {
                paths.insert(entry.path.clone(), false);
            }
        } else if is_unreachable(&entry.path, folders) {
            unavailable.push(entry.path.clone());
        }
    }

    let checked = known.iter().map(|entry| entry.path.clone()).collect();
    let mut known: HashMap<PathBuf, LibraryEntry> = known
        .into_iter()
        .map(|entry| (entry.path.clone(), entry))
        .collect();

    let mut entries: Vec<LibraryEntry> = paths
        .into_iter()
        .filter_map(|(path, in_folder)| {
            let (size, modified) = file_stamp(&path)?;
            let mut entry = match known.remove(&path) {
                Some(entry) if entry.size == size && entry.modified == modified => entry,
                previous => {
                    let mut entry = read_entry(&path, size)?;
                    entry.last_played = previous.and_then(|previous| previous.last_played);
                    entry
                }
            };
            entry.in_folder = in_folder;
            entry.size = size;
            entry.modified = modified;
            Some(entry)
        })
        .collect();

    entries.extend(unavailable.iter().filter_map(|path| known.remove(path)));

    ScanResult {
        generation,
        entries,
        missing_folders,
        unavailable,
        checked,
        error: None,
    }
}

/// Sorts the games in `entries` whose file is gone into deleted ones and
/// ones on a folder that is gone, without scanning the folders. It only
/// asks whether files and folders exist, so it is quick.
pub fn check(entries: &[LibraryEntry], folders: &[LibraryFolder]) -> FileCheck {
    let mut check = FileCheck::default();

    for entry in entries {
        if entry.path.is_file() {
            continue;
        }

        if is_unreachable(&entry.path, folders) {
            check.unavailable.push(entry.path.clone());
        } else {
            check.deleted.push(entry.path.clone());
        }
    }

    check
}

/// Whether the folder a missing game was on is gone too: the library folder
/// it is in, or for a game outside the library folders, the folder it was
/// in. A missing file in a folder that is still there was deleted.
fn is_unreachable(path: &Path, folders: &[LibraryFolder]) -> bool {
    let library_folder = folders
        .iter()
        .filter(|folder| path.starts_with(&folder.path))
        .max_by_key(|folder| folder.path.components().count());

    match library_folder {
        Some(folder) => !folder.path.is_dir(),
        None => !path.parent().is_some_and(Path::is_dir),
    }
}

/// A library entry for the ROM at `path` with the given header, found
/// outside the library folders.
pub fn entry_for(path: &Path, header: &CartridgeHeader) -> LibraryEntry {
    let (size, modified) = file_stamp(path).unwrap_or_default();

    LibraryEntry {
        path: path.to_path_buf(),
        title: header.title.clone(),
        supports_cgb: header.supports_cgb,
        in_folder: false,
        last_played: None,
        title_screen_checked: false,
        box_art_checked: false,
        size,
        modified,
    }
}

/// Adds the ROMs in `folder` to `paths`, marked as being in a library folder,
/// and those in its subfolders too if `recursive` is set. Hidden files and
/// folders (whose name starts with a dot) and symbolic links are skipped, so
/// a link back up the tree can't make the scan go round in circles.
fn collect_roms(folder: &Path, recursive: bool, paths: &mut BTreeMap<PathBuf, bool>) {
    let Ok(children) = fs::read_dir(folder) else {
        return;
    };

    for child in children.flatten() {
        let path = child.path();
        let Ok(file_type) = child.file_type() else {
            continue;
        };

        if child.file_name().to_string_lossy().starts_with('.') {
            continue;
        }

        if file_type.is_dir() && recursive {
            collect_roms(&path, true, paths);
        } else if file_type.is_file() && is_rom_file(&path) {
            paths.insert(path, true);
        }
    }
}

/// Whether the file has a ROM extension and a path the library file can
/// store (TOML strings must be valid UTF-8).
fn is_rom_file(path: &Path) -> bool {
    path.to_str().is_some()
        && path.extension().is_some_and(|extension| {
            EXTENSIONS
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

/// The file's size and modification time, or `None` if it can't be read.
fn file_stamp(path: &Path) -> Option<(u64, u64)> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_secs());
    Some((metadata.len(), modified))
}

/// Reads the ROM's header, or returns `None` if it isn't a ROM Nubo can
/// run.
fn read_entry(path: &Path, size: u64) -> Option<LibraryEntry> {
    if size > MAX_FILE_SIZE {
        return None;
    }

    let rom = fs::read(path).ok()?;
    let header = Emulator::read_rom_header(&rom).ok()?;
    Some(entry_for(path, &header))
}
