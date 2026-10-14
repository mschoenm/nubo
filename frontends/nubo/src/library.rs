//! The game library: the ROMs found in the folders the user added, plus the
//! games they played recently, with a picture of where they left off.
//!
//! What the library knows is kept in `library.toml` in the user's data
//! folder, so it shows up instantly when the app starts, and a scan in the
//! background brings it up to date. Each game also gets a picture of its
//! title screen (see `title_screens.rs`) and of the last screen played,
//! PNG files in the `title_screens` and `thumbnails` folders next to it.
//! The browser build has no folders to scan, so its library stays empty.

use jobs::Jobs;
use nubo_core::CartridgeHeader;
use nubo_frontend::background;
use nubo_frontend::palettes::DmgPalette;
use nubo_frontend::picture::Picture;
use nubo_frontend::picture::{SCREEN_HEIGHT, SCREEN_WIDTH};
use nubo_frontend::time::SystemTime;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub use scan::{FileCheck, ScanResult};

/// How many title screens are captured at the same time.
const MAX_CAPTURES: usize = 2;
/// How many games have their box art looked up at the same time.
const MAX_DOWNLOADS: usize = 4;

/// The library is only available where the app can read folders.
pub const AVAILABLE: bool = cfg!(not(target_arch = "wasm32"));

/// A game the library knows about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryEntry {
    pub path: PathBuf,
    /// The title from the cartridge header.
    pub title: String,
    pub supports_cgb: bool,
    /// Whether the file is in one of the library folders. Games opened from
    /// elsewhere are only listed as recently played.
    pub in_folder: bool,
    /// When the game was last started, in seconds since 1970.
    pub last_played: Option<u64>,
    /// Whether the game was already run to capture its title screen (even
    /// if it showed nothing worth keeping), so it isn't run again.
    #[serde(default)]
    pub title_screen_checked: bool,
    /// Whether the game's box art was already looked up (even if the
    /// collection has none), so it isn't looked up again.
    #[serde(default)]
    pub box_art_checked: bool,
    /// The file's size and modification time (in seconds since 1970) when
    /// its header was read, so a scan can tell whether to read it again.
    size: u64,
    modified: u64,
}

impl LibraryEntry {
    /// The game's name, taken from its file name, such as "Tetris" for
    /// `Tetris.gb`.
    pub fn name(&self) -> String {
        self.path
            .file_stem()
            .unwrap_or(self.path.as_os_str())
            .to_string_lossy()
            .into_owned()
    }

    /// A game at `path` in a library folder, for tests of the library
    /// screen.
    #[cfg(test)]
    pub fn example(path: &str) -> LibraryEntry {
        LibraryEntry {
            path: PathBuf::from(path),
            title: String::new(),
            supports_cgb: false,
            in_folder: true,
            last_played: None,
            title_screen_checked: false,
            box_art_checked: false,
            size: 0,
            modified: 0,
        }
    }

    /// Whether the search text matches the game's name or header title,
    /// ignoring case.
    pub fn matches(&self, search: &str) -> bool {
        let search = search.to_lowercase();
        self.name().to_lowercase().contains(&search) || self.title.to_lowercase().contains(&search)
    }
}

/// The games and their thumbnails.
#[derive(Debug, Default)]
pub struct Library {
    /// Every known game, sorted by name.
    entries: Vec<LibraryEntry>,
    thumbnails: HashMap<PathBuf, Picture>,
    /// Pictures of the games' title screens, in the chosen DMG palette.
    title_screens: HashMap<PathBuf, Picture>,
    captures: Jobs<MAX_CAPTURES>,
    /// Downloaded box art, for the games the collection has it for.
    box_art: HashMap<PathBuf, Picture>,
    downloads: Jobs<MAX_DOWNLOADS>,
    /// Folders that couldn't be found during the last scan.
    missing_folders: Vec<PathBuf>,
    /// Games kept but not shown because the folder they are on is gone,
    /// such as an unplugged drive.
    unavailable: HashSet<PathBuf>,
    /// Counts the scans started, so the result of an older scan that
    /// finishes late is ignored.
    scan_generation: u64,
    scanning: bool,
    /// Whether another scan was asked for (with the Rescan button, by the
    /// folder watcher or by changing the folders) while one was running.
    /// Only one scan runs at a time; this one starts when it is done.
    rescan_wanted: bool,
    /// Counts the scans whose result was taken in, so a quick file check
    /// that started before a scan finished doesn't undo what it found.
    finished_scans: u64,
}

impl Library {
    /// Reads what the library knew when the app last ran. If that can't be
    /// read, the library starts empty and the reason is returned too.
    pub fn load() -> (Library, Option<String>) {
        let (entries, error) = match store::load_entries() {
            Ok(entries) => (entries, None),
            Err(error) => (
                Vec::new(),
                Some(format!("The game library couldn't be read: {error}")),
            ),
        };

        let mut library = Library::default();
        library.replace_entries(entries);
        (library, error)
    }

    /// The games in the library folders that match `search`, by name.
    pub fn games<'a>(&'a self, search: &'a str) -> impl Iterator<Item = &'a LibraryEntry> {
        self.entries.iter().filter(move |entry| {
            entry.in_folder && entry.matches(search) && !self.unavailable.contains(&entry.path)
        })
    }

    /// Up to `limit` games, most recently played first.
    pub fn recently_played(&self, limit: usize) -> Vec<&LibraryEntry> {
        let mut played: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| entry.last_played.is_some() && !self.unavailable.contains(&entry.path))
            .collect();
        played.sort_by_key(|entry| std::cmp::Reverse(entry.last_played));
        played.truncate(limit);
        played
    }

    pub fn thumbnail(&self, path: &Path) -> Option<&Picture> {
        self.thumbnails.get(path)
    }

    pub fn title_screen(&self, path: &Path) -> Option<&Picture> {
        self.title_screens.get(path)
    }

    pub fn box_art(&self, path: &Path) -> Option<&Picture> {
        self.box_art.get(path)
    }

    /// Queues the games that have no title screen yet, and returns the
    /// ones to capture now: a few at a time, so the computer stays
    /// responsive.
    pub fn next_captures(&mut self) -> Vec<PathBuf> {
        self.captures.add(
            self.entries
                .iter()
                .filter(|entry| !entry.title_screen_checked)
                .map(|entry| entry.path.clone()),
        );
        self.captures.start()
    }

    /// Queues the games whose box art wasn't looked up yet, and returns the
    /// ones to look up now, with whether each is a Color game.
    pub fn next_downloads(&mut self) -> Vec<(PathBuf, bool)> {
        self.downloads.add(
            self.entries
                .iter()
                .filter(|entry| !entry.box_art_checked)
                .map(|entry| entry.path.clone()),
        );

        self.downloads
            .start()
            .into_iter()
            .map(|path| {
                let supports_cgb = self
                    .entries
                    .iter()
                    .any(|entry| entry.path == path && entry.supports_cgb);
                (path, supports_cgb)
            })
            .collect()
    }

    /// Keeps the box art looked up for the game at `path`, or `None` when
    /// the collection has none. A failed download is reported, and the
    /// downloads waiting stop, since they would most likely fail too; they
    /// are tried again after the next scan.
    pub fn finish_download(
        &mut self,
        path: &Path,
        result: Result<Option<DownloadedArt>, String>,
    ) -> Result<(), String> {
        self.downloads.finish(path);

        let picture = match result {
            Ok(picture) => picture,
            Err(error) => {
                self.downloads.cancel_waiting();
                return Err(format!("Box art couldn't be downloaded: {error}"));
            }
        };

        let mut saved = Ok(());
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.path == path) {
            entry.box_art_checked = true;

            if let Some(art) = picture {
                saved = store::save_box_art(path, &art.png);
                self.box_art.insert(path.to_path_buf(), art.picture);
            }
        }

        if self.downloads.is_idle() {
            saved.and(self.save())
        } else {
            saved
        }
    }

    /// Keeps the title screen captured for the game at `path` (`None` when
    /// it showed nothing worth keeping), shown in `palette` if it's an
    /// original Game Boy game. The library file is saved once every queued
    /// game is done.
    pub fn finish_capture(
        &mut self,
        path: &Path,
        pixels: Option<Vec<u8>>,
        palette: DmgPalette,
    ) -> Result<(), String> {
        self.captures.finish(path);

        let mut saved = Ok(());
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.path == path) {
            entry.title_screen_checked = true;

            if let Some(mut pixels) = pixels {
                saved = store::save_title_screen(path, &pixels);
                if !entry.supports_cgb {
                    palette.recolor(&mut pixels);
                }
                self.title_screens
                    .insert(path.to_path_buf(), screen_picture(pixels));
            }
        }

        if self.captures.is_idle() {
            saved.and(self.save())
        } else {
            saved
        }
    }

    /// Whether any game still needs its box art looked up.
    pub fn needs_downloads(&self) -> bool {
        self.entries.iter().any(|entry| !entry.box_art_checked)
    }

    /// Reads the saved pictures on a background thread, decoded so the
    /// window never has to. Each game gets the one picture its card shows:
    /// its box art (if `show_box_art`), else its title screen (painted in
    /// `palette` for original Game Boy games), else its last screen played.
    /// With `missing_only`, games that already have a picture are skipped.
    /// Hand the result to [`Library::set_pictures`], or show why there is
    /// none.
    pub fn load_pictures(
        &self,
        palette: DmgPalette,
        show_box_art: bool,
        missing_only: bool,
    ) -> impl Future<Output = Result<Vec<LoadedPicture>, String>> + use<> {
        let games: Vec<LibraryEntry> = self
            .entries
            .iter()
            .filter(|entry| !missing_only || !self.has_picture(&entry.path))
            .cloned()
            .collect();

        background::run(move || {
            games
                .into_iter()
                .filter_map(|entry| best_picture(&entry, palette, show_box_art))
                .collect()
        })
    }

    /// Keeps pictures read by [`Library::load_pictures`]. Each replaces
    /// whatever picture its game had.
    pub fn set_pictures(&mut self, pictures: Vec<LoadedPicture>) {
        for picture in pictures {
            for pictures in [
                &mut self.box_art,
                &mut self.title_screens,
                &mut self.thumbnails,
            ] {
                pictures.remove(&picture.path);
            }

            let pictures = match picture.kind {
                PictureKind::BoxArt => &mut self.box_art,
                PictureKind::TitleScreen => &mut self.title_screens,
                PictureKind::LastScreen => &mut self.thumbnails,
            };
            pictures.insert(picture.path, picture.picture);
        }
    }

    fn has_picture(&self, path: &Path) -> bool {
        self.box_art.contains_key(path)
            || self.title_screens.contains_key(path)
            || self.thumbnails.contains_key(path)
    }

    pub fn is_scanning(&self) -> bool {
        self.scanning
    }

    pub fn is_missing(&self, folder: &Path) -> bool {
        self.missing_folders.iter().any(|missing| missing == folder)
    }

    /// Starts a scan of `folders`, returning the work to run in the
    /// background, or `None` if a scan is already running: then another
    /// one starts as soon as it is done (see [`Library::finish_scan`]), so
    /// any number of requests during a scan lead to one more scan. Hand the
    /// result to [`Library::finish_scan`].
    pub fn start_scan(
        &mut self,
        folders: Vec<nubo_frontend::settings::LibraryFolder>,
    ) -> Option<impl Future<Output = ScanResult> + use<>> {
        if self.scanning {
            self.rescan_wanted = true;
            return None;
        }

        self.scan_generation += 1;
        self.scanning = true;

        let generation = self.scan_generation;
        let known = self.entries.clone();
        let scan = background::run(move || scan::scan(generation, &folders, known));
        Some(async move {
            scan.await
                .unwrap_or_else(|error| ScanResult::failed(generation, error))
        })
    }

    /// Whether another scan was asked for while the last one ran. Asking
    /// clears it, so the caller starts that scan.
    pub fn take_rescan_wanted(&mut self) -> bool {
        std::mem::take(&mut self.rescan_wanted)
    }

    /// Takes in the result of a scan and saves it, unless a newer scan has
    /// started since. Games played during the scan keep their play time. A
    /// scan that stopped before it finished leaves the library as it was.
    pub fn finish_scan(&mut self, result: ScanResult) -> Result<(), String> {
        if result.generation != self.scan_generation {
            return Ok(());
        }

        self.scanning = false;
        if let Some(error) = result.error {
            return Err(format!("The library folders couldn't be scanned: {error}"));
        }
        self.finished_scans += 1;
        self.missing_folders = result.missing_folders;
        self.unavailable = result.unavailable.into_iter().collect();

        let entries = merge_scan(&self.entries, result.entries, &result.checked);
        self.replace_entries(entries);
        self.save()
    }

    /// Starts a quick check, on a background thread, of whether the games'
    /// files still exist. Hand its result to [`Library::finish_check`]. A
    /// check that stopped before it finished found nothing, and the scan
    /// after it catches up.
    pub fn start_check(
        &self,
        folders: Vec<nubo_frontend::settings::LibraryFolder>,
    ) -> impl Future<Output = FileCheck> + use<> {
        let entries = self.entries.clone();
        let finished_scans = self.finished_scans;
        let check = background::run(move || scan::check(&entries, &folders));
        async move {
            FileCheck {
                finished_scans,
                ..check.await.unwrap_or_default()
            }
        }
    }

    /// Forgets the games whose file was deleted and hides the ones whose
    /// folder is gone, saving the library if anything was forgotten. A
    /// check that started before a scan finished is ignored: the scan saw
    /// the files later.
    pub fn finish_check(&mut self, check: FileCheck) -> Result<(), String> {
        if check.finished_scans != self.finished_scans {
            return Ok(());
        }

        self.unavailable.extend(check.unavailable);

        if check.deleted.is_empty() {
            return Ok(());
        }

        self.entries
            .retain(|entry| !check.deleted.contains(&entry.path));
        self.save()
    }

    /// Notes that the game at `path` just started, adding it to the library
    /// if it wasn't known yet.
    pub fn record_played(&mut self, path: &Path, header: &CartridgeHeader) -> Result<(), String> {
        let now = seconds_since_epoch(SystemTime::now());

        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.path == path) {
            entry.last_played = Some(now);
        } else {
            let mut entry = scan::entry_for(path, header);
            entry.last_played = Some(now);
            let mut entries = std::mem::take(&mut self.entries);
            entries.push(entry);
            self.replace_entries(entries);
        }

        self.save()
    }

    /// Keeps `pixels`, the last frame of the game at `path`, as its thumbnail.
    pub fn set_thumbnail(&mut self, path: &Path, pixels: Vec<u8>) -> Result<(), String> {
        let saved = store::save_thumbnail(path, &pixels);
        self.thumbnails
            .insert(path.to_path_buf(), screen_picture(pixels));
        saved
    }

    fn replace_entries(&mut self, mut entries: Vec<LibraryEntry>) {
        entries.sort_by_cached_key(|entry| entry.name().to_lowercase());

        self.entries = entries;
    }

    fn save(&self) -> Result<(), String> {
        store::save_entries(&self.entries)
            .map_err(|error| format!("The game library couldn't be saved: {error}"))
    }
}

/// News from watching the library folders.
#[derive(Debug, Clone)]
#[cfg_attr(
    target_arch = "wasm32",
    expect(dead_code, reason = "the browser build watches no folders")
)]
pub enum WatchEvent {
    /// A ROM or folder in a library folder was added, removed or changed.
    Changed,
    /// The folders can't be watched, for this reason.
    Failed(String),
}

/// Watches the library folders for games being added or removed, and sends
/// what changed to `output` until the returned future is dropped. The
/// browser build has no folders to watch, so it has no watcher.
#[cfg(not(target_arch = "wasm32"))]
pub use watcher::watch;

/// Opens the folder picker, returning the chosen folder or `None` if the
/// user cancels.
#[cfg(not(target_arch = "wasm32"))]
pub async fn pick_folder() -> Option<PathBuf> {
    let folder = rfd::AsyncFileDialog::new()
        .set_title("Add a ROM folder")
        .pick_folder()
        .await?;
    Some(folder.path().to_path_buf())
}

/// Browsers have no folder picker, so the library has no folders.
#[cfg(target_arch = "wasm32")]
pub async fn pick_folder() -> Option<PathBuf> {
    None
}

/// Combines a scan's `scanned` games with the library's `current` ones.
/// Games keep the latest play time of either, and games started for the
/// first time while the scan ran (so not among the `checked` games it knew
/// about) are kept. Games it checked and left out are gone.
fn merge_scan(
    current: &[LibraryEntry],
    mut scanned: Vec<LibraryEntry>,
    checked: &HashSet<PathBuf>,
) -> Vec<LibraryEntry> {
    let last_played: HashMap<&Path, u64> = current
        .iter()
        .filter_map(|entry| Some((entry.path.as_path(), entry.last_played?)))
        .collect();

    for entry in &mut scanned {
        if let Some(&played) = last_played.get(entry.path.as_path()) {
            entry.last_played = entry.last_played.max(Some(played));
        }
    }

    for entry in current {
        if !checked.contains(&entry.path) && !scanned.iter().any(|new| new.path == entry.path) {
            scanned.push(entry.clone());
        }
    }

    scanned
}

/// Which of a game's pictures something is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PictureKind {
    BoxArt,
    TitleScreen,
    /// The last screen played.
    LastScreen,
}

/// A picture read from disk, decoded and ready to draw.
#[derive(Debug, Clone)]
pub struct LoadedPicture {
    path: PathBuf,
    kind: PictureKind,
    picture: Picture,
}

/// Box art just downloaded: the PNG to save, and the picture decoded on the
/// download's thread, ready to draw.
#[derive(Debug, Clone)]
pub struct DownloadedArt {
    png: Vec<u8>,
    picture: Picture,
}

impl DownloadedArt {
    /// Decodes downloaded box art, or returns `None` if it isn't a picture
    /// this app can read.
    pub fn decode(png: Vec<u8>) -> Option<DownloadedArt> {
        let (width, height, pixels) = store::decode_png(&png)?;
        Some(DownloadedArt {
            png,
            picture: Picture::new(width, height, pixels),
        })
    }
}

/// The title screen saved for the game at `rom_path` as RGBA pixels, if it
/// was made after the ROM file last changed, so it needn't be captured
/// again (for example when a folder is added back to the library).
pub fn saved_title_screen(rom_path: &Path) -> Option<Vec<u8>> {
    let png = store::current_picture(PictureKind::TitleScreen, rom_path)?;
    let (width, height, pixels) = store::decode_png(&png)?;
    (width == SCREEN_WIDTH && height == SCREEN_HEIGHT).then_some(pixels)
}

/// The box art saved for the game at `rom_path`, if it was saved after the
/// ROM file last changed, so it needn't be downloaded again.
pub fn saved_box_art(rom_path: &Path) -> Option<DownloadedArt> {
    DownloadedArt::decode(store::current_picture(PictureKind::BoxArt, rom_path)?)
}

/// Reads and decodes the picture a game's card shows; see
/// [`Library::load_pictures`].
fn best_picture(
    entry: &LibraryEntry,
    palette: DmgPalette,
    show_box_art: bool,
) -> Option<LoadedPicture> {
    let loaded = |kind, (width, height, pixels)| LoadedPicture {
        path: entry.path.clone(),
        kind,
        picture: Picture::new(width, height, pixels),
    };

    if show_box_art && let Some(picture) = store::read_picture(PictureKind::BoxArt, &entry.path) {
        return Some(loaded(PictureKind::BoxArt, picture));
    }

    if let Some((width, height, mut pixels)) =
        store::read_picture(PictureKind::TitleScreen, &entry.path)
    {
        if !entry.supports_cgb {
            palette.recolor(&mut pixels);
        }
        return Some(loaded(PictureKind::TitleScreen, (width, height, pixels)));
    }

    store::read_picture(PictureKind::LastScreen, &entry.path)
        .map(|picture| loaded(PictureKind::LastScreen, picture))
}

/// A picture of a frame of the Game Boy screen.
fn screen_picture(pixels: Vec<u8>) -> Picture {
    Picture::new(SCREEN_WIDTH, SCREEN_HEIGHT, pixels)
}

/// A time as whole seconds since 1970, or 0 for times before that.
fn seconds_since_epoch(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

mod jobs;
mod scan;
#[cfg_attr(target_arch = "wasm32", path = "library/no_store.rs")]
#[cfg_attr(not(target_arch = "wasm32"), path = "library/file_store.rs")]
mod store;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
#[cfg(not(target_arch = "wasm32"))]
mod watcher;
