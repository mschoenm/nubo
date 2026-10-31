//! The game library: the ROMs found in the folders the user added, plus the
//! games they played recently, with a picture of where they left off.
//!
//! What the library knows is kept in `library.toml` in the user's data
//! folder, so it shows up instantly when the app starts, and a scan in the
//! background brings it up to date. Each game also gets a picture of its
//! title screen (see [`title_screens`]) and of the last screen played,
//! PNG files in the `title_screens` and `thumbnails` folders next to it,
//! and, if the player wants it, box art (see [`box_art`]) in `box_art`.
//! [`Cache`] says where those files are. A `library.toml` that can't be
//! read (such as one a newer Nubo wrote) is kept aside as
//! `library.kept-<number>.toml` before the next save can replace it.
//!
//! The desktop build (the `desktop` feature) finds its games in the player's
//! folders. The browser build (the `web` feature) has no folders to scan, so
//! the library keeps the file of every game the player adds or opens itself,
//! with everything else, in the browser's IndexedDB ([`KEEPS_GAMES`], see
//! `storage::indexed_db`). The folder watcher (`watch`) is desktop only.

use crate::background;
use crate::model::ModelPreference;
use crate::palettes::DmgPalette;
use crate::picture::{Picture, SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::settings::LibraryFolder;
#[cfg(any(feature = "desktop", feature = "web", test))]
use crate::storage::Unreadable;
use crate::time::SystemTime;
use box_art::GameList;
use jobs::Jobs;
use nubo_core::{CartridgeHeader, Model};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use scan::{FileCheck, ScanResult};
#[cfg(feature = "desktop")]
pub use watcher::{Watcher, Watching, watch};

/// How many title screens are captured at the same time.
const MAX_CAPTURES: usize = 2;
/// How many games have their box art looked up at the same time.
const MAX_DOWNLOADS: usize = 4;

/// Whether this build keeps a library: the desktop and browser builds do.
pub const AVAILABLE: bool = cfg!(any(feature = "desktop", feature = "web"));

/// Whether the library keeps the games' files itself, as the browser build
/// does, which has no folders: the player adds games (`Library::add_game`)
/// instead of folders, every game opened joins the library, and removing a
/// game deletes its file (`Library::remove_game`).
pub const KEEPS_GAMES: bool = cfg!(all(feature = "web", not(feature = "desktop")));

/// How many of a game's frames a title screen capture runs before the
/// browser gets to draw the page again (see [`Cache::title_screen`]).
#[cfg(all(feature = "web", not(feature = "desktop")))]
const CAPTURE_FRAMES_AT_ONCE: u32 = 10;

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
    /// didn't keep it, until the next scan reads it (scans read unchanged
    /// files again only for this) or the game is played.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_sgb: Option<bool>,
    /// The model the player picked for this game in its menu, which it
    /// starts on instead of the model in the settings. `None` follows the
    /// settings. Earlier versions could also pick Automatic, which picks by
    /// the cartridge even when the settings pick one model for every game;
    /// such a game keeps doing so until the player picks again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) model: Option<ModelPreference>,
    /// When the library first found the game, in seconds since 1970, for
    /// listing the games added last first. `None` for a game found by a
    /// version that didn't keep it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) added: Option<u64>,
    /// The file's size in bytes when its header was read, so a scan can tell
    /// whether to read it again.
    size: u64,
    /// The file's modification time, in seconds since 1970, when its header
    /// was read, so a scan can tell whether to read it again. In the browser,
    /// when the game was kept.
    modified: u64,
    /// The CRC-32 of the file the browser keeps, so adding a game with the
    /// same file name and title can tell the same file from a different
    /// one (see `Library::add_game`). `None` on the desktop, and for a
    /// game kept by a version that didn't keep it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    checksum: Option<u32>,
}

impl LibraryEntry {
    /// The game's name, made from its file name (see
    /// [`crate::rom::game_name`]), such as "Tetris" for
    /// `Tetris (World) (Rev 1).gb`.
    pub fn name(&self) -> String {
        crate::rom::display_name(&self.path)
    }

    /// The size of the game's file in bytes, as it was when its header was
    /// read.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// What the player picked for this game in its menu, or `None` to follow
    /// the settings. Earlier versions could also pick Automatic, which picks
    /// by the cartridge even when the settings pick one model for every game.
    pub fn preference(&self) -> Option<ModelPreference> {
        self.model
    }

    /// The model the player picked for this game, if they picked one rather
    /// than Automatic or the settings.
    pub fn model(&self) -> Option<Model> {
        match self.model {
            Some(ModelPreference::Fixed(model)) => Some(model),
            Some(ModelPreference::Automatic) | None => None,
        }
    }

    /// The model the game was made for, as its cartridge header says (see
    /// [`crate::model::made_for`]), or `None` while the library doesn't know
    /// whether it uses the Super Game Boy.
    pub fn made_for(&self) -> Option<Model> {
        if self.supports_cgb {
            Some(Model::CgbE)
        } else {
            self.supports_sgb
                .map(|supports_sgb| crate::model::made_for(false, supports_sgb))
        }
    }

    /// The model the game starts on when the settings pick `settings`: the
    /// one picked in its menu, else the one the settings pick for it. `None`
    /// while that depends on what the library doesn't know yet (see
    /// [`LibraryEntry::made_for`]).
    pub fn starts_on(&self, settings: ModelPreference) -> Option<Model> {
        match self.model.unwrap_or(settings) {
            ModelPreference::Fixed(model) => Some(model),
            ModelPreference::Automatic => self.made_for(),
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
            checksum: None,
        }
    }

    /// Whether the search text matches the game's name, its file name or
    /// its header title, ignoring case.
    pub fn matches(&self, search: &str) -> bool {
        self.matches_lowercase(&search.to_lowercase())
    }

    /// Like [`LibraryEntry::matches`], for a search text already in
    /// lowercase, so a search through the whole library lowercases it once.
    fn matches_lowercase(&self, search: &str) -> bool {
        if search.is_empty() {
            return true;
        }
        let file_name = self.path.file_stem().unwrap_or_default().to_string_lossy();
        self.name().to_lowercase().contains(search)
            || file_name.to_lowercase().contains(search)
            || self.title.to_lowercase().contains(search)
    }
}

/// Where the library keeps what it knows between runs: `library.toml` and
/// the `thumbnails`, `title_screens` and `box_art` folders of pictures, all
/// in one data folder (see `storage::files`). The browser build keeps the
/// same in IndexedDB (`storage::indexed_db`), whatever the cache. Builds with
/// neither feature keep nothing, and on the desktop neither does a cache
/// without a folder, such as [`Cache::default`].
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
            data_folder: crate::storage::files::data_folder(),
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
    /// saved for it (under `pictures_as`'s name, see
    /// [`Library::pictures_as`]) if it was made after the ROM file last
    /// changed (for example when a folder is added back to the library),
    /// else one captured by running the game (see
    /// [`title_screens::capture`]).
    /// `None` if the game showed nothing worth a picture. This is slow: on
    /// the desktop it runs on a thread of its own ([`background::run`]),
    /// and in the browser a few frames at a time, letting the page draw in
    /// between. Hand the result to [`Library::finish_capture`].
    #[cfg(not(all(feature = "web", not(feature = "desktop"))))]
    pub fn title_screen(
        &self,
        rom_path: PathBuf,
        pictures_as: PathBuf,
    ) -> impl Future<Output = Option<Vec<u8>>> + use<> {
        let cache = self.clone();
        let capture = background::run(move || {
            if let Some(saved) = cache.saved_title_screen(&rom_path, &pictures_as) {
                return Some(saved);
            }
            let rom = crate::rom::Rom::read(&rom_path).ok()?;
            title_screens::capture(&rom.bytes)
        });
        async move { capture.await.ok().flatten() }
    }

    /// See the desktop's version above.
    #[cfg(all(feature = "web", not(feature = "desktop")))]
    pub fn title_screen(
        &self,
        rom_path: PathBuf,
        pictures_as: PathBuf,
    ) -> impl Future<Output = Option<Vec<u8>>> + use<> {
        let cache = self.clone();
        async move {
            if let Some(saved) = cache.saved_title_screen(&rom_path, &pictures_as) {
                return Some(saved);
            }
            let rom = crate::rom::load(rom_path).await.ok()?;
            let mut capture = title_screens::Capture::start(&rom.bytes)?;
            loop {
                if let Some(pixels) = capture.run(CAPTURE_FRAMES_AT_ONCE) {
                    return pixels;
                }
                background::pause().await;
                while title_screens::is_held() {
                    background::pause_for(250).await;
                }
            }
        }
    }

    /// The box art of the game at `rom_path`: the one saved for it (under
    /// `pictures_as`'s name, see [`Library::pictures_as`]) if it was saved
    /// after the ROM file last changed, else the one looked up in
    /// `games` and downloaded (see [`box_art::find`]). `None` if the
    /// collection has none. This is slow: on the desktop it runs on a thread
    /// of its own ([`background::run`]). Hand the result to
    /// [`Library::finish_download`].
    pub fn box_art(
        &self,
        rom_path: PathBuf,
        pictures_as: PathBuf,
        supports_cgb: bool,
        games: Arc<GameList>,
    ) -> impl Future<Output = Result<Option<DownloadedArt>, String>> + use<> {
        let cache = self.clone();
        #[cfg(not(all(feature = "web", not(feature = "desktop"))))]
        {
            let work = background::run(move || {
                if let Some(saved) = cache.saved_box_art(&rom_path, &pictures_as) {
                    return Ok(Some(saved));
                }
                box_art::find(&rom_path, supports_cgb, &games)
                    .map(|png| png.and_then(DownloadedArt::decode))
            });
            async move { work.await.and_then(|result| result) }
        }
        #[cfg(all(feature = "web", not(feature = "desktop")))]
        async move {
            if let Some(saved) = cache.saved_box_art(&rom_path, &pictures_as) {
                return Ok(Some(saved));
            }
            box_art::find(&rom_path, supports_cgb, &games)
                .await
                .map(|png| png.and_then(DownloadedArt::decode))
        }
    }

    /// The title screen saved for the game at `rom_path` under
    /// `pictures_as`'s name as RGBA pixels, if it was made after the ROM
    /// file last changed, so it needn't be captured again.
    fn saved_title_screen(&self, rom_path: &Path, pictures_as: &Path) -> Option<Vec<u8>> {
        let png = store::current_picture(self, PictureKind::TitleScreen, pictures_as, rom_path)?;
        let (width, height, pixels) = store::decode_png(&png)?;
        (width == SCREEN_WIDTH && height == SCREEN_HEIGHT).then_some(pixels)
    }

    /// The box art saved for the game at `rom_path` under `pictures_as`'s
    /// name, if it was saved after the ROM file last changed, so it needn't
    /// be downloaded again.
    fn saved_box_art(&self, rom_path: &Path, pictures_as: &Path) -> Option<DownloadedArt> {
        DownloadedArt::decode(store::current_picture(
            self,
            PictureKind::BoxArt,
            pictures_as,
            rom_path,
        )?)
    }
}

/// The games and their thumbnails. On the desktop [`Library::default`]
/// keeps nothing between runs; [`Library::load`] keeps the player's library
/// (in the browser, both keep it in IndexedDB).
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
    /// Whether another scan was asked for (with "Look for new games now", by
    /// the folder watcher or by changing the folders) while one was running.
    /// Only one scan runs at a time; this one starts when it is done.
    rescan_wanted: bool,
    /// Counts the scans whose result was taken in, so a quick file check
    /// that started before a scan finished doesn't undo what it found.
    finished_scans: u64,
    /// Whether the saved list of games couldn't be read and is still in
    /// place (it couldn't be kept aside), so saving would write over it.
    /// The list is then not saved this run.
    read_only: bool,
}

impl Library {
    /// Reads what the library knew when the app last ran, from the player's
    /// data folder (in the browser, from IndexedDB). If that can't be read,
    /// the library starts empty and the reason is returned too.
    pub fn load() -> (Library, Option<String>) {
        Library::load_from(Cache::for_user())
    }

    /// Reads what the library knew from `cache`, and keeps the library
    /// there from now on. If that can't be read, the library starts empty
    /// and the reason is returned too; a `library.toml` that can't be parsed
    /// is kept aside first, so the next save can't lose it. One that can't
    /// be read at all, or can't be kept aside, is never written over: the
    /// library then works as usual but saves no list of games this run.
    pub fn load_from(cache: Cache) -> (Library, Option<String>) {
        let (entries, error, read_only) = match store::load_entries(&cache) {
            Ok(entries) => (entries, None, false),
            Err(unreadable) => (
                Vec::new(),
                Some(unreadable.problem),
                !unreadable.saving_is_safe,
            ),
        };

        let mut library = Library {
            cache,
            read_only,
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
        let search = search.to_lowercase();
        self.entries.iter().filter(move |entry| {
            entry.in_folder
                && entry.matches_lowercase(&search)
                && !self.unavailable.contains(&entry.path)
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

    /// Whether any game shows box art.
    pub fn has_box_art(&self) -> bool {
        !self.box_art.is_empty()
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
        let pictures_as = self.pictures_as(path);

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
                saved = store::save_box_art(&self.cache, &pictures_as, &art.png);
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
        let pictures_as = self.pictures_as(path);

        let mut saved = Ok(());
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.path == path) {
            entry.title_screen_checked = true;

            if let Some(mut pixels) = pixels {
                saved = store::save_title_screen(&self.cache, &pictures_as, &pixels);
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
        let games: Vec<(LibraryEntry, PathBuf)> = self
            .entries
            .iter()
            .filter(|entry| !missing_only || !self.has_picture(&entry.path))
            .map(|entry| (entry.clone(), self.pictures_as(&entry.path)))
            .collect();
        let cache = self.cache.clone();

        background::run(move || {
            games
                .into_iter()
                .filter_map(|(entry, pictures_as)| {
                    best_picture(&cache, &entry, &pictures_as, palette, show_box_art)
                })
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

    /// Whether the library is still finding its games or their title
    /// screens: a scan runs, or a game hasn't been run for its title screen
    /// yet. Box art isn't counted, since a download may never finish.
    pub fn is_busy(&self) -> bool {
        self.scanning
            || !self.captures.is_idle()
            || self.entries.iter().any(|entry| !entry.title_screen_checked)
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
        let scan = background::run(move || {
            if KEEPS_GAMES {
                scan::kept_games(generation, known, keeps_file)
            } else {
                scan::scan(generation, &folders, known)
            }
        });
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
        let entries = if KEEPS_GAMES {
            Vec::new()
        } else {
            self.entries.clone()
        };
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

    /// Whether any game counts as played, those on a drive that is gone too,
    /// which "Continue playing" leaves out until it is back.
    pub fn any_played(&self) -> bool {
        self.entries.iter().any(|entry| entry.last_played.is_some())
    }

    /// Notes that the game at `path` just started, adding it to the library
    /// if it wasn't known yet.
    pub fn record_played(&mut self, path: &Path, header: &CartridgeHeader) -> Result<(), String> {
        self.record_played_at(path, header, SystemTime::now())
    }

    /// Notes that the game at `path` was played at `time`, adding it to the
    /// library if it wasn't known yet. Scenes of the library list their
    /// played games this way, each as long ago as the scene says. A game
    /// whose path isn't valid UTF-8 stays out, as scans leave it out: the
    /// library file couldn't store it, and no save of it would work again.
    pub fn record_played_at(
        &mut self,
        path: &Path,
        header: &CartridgeHeader,
        time: SystemTime,
    ) -> Result<(), String> {
        if path.to_str().is_none() {
            return Ok(());
        }
        let now = seconds_since_epoch(time);

        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.path == path) {
            entry.last_played = Some(now);
            entry.supports_sgb = Some(header.supports_sgb);
        } else {
            let mut entry = scan::entry_for(path, header);
            entry.in_folder = KEEPS_GAMES;
            entry.last_played = Some(now);
            entry.added = Some(now);
            let mut entries = std::mem::take(&mut self.entries);
            entries.push(entry);
            self.replace_entries(entries);
        }

        self.save()
    }

    /// Starts the game at `path` as `preference` picks from now on, or on
    /// the model in the settings again with `None`.
    pub fn set_model(
        &mut self,
        path: &Path,
        preference: Option<ModelPreference>,
    ) -> Result<(), String> {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.path == path) else {
            return Ok(());
        };

        entry.model = preference;
        self.save()
    }

    /// The game at `path`, if the library knows it.
    pub fn entry(&self, path: &Path) -> Option<&LibraryEntry> {
        self.entries.iter().find(|entry| entry.path == path)
    }

    /// The path the pictures of the game at `path` are named after. On the
    /// desktop that is the name the game owns in the data folder (see
    /// `storage::names`); a game that hasn't started yet and shares its file
    /// name with another game in the library doesn't take a plain name
    /// nobody owns, so the two don't show each other's pictures. The browser
    /// keeps each game under a name of its own already, so it is `path`.
    pub fn pictures_as(&self, path: &Path) -> PathBuf {
        #[cfg(feature = "desktop")]
        if let Some(folder) = &self.cache.data_folder {
            let shared = self
                .entries
                .iter()
                .filter(|entry| entry.path.file_name() == path.file_name())
                .take(2)
                .any(|entry| entry.path != path);
            return crate::storage::names::picture_path(folder, path, shared);
        }
        path.to_path_buf()
    }

    /// Claims the names the data of the game at `path` goes by, as it
    /// starts, now that its `header` reads: its battery saves, save states,
    /// cheats and pictures (see `storage::names`). Call it before the game
    /// reads any of them. A name whose owner is on one of `folders` that
    /// can't be reached now (an unplugged drive) is never handed to another
    /// game. When the game's pictures go by another name now, it gets them
    /// again from there, and the ones under a plain name nobody owned,
    /// which may show another game of that file name, are made again.
    /// Returns why the record of names couldn't be read or saved; the names
    /// still hold for this run. The browser keeps each game under a name of
    /// its own already (see `Library::add_game`), so it claims nothing.
    pub fn claim_names(
        &mut self,
        path: &Path,
        header: &CartridgeHeader,
        folders: &[LibraryFolder],
    ) -> Result<(), String> {
        #[cfg(feature = "desktop")]
        if let Some(data_folder) = self.cache.data_folder.clone() {
            let before = self.pictures_as(path);
            let claim = crate::storage::names::claim(&data_folder, path, &header.title, |owner| {
                scan::is_on_unreachable_folder(owner, folders)
            });
            if claim.stored != before {
                if claim.how == crate::storage::names::How::Plain {
                    store::delete_pictures(&self.cache, path);
                }
                for pictures in [
                    &mut self.box_art,
                    &mut self.title_screens,
                    &mut self.thumbnails,
                ] {
                    pictures.remove(path);
                }
                if let Some(entry) = self.entries.iter_mut().find(|entry| entry.path == path) {
                    entry.title_screen_checked = false;
                    entry.box_art_checked = false;
                }
            }
            return claim.problem.map_or(Ok(()), Err);
        }
        let _ = (path, header, folders);
        Ok(())
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

    /// Forgets, with `forget_models`, the model the player picked for each
    /// game and, with `forget_played`, which games they played, for a
    /// player starting over. Games opened from elsewhere than the library
    /// folders then leave at the next scan, as after
    /// [`Library::forget_played`].
    pub fn reset_games(&mut self, forget_models: bool, forget_played: bool) -> Result<(), String> {
        for entry in &mut self.entries {
            if forget_models {
                entry.model = None;
            }
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

    /// Keeps the file of `rom` in the library, as the browser build does
    /// for every game the player adds or opens (see [`KEEPS_GAMES`]), and
    /// lists it among all games. Returns the path it is kept under, which
    /// its saves, save states and cheats are named after, or that the player
    /// must answer `same_title`'s question first.
    ///
    /// The same file kept before, under any of its names, is kept again
    /// there. A game kept before under the same file name with another
    /// header title is another game, so `rom` is kept beside it under
    /// `path_beside`'s name and never takes its saves. One with the same
    /// title but other contents may be a newer version of it or another
    /// game with a title alike (homebrew games often have an empty or
    /// generic one), which only the player can tell: `same_title` says
    /// which, or to ask. A newer version replaces it and its saves carry
    /// over; another game is kept beside it under a name with the CRC-32 of
    /// its contents in it. A game kept by a version that didn't keep
    /// checksums is taken as the same file when its size is the same, as
    /// before. Pictures go unless the file is the same, so they are made
    /// again. Fails, keeping nothing, if `rom` isn't a game the emulator can
    /// start: a zip file is unpacked whole to check it
    /// ([`nubo_core::Emulator::check_rom`]), so one damaged past the header
    /// isn't kept either.
    #[cfg(all(feature = "web", not(feature = "desktop")))]
    pub fn add_game(
        &mut self,
        rom: &crate::rom::Rom,
        same_title: SameTitle,
    ) -> Result<Kept, String> {
        let header = nubo_core::Emulator::check_rom(&rom.bytes)
            .map_err(|error| format!("{} isn't a game Nubo can play: {error}", rom.name()))?;
        let checksum = crc32fast::hash(&rom.bytes);
        let Some(path) = self.place_for(
            &rom.path,
            &header.title,
            checksum,
            rom.bytes.len() as u64,
            same_title,
        ) else {
            return Ok(Kept::Ask);
        };
        let keep_error = |error| format!("{} couldn't be kept in the browser: {error}", rom.name());
        store::save_rom(&path, &rom.bytes).map_err(keep_error)?;

        let now = seconds_since_epoch(SystemTime::now());
        let mut entry = scan::entry_for(&path, &header);
        entry.in_folder = true;
        entry.size = rom.bytes.len() as u64;
        entry.modified = now;
        entry.added = Some(now);
        entry.checksum = Some(checksum);

        let previous = self.entries.iter().position(|known| known.path == path);
        if let Some(index) = previous {
            let previous = &self.entries[index];
            let unchanged = previous.size == entry.size
                && previous.title == entry.title
                && previous.checksum.is_none_or(|known| known == checksum);
            entry.keep_from(previous, unchanged);
            if !unchanged {
                store::delete_pictures(&path).map_err(keep_error)?;
                for pictures in [
                    &mut self.box_art,
                    &mut self.title_screens,
                    &mut self.thumbnails,
                ] {
                    pictures.remove(&path);
                }
            }
        }
        let mut entries = std::mem::take(&mut self.entries);
        if let Some(index) = previous {
            entries.remove(index);
        }
        entries.push(entry);
        self.replace_entries(entries);
        self.save()?;
        Ok(Kept::As(path))
    }

    /// Where the browser keeps a game picked as `path` with the header
    /// `title`, whose file has the CRC-32 `checksum` and is `size` bytes
    /// long, as `Library::add_game` explains; `None` when `same_title`
    /// says to ask the player first.
    #[cfg(any(test, all(feature = "web", not(feature = "desktop"))))]
    fn place_for(
        &self,
        path: &Path,
        title: &str,
        checksum: u32,
        size: u64,
        same_title: SameTitle,
    ) -> Option<PathBuf> {
        if let Some(same) = self.entries.iter().find(|entry| {
            entry.checksum == Some(checksum)
                && entry.title == title
                && is_kept_as(&entry.path, path)
        }) {
            return Some(same.path.clone());
        }

        let beside = path_beside(path, title);
        let version_of = [path, beside.as_path()]
            .into_iter()
            .find_map(|kept| self.entry(kept).filter(|entry| entry.title == title));
        match version_of {
            None if self.entry(path).is_none() => Some(path.to_path_buf()),
            None => Some(beside),
            Some(known) if known.checksum.is_none() && known.size == size => {
                Some(known.path.clone())
            }
            Some(known) => match same_title {
                SameTitle::Ask => None,
                SameTitle::NewVersion => Some(known.path.clone()),
                SameTitle::AnotherGame => Some(crate::storage::name_with_hash(path, checksum)),
            },
        }
    }

    /// Whether the library keeps the file of the game at `path` (see
    /// `Library::add_game`).
    pub fn keeps_game(&self, path: &Path) -> bool {
        KEEPS_GAMES && self.entry(path).is_some_and(|entry| entry.in_folder)
    }

    /// Whether the library keeps the game at `path` and its file is really
    /// there: an entry can outlive a file the browser refused to keep.
    pub fn keeps_file_of(&self, path: &Path) -> bool {
        self.keeps_game(path) && keeps_file(path)
    }

    /// Takes the game at `path` out of the library, deleting its kept file
    /// and pictures, as the browser build does when the player removes a
    /// game. Its battery saves and save states stay, so adding the game
    /// again brings them back.
    #[cfg(all(feature = "web", not(feature = "desktop")))]
    pub fn remove_game(&mut self, path: &Path) -> Result<(), String> {
        self.forget_game(path).and(self.save())
    }

    /// Takes every game out of the library, as [`Library::remove_game`]
    /// does, for a player starting over in the browser. The library is saved
    /// once, at the end.
    #[cfg(all(feature = "web", not(feature = "desktop")))]
    pub fn remove_all_games(&mut self) -> Result<(), String> {
        let paths: Vec<PathBuf> = self
            .entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        paths
            .iter()
            .map(|path| self.forget_game(path))
            .fold(Ok(()), Result::and)
            .and(self.save())
    }

    /// Deletes the kept file and pictures of the game at `path` and drops it
    /// from the library, without saving the library.
    #[cfg(all(feature = "web", not(feature = "desktop")))]
    fn forget_game(&mut self, path: &Path) -> Result<(), String> {
        let removed = store::delete_rom(path).and_then(|()| store::delete_pictures(path));
        self.entries.retain(|entry| entry.path != path);
        for pictures in [
            &mut self.box_art,
            &mut self.title_screens,
            &mut self.thumbnails,
        ] {
            pictures.remove(path);
        }
        removed.map_err(|error| format!("The game couldn't be removed: {error}"))
    }

    /// How many games the library keeps the files of, and about how many
    /// bytes they, the library's pictures and the box art game lists take,
    /// for the browser build's settings.
    #[cfg(all(feature = "web", not(feature = "desktop")))]
    pub fn kept_size(&self) -> (usize, u64) {
        let count = self.entries.iter().filter(|entry| entry.in_folder).count();
        (count, store::size(&self.entries))
    }

    /// Keeps `pixels`, the last frame of the game at `path`, as its thumbnail.
    pub fn set_thumbnail(&mut self, path: &Path, pixels: Vec<u8>) -> Result<(), String> {
        // A game taken out of a library that keeps its games leaves nothing
        // behind.
        if KEEPS_GAMES && self.entry(path).is_none() {
            return Ok(());
        }
        let saved = store::save_thumbnail(&self.cache, &self.pictures_as(path), &pixels);
        self.thumbnails
            .insert(path.to_path_buf(), screen_picture(pixels));
        saved
    }

    /// Makes `entries` the library's games, sorted by name as the library
    /// lists them.
    fn replace_entries(&mut self, mut entries: Vec<LibraryEntry>) {
        entries.sort_by_cached_key(|entry| entry.name().to_lowercase());

        self.entries = entries;
    }

    /// Saves the list of games, unless that would write over a saved one
    /// that couldn't be read (see [`Library::load_from`]).
    fn save(&self) -> Result<(), String> {
        if self.read_only {
            return Ok(());
        }
        store::save_entries(&self.cache, &self.entries)
            .map_err(|error| format!("The game library couldn't be saved: {error}"))
    }
}

/// What the library knew about its games at one moment (see
/// [`Library::snapshot`]).
#[derive(Debug, Clone)]
pub struct LibrarySnapshot(Vec<LibraryEntry>);

/// What `Library::add_game` does with a game whose file name and header
/// title are those of a game the browser keeps already, but whose contents
/// differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameTitle {
    /// Keeps nothing yet and says the player must answer first
    /// ([`Kept::Ask`]).
    Ask,
    /// It is a newer version of the kept game: it replaces that one, and
    /// the saves carry over.
    NewVersion,
    /// It is another game whose title happens to be the same: it is kept
    /// beside the other one, with saves of its own.
    AnotherGame,
}

/// What `Library::add_game` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kept {
    /// It kept the game under this path, which its saves go by.
    As(PathBuf),
    /// It kept nothing: the browser keeps another file with the same file
    /// name and title, and the player must say whether this one is a new
    /// version of that game or another game ([`SameTitle`]).
    Ask,
}

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

    let scanned_paths: HashSet<PathBuf> = scanned.iter().map(|entry| entry.path.clone()).collect();
    for entry in current {
        if !checked.contains(&entry.path) && !scanned_paths.contains(&entry.path) {
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
    pictures_as: &Path,
    palette: DmgPalette,
    show_box_art: bool,
) -> Option<LoadedPicture> {
    let loaded = |kind, (width, height, pixels)| LoadedPicture {
        path: entry.path.clone(),
        kind,
        picture: Picture::new(width, height, pixels),
    };

    if show_box_art
        && let Some(picture) = store::read_picture(cache, PictureKind::BoxArt, pictures_as)
    {
        return Some(loaded(PictureKind::BoxArt, picture));
    }

    if let Some((width, height, mut pixels)) =
        store::read_picture(cache, PictureKind::TitleScreen, pictures_as)
    {
        if !entry.supports_cgb {
            palette.recolor(&mut pixels);
        }
        return Some(loaded(PictureKind::TitleScreen, (width, height, pixels)));
    }

    store::read_picture(cache, PictureKind::LastScreen, pictures_as)
        .map(|picture| loaded(PictureKind::LastScreen, picture))
}

/// Whether the library keeps the file of the game at `path` (see
/// [`KEEPS_GAMES`]).
#[cfg(all(feature = "web", not(feature = "desktop")))]
fn keeps_file(path: &Path) -> bool {
    store::has_rom(path)
}

/// Libraries that find their games in folders keep no files.
#[cfg(not(all(feature = "web", not(feature = "desktop"))))]
fn keeps_file(_path: &Path) -> bool {
    true
}

/// Whether the browser keeps a game picked as `path` under `kept`: the
/// same name, or the same with a hash in brackets after the stem, as
/// [`path_beside`] and [`SameTitle::AnotherGame`] name them.
#[cfg(any(test, all(feature = "web", not(feature = "desktop"))))]
fn is_kept_as(kept: &Path, path: &Path) -> bool {
    if kept == path {
        return true;
    }
    let hash = kept
        .file_stem()
        .and_then(|stem| stem.to_str())
        .zip(path.file_stem().and_then(|stem| stem.to_str()))
        .and_then(|(kept, stem)| kept.strip_prefix(stem))
        .and_then(|rest| rest.strip_prefix(" ("))
        .and_then(|rest| rest.strip_suffix(')'));
    kept.parent() == path.parent()
        && kept.extension() == path.extension()
        && hash.is_some_and(|hash| {
            hash.len() == 8 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

/// Where the browser keeps a game whose file name `path` already belongs to
/// another game, one with another header title: `game.gb` becomes
/// `game (1a2b3c4d).gb`, with a hash of `title` in brackets, which the
/// game's shown name leaves out (see `rom::game_name`). The same title always
/// gives the same name, so a newer version of that game replaces it, and a
/// game added again after it was removed gets its saves back. Players' saves
/// are named after it, so the name must never change: the hash is 32-bit
/// FNV-1a of the title's UTF-8 bytes, in lowercase hex.
#[cfg(any(test, all(feature = "web", not(feature = "desktop"))))]
fn path_beside(path: &Path, title: &str) -> PathBuf {
    crate::storage::name_with_hash(path, crate::storage::fnv1a(title.as_bytes()))
}

/// A picture of a frame of the Game Boy screen.
fn screen_picture(pixels: Vec<u8>) -> Picture {
    Picture::new(SCREEN_WIDTH, SCREEN_HEIGHT, pixels)
}

/// What to tell the player when `library.toml` couldn't be used because of
/// `error`, and whether saving over it is safe: `kept` says where it was
/// kept aside or why it couldn't be, and is `None` when it couldn't be read
/// at all (so it wasn't tried).
#[cfg(any(feature = "desktop", feature = "web", test))]
fn unreadable_library(error: &str, kept: Option<Result<String, String>>) -> Unreadable {
    let saving_is_safe = matches!(kept, Some(Ok(_)));
    let problem = match kept {
        Some(Ok(location)) => format!(
            "The game library couldn't be read, so Nubo starts a new one and kept the old one as {location}: {error}"
        ),
        Some(Err(keep_error)) => format!(
            "The game library couldn't be read, and keeping it aside failed too ({keep_error}), so Nubo leaves it as it is and saves no changes to it this time: {error}"
        ),
        None => format!(
            "The game library couldn't be read, so Nubo leaves it as it is and saves no changes to it this time: {error}"
        ),
    };
    Unreadable {
        problem,
        saving_is_safe,
    }
}

/// A time as whole seconds since 1970, or 0 for times before that.
fn seconds_since_epoch(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

pub mod box_art;
#[cfg(any(test, all(feature = "web", not(feature = "desktop"))))]
mod browser_store;
mod jobs;
mod scan;
#[cfg(all(feature = "web", not(feature = "desktop")))]
use browser_store as store;
#[cfg(feature = "desktop")]
#[path = "library/file_store.rs"]
mod store;
#[cfg(not(any(feature = "desktop", feature = "web")))]
#[path = "library/no_store.rs"]
mod store;
#[cfg(test)]
mod tests;
pub mod title_screens;
#[cfg(feature = "desktop")]
mod watcher;
