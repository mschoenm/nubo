//! The game library: its folders, scans, pictures and box art, searching
//! it, and moving through its cards with the D-pad.

use super::playing::read_rom;
use super::{App, Message};
use crate::dialog::{DialogKind, GameMenuPage};
use crate::rom_picker;
use crate::settings::{LibraryFilter, LibrarySort};
use crate::views;
use iced::Task;
use iced::widget;
use iced::widget::scrollable::AbsoluteOffset;
use nubo_core::Model;
use nubo_frontend::background;
use nubo_frontend::controls::Action;
use nubo_frontend::library::box_art::{self, GameList};
use nubo_frontend::library::{
    self, DownloadedArt, FileCheck, LoadedPicture, ScanResult, WatchEvent,
};
use nubo_frontend::links;
use nubo_frontend::save_states::{self, Slot};
use nubo_frontend::settings::LibraryFolder;
use std::path::PathBuf;
use std::sync::Arc;

/// The height of the library's top bar, used until the page reports how
/// much of it is visible.
const LIBRARY_BAR_HEIGHT: f32 = 50.0;
/// The room kept above and below a card scrolled into view, enough for the
/// heading above the first row.
const SCROLL_MARGIN: f32 = 48.0;

/// What happens in the library: the player searching and changing its
/// folders, and the background work that fills it.
#[derive(Debug, Clone)]
pub enum LibraryMessage {
    SearchChanged(String),
    /// The library page scrolled to this offset, showing this much height.
    Scrolled(f32, f32),
    AddFolder,
    FolderPicked(Option<PathBuf>),
    RemoveFolder(usize),
    IncludeSubfolders(usize, bool),
    Rescan,
    /// The player took this game off "Continue playing".
    RemoveRecent(PathBuf),
    /// Opens the menu of this game (a right click, the "⋯" button or
    /// Select on its card).
    OpenGameMenu(PathBuf),
    /// Opens the menu of the library folder at this place in the list.
    OpenFolderMenu(usize),
    /// Opens the menu that sorts and filters the games.
    OpenSortMenu,
    SortPicked(LibrarySort),
    FilterPicked(LibraryFilter),
    /// Opens the folder this game is in.
    ShowInFolder(PathBuf),
    /// The player picked a model for this game in its menu, or `None` to
    /// follow the settings again.
    GameModelPicked(PathBuf, Option<Model>),
    /// Asks whether to delete this game's save states.
    AskDeleteSaveStates(PathBuf),
    /// Deletes this game's save states, as the player confirmed.
    DeleteSaveStates(PathBuf),
    Scanned(ScanResult),
    /// The quick startup look at whether the games' files still exist is
    /// done.
    FilesChecked(FileCheck),
    /// The folder watcher saw a change, or stopped working.
    Watch(WatchEvent),
    DownloadBoxArtToggled(bool),
    /// The player answered the question whether to download box art.
    BoxArtAnswered(bool),
    /// The player asked to try downloading box art again after it failed.
    RetryBoxArt,
    GameListLoaded(Result<Arc<GameList>, String>),
    /// A game's box art was looked up: a PNG, `None` if there is none, or
    /// why the download failed.
    BoxArtDownloaded(PathBuf, Result<Option<DownloadedArt>, String>),
    /// The saved pictures of the games were read and decoded, or why they
    /// weren't.
    PicturesLoaded(Result<Vec<LoadedPicture>, String>),
    /// A game was run to capture its title screen; `None` if it showed
    /// nothing worth keeping.
    TitleScreenCaptured(PathBuf, Option<Vec<u8>>),
}

impl From<LibraryMessage> for Message {
    fn from(message: LibraryMessage) -> Self {
        Message::Library(message)
    }
}

impl App {
    pub(super) fn update_library(&mut self, message: LibraryMessage) -> Task<Message> {
        match message {
            LibraryMessage::SearchChanged(search) => {
                self.search = search;
                self.library_selection = None;
            }
            LibraryMessage::Scrolled(offset, height) => self.library_scroll = (offset, height),
            LibraryMessage::AddFolder => {
                return Task::perform(rom_picker::pick_folder(), |path| {
                    LibraryMessage::FolderPicked(path).into()
                });
            }
            LibraryMessage::FolderPicked(Some(path)) => {
                if !self
                    .settings
                    .shared
                    .library_folders
                    .iter()
                    .any(|folder| folder.path == path)
                {
                    self.change_settings(|settings| {
                        settings.shared.library_folders.push(LibraryFolder {
                            path,
                            include_subfolders: true,
                        });
                    });
                    self.ask_about_box_art();
                    return self.rescan();
                }
            }
            LibraryMessage::FolderPicked(None) => {}
            LibraryMessage::RemoveFolder(index) => {
                if index < self.settings.shared.library_folders.len() {
                    self.change_settings(|settings| {
                        settings.shared.library_folders.remove(index);
                    });
                    return self.rescan();
                }
            }
            LibraryMessage::IncludeSubfolders(index, include) => {
                if index < self.settings.shared.library_folders.len() {
                    self.change_settings(|settings| {
                        settings.shared.library_folders[index].include_subfolders = include;
                    });
                    return self.rescan();
                }
            }
            LibraryMessage::Rescan | LibraryMessage::Watch(WatchEvent::Changed) => {
                return self.rescan();
            }
            LibraryMessage::RemoveRecent(path) => {
                if let Err(error) = self.library.forget_played(&path) {
                    self.toasts.error(error);
                }
                self.library_selection = None;
                // A game moving up into "Continue playing" wasn't watched
                // before, so its file may be gone already.
                return self.check_files();
            }
            LibraryMessage::OpenGameMenu(path) => {
                self.show_game_menu(path, GameMenuPage::Main);
            }
            LibraryMessage::OpenSortMenu => self.open_dialog(DialogKind::SortMenu),
            LibraryMessage::OpenFolderMenu(index) => {
                self.open_dialog(DialogKind::FolderMenu(index));
            }
            LibraryMessage::SortPicked(sort) => {
                self.change_settings(|settings| settings.library_sort = sort);
                self.library_selection = None;
            }
            LibraryMessage::FilterPicked(filter) => {
                self.change_settings(|settings| settings.library_filter = filter);
                self.library_selection = None;
            }
            LibraryMessage::ShowInFolder(path) => {
                if let Err(error) = links::show_folder(&path) {
                    self.toasts.error(error);
                }
            }
            LibraryMessage::GameModelPicked(path, model) => {
                if let Err(error) = self.library.set_model(&path, model) {
                    self.toasts.error(error);
                }
                self.show_game_menu(path, GameMenuPage::Main);
            }
            LibraryMessage::AskDeleteSaveStates(path) => {
                self.open_dialog(DialogKind::DeleteSaveStates(path));
            }
            LibraryMessage::DeleteSaveStates(path) => {
                let deleted = Slot::LOADABLE
                    .iter()
                    .try_for_each(|&slot| save_states::delete(&path, slot));
                match deleted {
                    Ok(()) => self.toasts.info("Save states deleted"),
                    Err(error) => self.toasts.error(error),
                }
            }
            LibraryMessage::Watch(WatchEvent::Failed(error)) => self.toasts.error(format!(
                "New games won't show up by themselves ({error}). Use \"Look for new games now\" in a folder's menu in Settings › Library instead."
            )),
            LibraryMessage::Scanned(result) => {
                if let Err(error) = self.library.finish_scan(result) {
                    self.toasts.error(error);
                }
                let rescan = if self.library.take_rescan_wanted() {
                    self.rescan()
                } else {
                    Task::none()
                };
                return Task::batch([
                    rescan,
                    self.load_pictures(true),
                    self.capture_title_screens(),
                    self.download_box_art(),
                ]);
            }
            LibraryMessage::FilesChecked(check) => {
                if let Err(error) = self.library.finish_check(check) {
                    self.toasts.error(error);
                }
                self.library_selection = None;
            }
            LibraryMessage::DownloadBoxArtToggled(enabled) => {
                self.change_settings(|settings| {
                    settings.shared.download_box_art = enabled;
                    settings.shared.box_art_asked = true;
                });
                return Task::batch([self.load_pictures(false), self.download_box_art()]);
            }
            LibraryMessage::BoxArtAnswered(download) => {
                return self.update_library(LibraryMessage::DownloadBoxArtToggled(download));
            }
            LibraryMessage::RetryBoxArt => {
                self.box_art_problem = None;
                return self.download_box_art();
            }
            LibraryMessage::GameListLoaded(result) => {
                self.loading_game_list = false;
                match result {
                    Ok(list) => {
                        self.game_list = Some(list);
                        return self.download_box_art();
                    }
                    Err(error) => self.box_art_problem = Some(error),
                }
            }
            LibraryMessage::BoxArtDownloaded(path, result) => {
                // A download that failed stops the others until the player
                // tries again; a picture that couldn't be saved is a problem
                // on this computer, which the player should hear about.
                let failed = result.as_ref().err().cloned();
                let saved = self.library.finish_download(&path, result);
                match failed {
                    Some(error) => self.box_art_problem = Some(error),
                    None => {
                        if let Err(error) = saved {
                            self.toasts.error(error);
                        }
                    }
                }
                return self.download_box_art();
            }
            LibraryMessage::PicturesLoaded(Ok(pictures)) => self.library.set_pictures(pictures),
            LibraryMessage::PicturesLoaded(Err(error)) => self
                .toasts
                .error(format!("The games' pictures couldn't be read: {error}")),
            LibraryMessage::TitleScreenCaptured(path, pixels) => {
                if let Err(error) =
                    self.library
                        .finish_capture(&path, pixels, self.settings.shared.dmg_palette)
                {
                    self.toasts.error(error);
                }
                return self.capture_title_screens();
            }
        }

        Task::none()
    }

    /// Moves through the library's cards with a direction, around the ends
    /// unless the key is `held` (see `Shelves::step`), scrolling the
    /// selected card into view, starts the selected game with A or Start,
    /// and opens its menu with Select.
    pub(super) fn navigate_library(&mut self, action: Action, held: bool) -> Task<Message> {
        let shelves = views::library::Shelves::new(&self.library, &self.settings, &self.search);
        let count = shelves.len();
        if count == 0 {
            return Task::none();
        }
        let current = self.library_selection.map(|index| index.min(count - 1));

        if matches!(action, Action::A | Action::Start) {
            return match current.and_then(|index| shelves.entry(index)) {
                Some(entry) => read_rom(entry.path.clone()),
                None => Task::none(),
            };
        }

        if action == Action::Select {
            if let Some(entry) = current.and_then(|index| shelves.entry(index)) {
                let path = entry.path.clone();
                self.show_game_menu(path, GameMenuPage::Main);
            }
            return Task::none();
        }

        if !action.is_direction() {
            return Task::none();
        }

        let next = current.map_or(0, |index| {
            shelves.step(index, action, self.window_width, !held)
        });
        let (top, bottom) = shelves.card_bounds(next, self.window_width);
        self.library_selection = Some(next);

        // Keep the card in view, with room for the heading above the first
        // row.
        let (offset, mut height) = self.library_scroll;
        if height <= 0.0 {
            height = self.window_height - LIBRARY_BAR_HEIGHT;
        }
        let target = if top - SCROLL_MARGIN < offset {
            Some(top - SCROLL_MARGIN)
        } else if bottom + SCROLL_MARGIN > offset + height {
            Some(bottom + SCROLL_MARGIN - height)
        } else {
            None
        };

        match target {
            Some(y) => widget::operation::scroll_to(
                views::library::scroll_id(),
                AbsoluteOffset {
                    x: None,
                    y: Some(y.max(0.0)),
                },
            ),
            None => Task::none(),
        }
    }

    /// Starts capturing the title screens of games that have none yet, a
    /// few at a time in the background.
    fn capture_title_screens(&mut self) -> Task<Message> {
        Task::batch(self.library.next_captures().into_iter().map(|path| {
            let rom_path = path.clone();
            let cache = self.library.cache().clone();
            Task::perform(
                background::run(move || cache.title_screen(&rom_path)),
                move |pixels| {
                    LibraryMessage::TitleScreenCaptured(path, pixels.ok().flatten()).into()
                },
            )
        }))
    }

    /// Looks up the box art of games that weren't looked up yet, a few at a
    /// time on background threads, if the player said yes to box art and
    /// nothing failed this run (see `App::box_art_problem`). The
    /// game lists are read (or downloaded) first, and only when a game needs
    /// them, so a library whose box art is all looked up touches neither the
    /// network nor the lists.
    fn download_box_art(&mut self) -> Task<Message> {
        if !box_art::AVAILABLE
            || self.fresh
            || !self.settings.shared.download_box_art
            || !self.settings.shared.box_art_asked
            || self.box_art_problem.is_some()
            || !self.library.needs_downloads()
        {
            return Task::none();
        }

        let Some(list) = self.game_list.clone() else {
            if self.loading_game_list {
                return Task::none();
            }
            self.loading_game_list = true;
            return Task::perform(
                background::run(|| box_art::load_game_list().map(Arc::new)),
                |result| LibraryMessage::GameListLoaded(result.flatten()).into(),
            );
        };

        Task::batch(
            self.library
                .next_downloads()
                .into_iter()
                .map(|(path, supports_cgb)| {
                    let list = list.clone();
                    let rom_path = path.clone();
                    let cache = self.library.cache().clone();
                    Task::perform(
                        background::run(move || cache.box_art(&rom_path, supports_cgb, &list)),
                        move |result| {
                            LibraryMessage::BoxArtDownloaded(path, result.flatten()).into()
                        },
                    )
                }),
        )
    }

    /// Whether this build keeps a library and this run may use it: not with
    /// `--fresh`, which leaves the player's library alone.
    pub(super) fn keeps_library(&self) -> bool {
        library::AVAILABLE && !self.fresh
    }

    /// Reads and decodes the games' saved pictures on a background thread.
    /// With `missing_only`, only for games that have none on screen yet,
    /// such as games a scan just found.
    pub(super) fn load_pictures(&self, missing_only: bool) -> Task<Message> {
        if !self.keeps_library() {
            return Task::none();
        }

        Task::perform(
            self.library.load_pictures(
                self.settings.shared.dmg_palette,
                self.settings.shared.download_box_art,
                missing_only,
            ),
            |pictures| LibraryMessage::PicturesLoaded(pictures).into(),
        )
    }

    /// The games "Continue playing" shows that aren't in a library folder,
    /// whose files the folder watcher watches too, so a deleted one leaves
    /// the list right away.
    pub(super) fn recent_games_elsewhere(&self) -> Vec<PathBuf> {
        if !self.keeps_library() {
            return Vec::new();
        }

        self.library
            .recently_played(views::library::RECENT_LIMIT)
            .into_iter()
            .filter(|entry| !entry.in_folder)
            .map(|entry| entry.path.clone())
            .collect()
    }

    /// Starts a quick background look at whether the games' files still
    /// exist, so games that were deleted disappear right away, before the
    /// slower scan of the folders. The app runs it when it starts, and when
    /// a game is taken off "Continue playing", which brings up one that
    /// the folder watcher didn't watch.
    pub(super) fn check_files(&self) -> Task<Message> {
        if !self.keeps_library() {
            return Task::none();
        }

        Task::perform(
            self.library
                .start_check(self.settings.shared.library_folders.clone()),
            |check| LibraryMessage::FilesChecked(check).into(),
        )
    }

    /// Starts scanning the library folders in the background. The Rescan button
    /// in the settings, the folder watcher and folder changes all come here;
    /// while a scan runs, they lead to one more scan after it instead of a
    /// second scan at the same time.
    pub(super) fn rescan(&mut self) -> Task<Message> {
        if !self.keeps_library() {
            return Task::none();
        }

        match self
            .library
            .start_scan(self.settings.shared.library_folders.clone())
        {
            Some(scan) => Task::perform(scan, |result| LibraryMessage::Scanned(result).into()),
            None => Task::none(),
        }
    }
}
