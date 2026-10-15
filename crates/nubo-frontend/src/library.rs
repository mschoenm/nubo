//! The game library: the ROMs found in the folders the user added, plus the
//! games they played recently, with a picture of where they left off.
//!
//! What the library knows is kept in `library.toml` in the user's data
//! folder, so it shows up instantly when the app starts, and a scan in the
//! background brings it up to date. Each game also gets a picture of its
//! title screen (see [`title_screens`]) and of the last screen played,
//! PNG files in the `title_screens` and `thumbnails` folders next to it,
//! and, if the player wants it, box art (see [`box_art`]) in `box_art`.
//! [`Cache`] says where those files are.
//!
//! Only the desktop build (the `desktop` feature) keeps a library: the
//! browser has no folders to scan, so its library stays empty. The folder
//! watcher (`watch`) is desktop only too.

use crate::background;
use crate::model::ModelPreference;
use crate::palettes::DmgPalette;
use crate::picture::{Picture, SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::settings::LibraryFolder;
use crate::time::SystemTime;
use box_art::GameList;
use jobs::Jobs;
use nubo_core::{CartridgeHeader, Model};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub use scan::{FileCheck, ScanResult};
#[cfg(feature = "desktop")]
pub use watcher::{Watcher, Watching, watch};

/// How many title screens are captured at the same time.
const MAX_CAPTURES: usize = 2;
/// How many games have their box art looked up at the same time.
const MAX_DOWNLOADS: usize = 4;

/// Whether this build keeps a library: only the desktop build, which can
/// read folders, does.
pub const AVAILABLE: bool = cfg!(feature = "desktop");

/// A game the library knows about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryEntry {
    /// Where the ROM file is.
    pub path: PathBuf,
    /// The title from the cartridge header.
    pub title: String,
    /// Whether the game runs on a Game Boy Color (the cartridge header says
    /// so).
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
    /// Whether the game uses the Super Game Boy's colors and border (the
    /// cartridge header says so). `None` for a game read by a version that
    /// didn't keep it, until the game is played or its file changes: scans
    /// don't read unchanged files again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_sgb: Option<bool>,
    /// The model the player picked for this game in its menu, which it
    /// starts on instead of the one in the settings. Always a fixed model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelPreference>,
    /// When the library first found the game, in seconds since 1970, for
    /// listing the games added last first. `None` for a game found by a
    /// version that didn't keep it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added: Option<u64>,
    /// The file's size and modification time (in seconds since 1970) when
    /// its header was read, so a scan can tell whether to read it again.
    size: u64,
    modified: u64,
}

impl LibraryEntry {
    /// The game's name, made from its file name (see
    /// [`crate::rom::game_name`]), such as "Tetris" for
    /// `Tetris (World) (Rev 1).gb`.
    pub fn name(&self) -> String {
        crate::rom::display_name(&self.path)
    }

    /// The model the player picked for this game, if they picked one.
    pub fn model(&self) -> Option<Model> {
        match self.model {
            Some(ModelPreference::Fixed(model)) => Some(model),
            Some(ModelPreference::Automatic) | None => None,
        }
    }

    /// When the game was added to the library, for sorting: when the library
    /// found it, or for a game found before that was kept, when its file
    /// last changed.
    pub fn added(&self) -> u64 {
        self.added.unwrap_or(self.modified)
    }

    /// Keeps what the player did with `previous`, the same game as an
    /// earlier scan knew it: when they played it, the model they picked and
    /// when it was added. With `unchanged`, the file is the same too, so its
    /// pictures were already looked for.
    fn keep_from(&mut self, previous: &LibraryEntry, unchanged: bool) {
        self.last_played = previous.last_played;
        self.model = previous.model;
        self.added = previous.added;
        if unchanged {
            self.title_screen_checked = previous.title_screen_checked;
            self.box_art_checked = previous.box_art_checked;
        }
    }

    /// A game at `path` in a library folder that nothing is known about
    /// yet, for tests of an app's library screen.
    pub fn example(path: &str) -> LibraryEntry {
        LibraryEntry {
            path: PathBuf::from(path),
            title: String::new(),
            supports_cgb: false,
            in_folder: true,
            last_played: None,
            title_screen_checked: false,
            box_art_checked: false,
            supports_sgb: Some(false),
            model: None,
            added: None,
            size: 0,
            modified: 0,
        }
    }

    /// Whether the search text matches the game's name, its file name or
    /// its header title, ignoring case.
    pub fn matches(&self, search: &str) -> bool {
        let search = search.to_lowercase();
        let file_name = self.path.file_stem().unwrap_or_default().to_string_lossy();
        self.name().to_lowercase().contains(&search)
            || file_name.to_lowercase().contains(&search)
            || self.title.to_lowercase().contains(&search)
    }
}

/// Where the library keeps what it knows between runs: `library.toml` and
/// the `thumbnails`, `title_screens` and `box_art` folders of pictures, all
/// in one data folder (see `storage::files`). Builds without the `desktop`
/// feature keep nothing, and neither does a cache without a folder, such as
/// [`Cache::default`].
#[derive(Debug, Clone, Default)]
pub struct Cache {
    /// The data folder, or `None` when the system has none for us.
    #[cfg(feature = "desktop")]
    data_folder: Option<PathBuf>,
}

impl Cache {
    /// The player's own data folder, where the app keeps its library.
    pub fn for_user() -> Cache {
        Cache {
            #[cfg(feature = "desktop")]
            data_folder: crate::storage::files::project_directories()
                .map(|directories| directories.data_dir().to_path_buf()),
        }
    }

    /// The library's files in `data_folder` instead of the player's data
    /// folder, for tests and measurements that mustn't touch the player's
    /// library. The box art game lists (see `box_art`) stay in the player's
    /// data folder.
    #[cfg(feature = "desktop")]
    pub fn in_folder(data_folder: PathBuf) -> Cache {
        Cache {
            data_folder: Some(data_folder),
        }
    }

    /// The title screen of the game at `rom_path` as RGBA pixels: the one
    /// saved for it if it was made after the ROM file last changed (for
    /// example when a folder is added back to the library), else one
    /// captured by running the game (see [`title_screens::capture`]).
    /// `None` if the game showed nothing worth a picture. This is slow, so
    /// run it with [`background::run`] and hand the result to
    /// [`Library::finish_capture`].
    pub fn title_screen(&self, rom_path: &Path) -> Option<Vec<u8>> {
        if let Some(saved) = self.saved_title_screen(rom_path) {
            return Some(saved);
        }
        let rom = std::fs::read(rom_path).ok()?;
        title_screens::capture(&rom)
    }

    /// The box art of the game at `rom_path`: the one saved for it if it
    /// was saved after the ROM file last changed, else the one looked up in
    /// `games` and downloaded (see [`box_art::find`]). `None` if the
    /// collection has none. This is slow, so run it with
    /// [`background::run`] and hand the result to
    /// [`Library::finish_download`].
    pub fn box_art(
        &self,
        rom_path: &Path,
        supports_cgb: bool,
        games: &GameList,
    ) -> Result<Option<DownloadedArt>, String> {
        if let Some(saved) = self.saved_box_art(rom_path) {
            return Ok(Some(saved));
        }
        box_art::find(rom_path, supports_cgb, games).map(|png| png.and_then(DownloadedArt::decode))
    }

    /// The title screen saved for the game at `rom_path` as RGBA pixels, if
    /// it was made after the ROM file last changed, so it needn't be
    /// captured again.
    fn saved_title_screen(&self, rom_path: &Path) -> Option<Vec<u8>> {
        let png = store::current_picture(self, PictureKind::TitleScreen, rom_path)?;
        let (width, height, pixels) = store::decode_png(&png)?;
        (width == SCREEN_WIDTH && height == SCREEN_HEIGHT).then_some(pixels)
    }

    /// The box art saved for the game at `rom_path`, if it was saved after
    /// the ROM file last changed, so it needn't be downloaded again.
    fn saved_box_art(&self, rom_path: &Path) -> Option<DownloadedArt> {
        DownloadedArt::decode(store::current_picture(self, PictureKind::BoxArt, rom_path)?)
    }
}

/// The games and their thumbnails. [`Library::default`] keeps nothing
/// between runs; [`Library::load`] keeps the player's library.
#[derive(Debug, Default)]
pub struct Library {
    /// Where the library is kept between runs.
    cache: Cache,
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
    /// Reads what the library knew when the app last ran, from the player's
    /// data folder. If that can't be read, the library starts empty and the
    /// reason is returned too.
    pub fn load() -> (Library, Option<String>) {
        Library::load_from(Cache::for_user())
    }

    /// Reads what the library knew from `cache`, and keeps the library
    /// there from now on. If that can't be read, the library starts empty
    /// and the reason is returned too.
    pub fn load_from(cache: Cache) -> (Library, Option<String>) {
        let (entries, error) = match store::load_entries(&cache) {
            Ok(entries) => (entries, None),
            Err(error) => (
                Vec::new(),
                Some(format!("The game library couldn't be read: {error}")),
            ),
        };

        let mut library = Library {
            cache,
            ..Library::default()
        };
        library.replace_entries(entries);
        (library, error)
    }

    /// Where the library is kept between runs, for the slow work that
    /// reads its pictures on another thread ([`Cache::title_screen`] and
    /// [`Cache::box_art`]).
    pub fn cache(&self) -> &Cache {
        &self.cache
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

    /// The picture of the last screen played of the game at `path`, if it
    /// was read.
    pub fn thumbnail(&self, path: &Path) -> Option<&Picture> {
        self.thumbnails.get(path)
    }

    /// The title screen of the game at `path`, if it was read or captured.
    pub fn title_screen(&self, path: &Path) -> Option<&Picture> {
        self.title_screens.get(path)
    }

    /// The box art of the game at `path`, if it was read or downloaded.
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
                saved = store::save_box_art(&self.cache, path, &art.png);
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
                saved = store::save_title_screen(&self.cache, path, &pixels);
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
        let cache = self.cache.clone();

        background::run(move || {
            games
                .into_iter()
                .filter_map(|entry| best_picture(&cache, &entry, palette, show_box_art))
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

    /// Whether a scan of the library folders is running.
    pub fn is_scanning(&self) -> bool {
        self.scanning
    }

    /// Whether the library `folder` couldn't be found during the last
    /// scan.
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
        folders: Vec<LibraryFolder>,
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
        folders: Vec<LibraryFolder>,
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
            entry.supports_sgb = Some(header.supports_sgb);
        } else {
            let mut entry = scan::entry_for(path, header);
            entry.last_played = Some(now);
            entry.added = Some(now);
            let mut entries = std::mem::take(&mut self.entries);
            entries.push(entry);
            self.replace_entries(entries);
        }

        self.save()
    }

    /// Starts the game at `path` on `model` from now on, or on the model in
    /// the settings again with `None`.
    pub fn set_model(&mut self, path: &Path, model: Option<Model>) -> Result<(), String> {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.path == path) else {
            return Ok(());
        };

        entry.model = model.map(ModelPreference::Fixed);
        self.save()
    }

    /// The game at `path`, if the library knows it.
    pub fn entry(&self, path: &Path) -> Option<&LibraryEntry> {
        self.entries.iter().find(|entry| entry.path == path)
    }

    /// Takes the game at `path` off the recently played games, for a player
    /// who doesn't want it there. A game in a library folder stays among
    /// all games; one opened from elsewhere is forgotten at the next scan,
    /// which only keeps such games while they count as played.
    pub fn forget_played(&mut self, path: &Path) -> Result<(), String> {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.path == path) else {
            return Ok(());
        };

        entry.last_played = None;
        self.save()
    }

    /// Forgets the model the player picked for each game and, with
    /// `forget_played`, which games they played, for a player resetting the
    /// settings. Games opened from elsewhere than the library folders then
    /// leave at the next scan, as after [`Library::forget_played`].
    pub fn reset_games(&mut self, forget_played: bool) -> Result<(), String> {
        for entry in &mut self.entries {
            entry.model = None;
            if forget_played {
                entry.last_played = None;
            }
        }
        self.save()
    }

    /// A copy of what the library knows about its games, which
    /// [`Library::restore`] puts back, such as when the player undoes a
    /// reset of the settings.
    pub fn snapshot(&self) -> LibrarySnapshot {
        LibrarySnapshot(self.entries.clone())
    }

    /// Puts the games back as `snapshot` knew them and saves them. A scan
    /// that is running started from what the library knew before, so its
    /// result is ignored: scan again afterwards.
    pub fn restore(&mut self, snapshot: LibrarySnapshot) -> Result<(), String> {
        self.scan_generation += 1;
        self.scanning = false;
        self.replace_entries(snapshot.0);
        self.save()
    }

    /// Keeps `pixels`, the last frame of the game at `path`, as its thumbnail.
    pub fn set_thumbnail(&mut self, path: &Path, pixels: Vec<u8>) -> Result<(), String> {
        let saved = store::save_thumbnail(&self.cache, path, &pixels);
        self.thumbnails
            .insert(path.to_path_buf(), screen_picture(pixels));
        saved
    }

    fn replace_entries(&mut self, mut entries: Vec<LibraryEntry>) {
        entries.sort_by_cached_key(|entry| entry.name().to_lowercase());

        self.entries = entries;
    }

    fn save(&self) -> Result<(), String> {
        store::save_entries(&self.cache, &self.entries)
            .map_err(|error| format!("The game library couldn't be saved: {error}"))
    }
}

/// What the library knew about its games at one moment (see
/// [`Library::snapshot`]).
#[derive(Debug, Clone)]
pub struct LibrarySnapshot(Vec<LibraryEntry>);

/// News from watching the library folders (see `watch`).
#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// A ROM or folder in a library folder was added, removed or changed.
    Changed,
    /// The folders can't be watched, for this reason.
    Failed(String),
}

/// Combines a scan's `scanned` games with the library's `current` ones.
/// Games keep their play time as the library has it now, which is newer
/// than what the scan started from: games played during the scan keep it,
/// and games taken off the recently played ones stay off. Games started for
/// the first time while the scan ran (so not among the `checked` games it
/// knew about) are kept. Games it checked and left out are gone.
fn merge_scan(
    current: &[LibraryEntry],
    mut scanned: Vec<LibraryEntry>,
    checked: &HashSet<PathBuf>,
) -> Vec<LibraryEntry> {
    // The scan started from the entries as they were then; what the player
    // did since (played a game, picked its model) is in `current`.
    let known: HashMap<&Path, &LibraryEntry> = current
        .iter()
        .map(|entry| (entry.path.as_path(), entry))
        .collect();

    for entry in &mut scanned {
        if let Some(known) = known.get(entry.path.as_path()) {
            entry.last_played = known.last_played;
            entry.model = known.model;
            entry.added = known.added.or(entry.added);
            entry.supports_sgb = entry.supports_sgb.or(known.supports_sgb);
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
    /// The game's box art, downloaded (see [`box_art`]).
    BoxArt,
    /// The game's title screen, captured by running it (see
    /// [`title_screens`]).
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

/// Reads and decodes the picture a game's card shows; see
/// [`Library::load_pictures`].
fn best_picture(
    cache: &Cache,
    entry: &LibraryEntry,
    palette: DmgPalette,
    show_box_art: bool,
) -> Option<LoadedPicture> {
    let loaded = |kind, (width, height, pixels)| LoadedPicture {
        path: entry.path.clone(),
        kind,
        picture: Picture::new(width, height, pixels),
    };

    if show_box_art
        && let Some(picture) = store::read_picture(cache, PictureKind::BoxArt, &entry.path)
    {
        return Some(loaded(PictureKind::BoxArt, picture));
    }

    if let Some((width, height, mut pixels)) =
        store::read_picture(cache, PictureKind::TitleScreen, &entry.path)
    {
        if !entry.supports_cgb {
            palette.recolor(&mut pixels);
        }
        return Some(loaded(PictureKind::TitleScreen, (width, height, pixels)));
    }

    store::read_picture(cache, PictureKind::LastScreen, &entry.path)
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

pub mod box_art;
mod jobs;
mod scan;
#[cfg_attr(not(feature = "desktop"), path = "library/no_store.rs")]
#[cfg_attr(feature = "desktop", path = "library/file_store.rs")]
mod store;
#[cfg(test)]
mod tests;
pub mod title_screens;
#[cfg(feature = "desktop")]
mod watcher;
