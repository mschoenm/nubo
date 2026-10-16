//! The dialogs over the current screen (see `crate::dialog`): what each one
//! says and offers, picking a choice, and moving between the choices with
//! the D-pad.

use super::{App, GameMessage, LibraryMessage, Message, SettingsMessage, scroll_in_proportion};
use crate::dialog::{
    Choice, ChoiceStyle, Dialog, DialogAction, DialogContent, DialogKind, GameMenuPage, Layout,
    Notice, ResetChoices,
};
use crate::selection;
use crate::settings::{LibraryFilter, LibrarySort, Settings};
use crate::ui::icons::Icon;
use crate::views;
use iced::Task;
use nubo_frontend::controls::Action;
use nubo_frontend::library::{self, LibraryEntry, box_art};
use nubo_frontend::links;
use nubo_frontend::model::{self, Console, ModelPreference};
use nubo_frontend::save_states;
use nubo_frontend::settings::LibraryFolder;
use nubo_frontend::updates;
use std::path::{Path, PathBuf};

/// What happens to the open dialog.
#[derive(Debug, Clone)]
pub enum DialogMessage {
    /// The player picked a choice or dismissed the dialog, which closes it;
    /// then the message runs, if there is one.
    Closed(Option<Box<Message>>),
    /// Shows another page of a library game's menu.
    ShowGameMenu(PathBuf, GameMenuPage),
    /// The player ticked or unticked a box of the question whether to reset
    /// the settings, leaving these ticked.
    ResetChoicesChanged(ResetChoices),
}

impl From<DialogMessage> for Message {
    fn from(message: DialogMessage) -> Self {
        Message::Dialog(message)
    }
}

impl DialogAction {
    /// The message that does this action.
    pub fn into_message(self) -> Message {
        match self {
            DialogAction::Close => DialogMessage::Closed(None).into(),
            DialogAction::CloseThen(message) => {
                DialogMessage::Closed(Some(Box::new(message))).into()
            }
            DialogAction::Stay(message) => message,
        }
    }
}

impl App {
    /// Handles a message about the open dialog: closing it after a choice,
    /// turning to another page of a game's menu, or ticking a box.
    pub(super) fn update_dialog(&mut self, message: DialogMessage) -> Task<Message> {
        match message {
            DialogMessage::Closed(message) => {
                self.dialog = None;
                if let Some(message) = message {
                    return self.update(*message);
                }
            }
            DialogMessage::ShowGameMenu(path, page) => self.show_game_menu(path, page),
            DialogMessage::ResetChoicesChanged(choices) => {
                if let Some(dialog) = &mut self.dialog
                    && matches!(dialog.kind, DialogKind::ResetSettings(_))
                {
                    dialog.kind = DialogKind::ResetSettings(choices);
                }
            }
        }
        Task::none()
    }

    /// Asks whether to reset every setting, with nothing else ticked and
    /// the D-pad on Cancel, so a stray press of A deletes nothing, scrolled
    /// to Cancel in a window too short for the whole question.
    pub(super) fn ask_reset(&mut self) -> Task<Message> {
        if self.dialog.is_some() {
            return Task::none();
        }
        self.dialog = Some(Dialog::new(DialogKind::ResetSettings(
            ResetChoices::default(),
        )));
        let cancel = self
            .dialog_content()
            .and_then(|content| content.choices.iter().position(|choice| !choice.checkbox));
        if let (Some(dialog), Some(cancel)) = (&mut self.dialog, cancel) {
            dialog.selection = cancel;
        }
        self.scroll_to_selection()
    }

    /// Scrolls a question so the choice the D-pad is on is in view, in a
    /// window too short for all of it. A menu scrolls its rows by itself.
    fn scroll_to_selection(&self) -> Task<Message> {
        match (&self.dialog, self.dialog_content()) {
            (Some(dialog), Some(content)) if content.layout == Layout::Buttons => {
                scroll_in_proportion(
                    views::dialog::scroll_id(),
                    dialog.selection,
                    content.choices.len(),
                )
            }
            _ => Task::none(),
        }
    }

    /// What the reset can do besides the settings in this build and this
    /// run. Saves can't be deleted while a game is open, which would write
    /// its own again, so their boxes are greyed out then, and nothing kept
    /// between runs is touched with `--fresh`.
    pub(super) fn reset_offers(&self) -> ResetOffers {
        let keeps_data = !self.fresh;
        ResetOffers {
            delete_battery_saves: keeps_data,
            delete_save_states: keeps_data && save_states::AVAILABLE,
            remove_folders: keeps_data
                && if library::KEEPS_GAMES {
                    self.library.games("").next().is_some()
                } else {
                    library::AVAILABLE && !self.settings.shared.library_folders.is_empty()
                },
            clear_recently_played: keeps_data && library::AVAILABLE && self.library.any_played(),
            game_open: self.game.is_some(),
        }
    }

    /// Opens (or turns to) `page` of the menu of the game at `path`, with the
    /// D-pad on the choice in use, or on the recommended revision on a page
    /// of revisions.
    pub(super) fn show_game_menu(&mut self, path: PathBuf, page: GameMenuPage) {
        self.dialog = Some(Dialog::new(DialogKind::GameMenu(path, page)));
        let recommended = match page {
            GameMenuPage::Revisions(console) => console
                .revisions()
                .iter()
                .position(|&model| model == console.recommended()),
            GameMenuPage::Main | GameMenuPage::Consoles => None,
        };
        let checked = self
            .dialog_content()
            .and_then(|content| content.choices.iter().position(|choice| choice.checked))
            .or(recommended);
        if let (Some(dialog), Some(checked)) = (&mut self.dialog, checked) {
            dialog.selection = checked;
        }
    }

    /// What the open dialog shows, or `None` while none is open.
    pub(crate) fn dialog_content(&self) -> Option<DialogContent> {
        let dialog = self.dialog.as_ref()?;
        match &dialog.kind {
            DialogKind::BoxArt => Some(box_art_question()),
            DialogKind::Updates => Some(updates_question()),
            DialogKind::SortMenu => Some(sort_menu(&self.settings)),
            DialogKind::FolderMenu(index) => {
                let folder = self.settings.shared.library_folders.get(*index)?;
                Some(folder_menu(*index, folder))
            }
            DialogKind::GameMenu(path, page) => {
                let entry = self.library.entry(path)?;
                Some(match *page {
                    GameMenuPage::Main => self.game_menu(entry),
                    GameMenuPage::Consoles => self.consoles_page(entry),
                    GameMenuPage::Revisions(console) => revisions_page(entry, console),
                })
            }
            DialogKind::DeleteSaveStates(path) => {
                Some(delete_question(path, self.library.entry(path)))
            }
            DialogKind::RemoveGame(path) => Some(remove_question(path, self.library.entry(path))),
            DialogKind::ResetSettings(choices) => {
                Some(reset_question(*choices, self.reset_offers()))
            }
        }
    }

    /// The first page of a library game's menu.
    fn game_menu(&self, entry: &LibraryEntry) -> DialogContent {
        let path = &entry.path;
        let mut choices = vec![
            Choice::new(
                Icon::Play,
                "Play",
                ChoiceStyle::Secondary,
                DialogAction::CloseThen(GameMessage::PlayGame(path.clone()).into()),
            ),
            Choice::new(
                Icon::Cpu,
                "Play on…",
                ChoiceStyle::Secondary,
                DialogAction::Stay(
                    DialogMessage::ShowGameMenu(path.clone(), GameMenuPage::Consoles).into(),
                ),
            )
            .hint(match entry.model() {
                Some(model) => String::from(model.name()),
                None => String::from("As in the settings"),
            }),
        ];
        if links::CAN_SHOW_FOLDERS {
            choices.push(Choice::new(
                Icon::FolderOpen,
                "Show in folder",
                ChoiceStyle::Secondary,
                DialogAction::CloseThen(LibraryMessage::ShowInFolder(path.clone()).into()),
            ));
        }
        if entry.last_played.is_some() {
            choices.push(Choice::new(
                Icon::X,
                "Remove from Continue playing",
                ChoiceStyle::Secondary,
                DialogAction::CloseThen(LibraryMessage::RemoveRecent(path.clone()).into()),
            ));
        }
        if save_states::AVAILABLE && !self.fresh && save_states::any_saved(path) {
            choices.push(Choice::new(
                Icon::Trash2,
                "Delete save states…",
                ChoiceStyle::Danger,
                DialogAction::CloseThen(LibraryMessage::AskDeleteSaveStates(path.clone()).into()),
            ));
        }
        if self.library.keeps_game(path) && !self.fresh {
            choices.push(Choice::new(
                Icon::Trash2,
                "Remove from library…",
                ChoiceStyle::Danger,
                DialogAction::CloseThen(LibraryMessage::AskRemoveGame(path.clone()).into()),
            ));
        }

        DialogContent {
            title: entry.name(),
            text: None,
            notice: None,
            layout: Layout::List,
            choices,
            dismiss: DialogAction::Close,
            click_outside_dismisses: true,
        }
    }

    /// The page of a game's menu that picks the console it starts on.
    fn consoles_page(&self, entry: &LibraryEntry) -> DialogContent {
        let path = &entry.path;
        let picked = entry.model();
        let settings_model = match self.settings.shared.model {
            ModelPreference::Automatic => String::from("Automatic"),
            ModelPreference::Fixed(model) => String::from(model.name()),
        };

        let mut choices = vec![
            Choice::new(
                Icon::Settings,
                "As in the settings",
                ChoiceStyle::Secondary,
                DialogAction::Stay(LibraryMessage::GameModelPicked(path.clone(), None).into()),
            )
            .hint(settings_model)
            .checked(picked.is_none()),
        ];
        choices.extend(Console::ALL.map(|console| {
            let in_use = picked.filter(|&model| Console::of(model) == console);
            let action = if console.revisions().len() > 1 {
                DialogMessage::ShowGameMenu(path.clone(), GameMenuPage::Revisions(console)).into()
            } else {
                LibraryMessage::GameModelPicked(path.clone(), Some(console.recommended())).into()
            };
            let choice = Choice::new(
                Icon::Gamepad2,
                console.label(),
                ChoiceStyle::Secondary,
                DialogAction::Stay(action),
            )
            .checked(in_use.is_some());
            match in_use {
                Some(model) if console.revisions().len() > 1 => choice.hint(model.name()),
                _ => choice,
            }
        }));

        DialogContent {
            title: format!("Play {} on", entry.name()),
            text: None,
            notice: None,
            layout: Layout::List,
            choices,
            dismiss: DialogAction::Stay(
                DialogMessage::ShowGameMenu(path.clone(), GameMenuPage::Main).into(),
            ),
            click_outside_dismisses: true,
        }
    }

    /// Opens `kind`, unless another dialog is already open.
    pub(super) fn open_dialog(&mut self, kind: DialogKind) {
        if self.dialog.is_none() {
            self.dialog = Some(Dialog::new(kind));
        }
    }

    /// Closes the open dialog once what it was about is gone, such as a
    /// game's menu after a rescan removed the game: it would show nothing,
    /// yet keep the controller and the game waiting behind it.
    pub(super) fn close_emptied_dialog(&mut self) {
        if self.dialog.is_some() && self.dialog_content().is_none() {
            self.dialog = None;
        }
    }

    /// Dismisses the open dialog, as B and Esc do.
    pub(super) fn dismiss_dialog(&mut self) -> Task<Message> {
        match self.dialog_content() {
            Some(content) => self.update(content.dismiss.into_message()),
            None => {
                self.dialog = None;
                Task::none()
            }
        }
    }

    /// Moves between the open dialog's choices with a direction, around the
    /// ends unless the key is `held` (see `crate::selection`), picks one
    /// with A or Start, and dismisses the dialog with B.
    pub(super) fn navigate_dialog(&mut self, action: Action, held: bool) -> Task<Message> {
        let Some(content) = self.dialog_content() else {
            return Task::none();
        };
        let Some(dialog) = &mut self.dialog else {
            return Task::none();
        };
        let choices = content.choices.len();

        match action {
            Action::Up | Action::Left | Action::Down | Action::Right => {
                let step = if matches!(action, Action::Up | Action::Left) {
                    -1
                } else {
                    1
                };
                dialog.selection = selection::moved(dialog.selection, step, choices, !held);
                return self.scroll_to_selection();
            }
            Action::A | Action::Start => {
                if let Some(choice) = content
                    .choices
                    .into_iter()
                    .nth(dialog.selection)
                    .filter(|choice| choice.enabled)
                {
                    return self.update(choice.action.into_message());
                }
            }
            Action::B => return self.dismiss_dialog(),
            _ => {}
        }
        Task::none()
    }

    /// Asks whether to look for newer versions, once: on a start after the
    /// player has played a game, so it never comes on top of the first
    /// questions.
    pub(super) fn ask_about_updates(&mut self) {
        if updates::AVAILABLE
            && !self.fresh
            && self.screen_scale.is_none()
            && self.settings.shared.check_for_updates.is_none()
            && !self.library.recently_played(1).is_empty()
        {
            self.open_dialog(DialogKind::Updates);
        }
    }

    /// Asks whether to download box art, once the library has a folder (in
    /// the browser, a game) and the player hasn't answered yet. Box art waits
    /// for the answer.
    pub(super) fn ask_about_box_art(&mut self) {
        if box_art::AVAILABLE
            && !self.fresh
            && self.screen_scale.is_none()
            && !self.settings.shared.box_art_asked
            && views::library::has_games(&self.library, &self.settings)
        {
            self.open_dialog(DialogKind::BoxArt);
        }
    }
}

/// The question whether to look for newer versions.
fn updates_question() -> DialogContent {
    DialogContent {
        title: String::from("Tell you when a new Nubo is out?"),
        text: Some(String::from(
            "When Nubo starts, it can ask GitHub for the newest version and say when there \
             is one. You can change this later in Settings › About.",
        )),
        notice: None,
        layout: Layout::Buttons,
        choices: vec![
            Choice::new(
                Icon::BellRing,
                "Yes, tell me",
                ChoiceStyle::Primary,
                DialogAction::CloseThen(SettingsMessage::CheckForUpdatesToggled(true).into()),
            ),
            Choice::new(
                Icon::X,
                "No thanks",
                ChoiceStyle::Secondary,
                DialogAction::CloseThen(SettingsMessage::CheckForUpdatesToggled(false).into()),
            ),
        ],
        dismiss: DialogAction::CloseThen(SettingsMessage::CheckForUpdatesToggled(false).into()),
        click_outside_dismisses: false,
    }
}

/// The question whether to download box art.
fn box_art_question() -> DialogContent {
    DialogContent {
        title: String::from("Show box art for your games?"),
        text: Some(format!(
            "Nubo can download box art for the games in your library. To find it, Nubo \
             sends their names to {}. You can change this later in Settings › Library.",
            if box_art::SERVER == "GitHub" {
                "GitHub, which keeps a copy of libretro's thumbnails"
            } else {
                "libretro's thumbnail server"
            }
        )),
        notice: None,
        layout: Layout::Buttons,
        choices: vec![
            Choice::new(
                Icon::Download,
                "Download box art",
                ChoiceStyle::Primary,
                DialogAction::CloseThen(LibraryMessage::BoxArtAnswered(true).into()),
            ),
            Choice::new(
                Icon::X,
                "Not now",
                ChoiceStyle::Secondary,
                DialogAction::CloseThen(LibraryMessage::BoxArtAnswered(false).into()),
            ),
        ],
        dismiss: DialogAction::CloseThen(LibraryMessage::BoxArtAnswered(false).into()),
        click_outside_dismisses: false,
    }
}

/// The menu that picks the order and the kind of games the library lists.
fn sort_menu(settings: &Settings) -> DialogContent {
    let sorts = LibrarySort::ALL.map(|sort| {
        Choice::new(
            Icon::ArrowDownUp,
            sort.label(),
            ChoiceStyle::Secondary,
            DialogAction::Stay(LibraryMessage::SortPicked(sort).into()),
        )
        .checked(settings.library_sort == sort)
    });
    let filters = LibraryFilter::ALL
        .into_iter()
        .enumerate()
        .map(|(index, filter)| {
            let choice = Choice::new(
                Icon::Funnel,
                filter.label(),
                ChoiceStyle::Secondary,
                DialogAction::Stay(LibraryMessage::FilterPicked(filter).into()),
            )
            .checked(settings.library_filter == filter);
            if index == 0 {
                choice.starts_group()
            } else {
                choice
            }
        });

    DialogContent {
        title: String::from("Sort and show"),
        text: None,
        notice: None,
        layout: Layout::List,
        choices: sorts.into_iter().chain(filters).collect(),
        dismiss: DialogAction::Close,
        click_outside_dismisses: true,
    }
}

/// The menu of the library folder at `index` in the settings' list.
fn folder_menu(index: usize, folder: &LibraryFolder) -> DialogContent {
    let mut choices = vec![
        Choice::new(
            Icon::FolderTree,
            "Look in subfolders too",
            ChoiceStyle::Secondary,
            DialogAction::Stay(
                LibraryMessage::IncludeSubfolders(index, !folder.include_subfolders).into(),
            ),
        )
        .checked(folder.include_subfolders),
    ];
    if links::CAN_SHOW_FOLDERS {
        choices.push(Choice::new(
            Icon::FolderOpen,
            "Show in file manager",
            ChoiceStyle::Secondary,
            DialogAction::CloseThen(LibraryMessage::ShowInFolder(folder.path.clone()).into()),
        ));
    }
    choices.extend([
        Choice::new(
            Icon::RefreshCw,
            "Look for new games now",
            ChoiceStyle::Secondary,
            DialogAction::CloseThen(LibraryMessage::Rescan.into()),
        ),
        Choice::new(
            Icon::Trash2,
            "Remove from the library",
            ChoiceStyle::Danger,
            DialogAction::CloseThen(LibraryMessage::RemoveFolder(index).into()),
        )
        .starts_group(),
    ]);

    DialogContent {
        title: folder
            .path
            .file_name()
            .unwrap_or(folder.path.as_os_str())
            .to_string_lossy()
            .into_owned(),
        text: Some(String::from(
            "Removing the folder only takes its games out of the library. The files stay.",
        )),
        notice: None,
        layout: Layout::List,
        choices,
        dismiss: DialogAction::Close,
        click_outside_dismisses: true,
    }
}

/// The page of a game's menu that picks a revision of `console`.
fn revisions_page(entry: &LibraryEntry, console: Console) -> DialogContent {
    let path = &entry.path;
    let choices = console
        .revisions()
        .iter()
        .map(|&revision| {
            let choice = Choice::new(
                Icon::Cpu,
                revision.name(),
                ChoiceStyle::Secondary,
                DialogAction::Stay(
                    LibraryMessage::GameModelPicked(path.clone(), Some(revision)).into(),
                ),
            )
            .checked(entry.model() == Some(revision));
            if revision == console.recommended() {
                choice.hint("Recommended")
            } else {
                choice
            }
        })
        .collect();

    DialogContent {
        title: format!("Play {} on", entry.name()),
        text: Some(String::from(model::about(
            entry
                .model()
                .filter(|&model| Console::of(model) == console)
                .unwrap_or(console.recommended()),
        ))),
        notice: None,
        layout: Layout::List,
        choices,
        dismiss: DialogAction::Stay(
            DialogMessage::ShowGameMenu(path.clone(), GameMenuPage::Consoles).into(),
        ),
        click_outside_dismisses: true,
    }
}

/// What the question that starts over shows a box for besides the
/// settings, which it always offers (see `App::reset_offers`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResetOffers {
    pub delete_battery_saves: bool,
    pub delete_save_states: bool,
    pub remove_folders: bool,
    pub clear_recently_played: bool,
    /// Whether a game is open, which greys out the boxes that delete saves,
    /// with a note to close the game first.
    pub game_open: bool,
}

impl ResetOffers {
    /// Whether the boxes that delete saves can be ticked.
    fn can_delete_saves(self) -> bool {
        !self.game_open
    }
}

impl ResetChoices {
    /// These choices as far as `offers` allows them: a box that isn't on
    /// show or is greyed out resets nothing, such as a deleting one that a
    /// game opened meanwhile greyed out.
    pub fn within(self, offers: ResetOffers) -> ResetChoices {
        let can_delete = offers.can_delete_saves();
        ResetChoices {
            reset_settings: self.reset_settings,
            delete_battery_saves: self.delete_battery_saves
                && offers.delete_battery_saves
                && can_delete,
            delete_save_states: self.delete_save_states && offers.delete_save_states && can_delete,
            remove_folders: self.remove_folders && offers.remove_folders,
            clear_recently_played: self.clear_recently_played && offers.clear_recently_played,
        }
    }

    /// Whether the reset deletes saves, or in the browser the games its
    /// library keeps, which can't be put back.
    pub fn deletes(self) -> bool {
        self.delete_battery_saves
            || self.delete_save_states
            || (self.remove_folders && library::KEEPS_GAMES)
    }
}

/// The question what to start over with: a box for the settings, ticked at
/// first, and one for each other thing `offers` lets it reset, ticked as in
/// `choices`. A box ticks or unticks itself with
/// `DialogMessage::ResetChoicesChanged`. While a game is open, the boxes
/// that delete saves are greyed out and unticked, and say to close the game
/// first, so nobody wonders whether the reset deletes them anyway. The
/// button says what it does, and is greyed out while nothing is ticked.
fn reset_question(choices: ResetChoices, offers: ResetOffers) -> DialogContent {
    let effective = choices.within(offers);
    let can_delete = offers.can_delete_saves();
    let delete_description = |description: &'static str| {
        if can_delete {
            description
        } else {
            "Kept while a game is open. Close the game first to delete them."
        }
    };
    let toggle = |change: fn(&mut ResetChoices)| {
        let mut changed = choices;
        change(&mut changed);
        DialogAction::Stay(DialogMessage::ResetChoicesChanged(changed).into())
    };

    let mut boxes = vec![Choice::checkbox(
        "Reset settings",
        "The theme, colors, model, controls, sound and every other setting, and the console \
         picked for each game. Nubo asks again about box art and new versions.",
        choices.reset_settings,
        ChoiceStyle::Secondary,
        toggle(|choices| choices.reset_settings = !choices.reset_settings),
    )];
    if offers.delete_battery_saves {
        boxes.push(
            Choice::checkbox(
                "Delete in-game saves",
                delete_description(
                    "What every game saved itself, so each one starts from the beginning again.",
                ),
                effective.delete_battery_saves,
                ChoiceStyle::Danger,
                toggle(|choices| choices.delete_battery_saves = !choices.delete_battery_saves),
            )
            .enabled(can_delete),
        );
    }
    if offers.delete_save_states {
        boxes.push(
            Choice::checkbox(
                "Delete save states",
                delete_description(
                    "Where you left off, the quick save and slots 1 to 3, for every game.",
                ),
                effective.delete_save_states,
                ChoiceStyle::Danger,
                toggle(|choices| choices.delete_save_states = !choices.delete_save_states),
            )
            .enabled(can_delete),
        );
    }
    if offers.remove_folders {
        boxes.push(if library::KEEPS_GAMES {
            Choice::checkbox(
                "Remove every game",
                "The library deletes the games it keeps in this browser. Their saves stay \
                 unless you delete them too.",
                choices.remove_folders,
                ChoiceStyle::Danger,
                toggle(|choices| choices.remove_folders = !choices.remove_folders),
            )
        } else {
            Choice::checkbox(
                "Remove game folders",
                "The library forgets its folders. Your ROM files stay where they are.",
                choices.remove_folders,
                ChoiceStyle::Secondary,
                toggle(|choices| choices.remove_folders = !choices.remove_folders),
            )
        });
    }
    if offers.clear_recently_played {
        boxes.push(Choice::checkbox(
            "Clear Continue playing",
            "The library forgets which games you played last.",
            choices.clear_recently_played,
            ChoiceStyle::Secondary,
            toggle(|choices| choices.clear_recently_played = !choices.clear_recently_played),
        ));
    }

    let notice = if effective.deletes() {
        Notice {
            text: String::from("Deleted saves can't be brought back."),
            warning: true,
        }
    } else {
        Notice {
            text: String::from("You can undo this right after."),
            warning: false,
        }
    };
    let (icon, label) = match (effective.reset_settings, effective.deletes()) {
        (true, true) => (Icon::RotateCcw, "Reset and delete"),
        (true, false) => (Icon::RotateCcw, "Reset"),
        (false, true) => (Icon::Trash2, "Delete"),
        (false, false) => (Icon::Eraser, "Clear"),
    };
    let anything = effective.reset_settings
        || effective.deletes()
        || effective.remove_folders
        || effective.clear_recently_played;

    boxes.extend([
        Choice::new(
            Icon::X,
            "Cancel",
            ChoiceStyle::Secondary,
            DialogAction::Close,
        ),
        Choice::new(
            icon,
            label,
            ChoiceStyle::Danger,
            DialogAction::CloseThen(SettingsMessage::Reset(choices).into()),
        )
        .enabled(anything),
    ]);

    DialogContent {
        title: String::from("Start over?"),
        text: Some(String::from("Tick what to put back or remove.")),
        notice: Some(notice),
        layout: Layout::Buttons,
        choices: boxes,
        dismiss: DialogAction::Close,
        click_outside_dismisses: true,
    }
}

/// The question whether to delete the save states of the game at `path`.
fn delete_question(path: &Path, entry: Option<&LibraryEntry>) -> DialogContent {
    let name = entry.map_or_else(
        || nubo_frontend::rom::display_name(path),
        LibraryEntry::name,
    );
    DialogContent {
        title: format!("Delete the save states of {name}?"),
        text: Some(String::from(
            "This deletes where you left off, the quick save and slots 1 to 3. The game's own \
             saves stay. This can't be undone.",
        )),
        notice: None,
        layout: Layout::Buttons,
        choices: vec![
            Choice::new(
                Icon::X,
                "Cancel",
                ChoiceStyle::Secondary,
                DialogAction::Close,
            ),
            Choice::new(
                Icon::Trash2,
                "Delete",
                ChoiceStyle::Danger,
                DialogAction::CloseThen(
                    LibraryMessage::DeleteSaveStates(path.to_path_buf()).into(),
                ),
            ),
        ],
        dismiss: DialogAction::Close,
        click_outside_dismisses: true,
    }
}

/// The question whether to take the game at `path` out of the browser's
/// library.
fn remove_question(path: &Path, entry: Option<&LibraryEntry>) -> DialogContent {
    let name = entry.map_or_else(
        || nubo_frontend::rom::display_name(path),
        LibraryEntry::name,
    );
    DialogContent {
        title: format!("Remove {name} from the library?"),
        text: Some(String::from(
            "This deletes the game from this browser. Its saves and save states stay, and come \
             back if you add the game again.",
        )),
        notice: None,
        layout: Layout::Buttons,
        choices: vec![
            Choice::new(
                Icon::X,
                "Cancel",
                ChoiceStyle::Secondary,
                DialogAction::Close,
            ),
            Choice::new(
                Icon::Trash2,
                "Remove",
                ChoiceStyle::Danger,
                DialogAction::CloseThen(LibraryMessage::RemoveGame(path.to_path_buf()).into()),
            ),
        ],
        dismiss: DialogAction::Close,
        click_outside_dismisses: true,
    }
}

#[cfg(test)]
mod tests;
