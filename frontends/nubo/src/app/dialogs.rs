//! The dialogs over the current screen (see `crate::dialog`): what each one
//! says and offers, picking a choice, and moving between the choices with
//! the D-pad.

use super::playing::ResumeAnswer;
use super::{App, GameMessage, LibraryMessage, Message, SettingsMessage, scroll_in_proportion};
use crate::dialog::{
    Choice, ChoiceStyle, Dialog, DialogAction, DialogContent, DialogKind, Fold, GameMenuPage,
    Layout, Notice, ResetChoices,
};
use crate::selection;
use crate::settings::{LibraryFilter, LibrarySort, Settings};
use crate::ui::icons::Icon;
use crate::views;
use iced::Task;
use nubo_core::Model;
use nubo_frontend::controls::Action;
use nubo_frontend::library::{self, LibraryEntry, SameTitle, box_art};
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
    /// Unfolds the revisions of a console on the page of a library game's
    /// menu that picks its console, or folds them with `None`, with the
    /// D-pad on the row of the console `on`.
    UnfoldConsole {
        path: PathBuf,
        unfolded: Option<Console>,
        on: Console,
    },
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

/// Where the Console row is on the first page of a game's menu, after Play.
const CONSOLE_ROW: usize = 1;

/// How a game's menu names `model`: the console, with the revision when it
/// isn't the most common one, as "Game Boy Color (CGB-C)".
pub(super) fn model_label(model: Model) -> String {
    let console = Console::of(model);
    if model == console.recommended() {
        String::from(console.label())
    } else {
        format!("{} ({})", console.label(), model.name())
    }
}

impl App {
    /// Handles a message about the open dialog: closing it after a choice,
    /// turning to another page of a game's menu, or ticking a box.
    pub(super) fn update_dialog(&mut self, message: DialogMessage) -> Task<Message> {
        match message {
            DialogMessage::Closed(message) => {
                self.dialog = None;
                let task = match message {
                    Some(message) => self.update(*message),
                    None => Task::none(),
                };
                self.ask_about_same_title();
                return task;
            }
            DialogMessage::ShowGameMenu(path, page) => self.show_game_menu(path, page),
            DialogMessage::UnfoldConsole { path, unfolded, on } => {
                self.dialog = Some(Dialog::new(DialogKind::GameMenu(
                    path,
                    GameMenuPage::Consoles(unfolded),
                )));
                let row = self.dialog_content().and_then(|content| {
                    content
                        .choices
                        .iter()
                        .position(|choice| !choice.nested && choice.label == on.label())
                });
                if let (Some(dialog), Some(row)) = (&mut self.dialog, row) {
                    dialog.selection = row;
                }
            }
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

    /// Scrolls a question, or a menu's rows, so the choice the D-pad is on
    /// is in view, in a window too short for all of it. Iced doesn't scroll
    /// to the app's own highlight by itself.
    fn scroll_to_selection(&self) -> Task<Message> {
        let Some((dialog, content)) = self.dialog.as_ref().zip(self.dialog_content()) else {
            return Task::none();
        };
        let id = match content.layout {
            Layout::Buttons => views::dialog::scroll_id(),
            Layout::List => views::dialog::list_scroll_id(),
        };
        scroll_in_proportion(id, dialog.selection, content.choices.len())
    }

    /// What the reset can do besides the settings in this build and this
    /// run. Saves can't be deleted while a game is open, which would write
    /// its own again, so their boxes are grayed out then, and nothing kept
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
                    !self.settings.shared.library_folders.is_empty()
                },
            clear_recently_played: keeps_data && self.library.any_played(),
            game_open: self.game.is_some(),
        }
    }

    /// Opens (or turns to) `page` of the menu of the game at `path`, with the
    /// D-pad on the choice in use (the revision rather than its console when
    /// its revisions are unfolded), or on the Console row when going back
    /// from the page that picks it.
    pub(super) fn show_game_menu(&mut self, path: PathBuf, page: GameMenuPage) {
        let from_consoles = matches!(
            &self.dialog,
            Some(Dialog {
                kind: DialogKind::GameMenu(_, GameMenuPage::Consoles(_)),
                ..
            })
        );
        self.dialog = Some(Dialog::new(DialogKind::GameMenu(path, page)));
        self.refresh_game_menu_save_states();
        let selection = if page == GameMenuPage::Main && from_consoles {
            Some(CONSOLE_ROW)
        } else {
            self.dialog_content().and_then(|content| {
                content
                    .choices
                    .iter()
                    .rposition(|choice| choice.checked || choice.current)
            })
        };
        if let (Some(dialog), Some(selection)) = (&mut self.dialog, selection) {
            dialog.selection = selection;
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
                    GameMenuPage::Consoles(unfolded) => self.consoles_page(entry, unfolded),
                })
            }
            DialogKind::DeleteSaveStates(path) => {
                Some(delete_question(path, self.library.entry(path)))
            }
            DialogKind::RemoveGame(path) => Some(remove_question(path, self.library.entry(path))),
            DialogKind::ResetSettings(choices) => {
                Some(reset_question(*choices, self.reset_offers()))
            }
            DialogKind::OtherModel {
                saved,
                configured,
                remembers,
            } => Some(other_model_question(*saved, *configured, *remembers)),
            DialogKind::SameTitle(path) => Some(same_title_question(path)),
        }
    }

    /// The first page of a library game's menu. Its Console row shows the
    /// console the game itself is set to start on, which later runs use;
    /// with `--model`, a line under it says this run starts every game on
    /// that model instead (see `App::model_for`).
    fn game_menu(&self, entry: &LibraryEntry) -> DialogContent {
        let path = &entry.path;
        let starts_on = entry.starts_on(self.settings.shared.model);
        // A revision other than the most common one shows unfolded.
        let unfolded = starts_on
            .map(Console::of)
            .filter(|console| starts_on != Some(console.recommended()));
        let mut console = Choice::new(
            Icon::Gamepad2,
            "Console",
            ChoiceStyle::Secondary,
            DialogAction::Stay(
                DialogMessage::ShowGameMenu(path.clone(), GameMenuPage::Consoles(unfolded)).into(),
            ),
        )
        .hint(match starts_on {
            Some(model) => format!("{} ›", model_label(model)),
            None => String::from("Automatic ›"),
        });
        if let Some(model) = self.model_override {
            console = console.description(format!(
                "This run starts every game on the {} (--model)",
                model_label(model)
            ));
        }
        let mut choices = vec![
            Choice::new(
                Icon::Play,
                "Play",
                ChoiceStyle::Secondary,
                DialogAction::CloseThen(GameMessage::PlayGame(path.clone()).into()),
            ),
            console,
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
        if self.game_menu_save_states {
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
            back_button: false,
        }
    }

    /// The page of a game's menu that picks the console it starts on, with
    /// the revisions of `unfolded` under it. The console the game starts on
    /// shows in bold, with its revision on the button that unfolds the
    /// revisions (the Console row of the first page unfolds them when it
    /// isn't the most common one), and the console it was made for says so.
    /// Picking a console starts the game on its revision in use, or else the
    /// most common one, and closes the menu.
    fn consoles_page(&self, entry: &LibraryEntry, unfolded: Option<Console>) -> DialogContent {
        let path = &entry.path;
        let settings = self.settings.shared.model;
        let current = entry.starts_on(settings);
        let made_for = entry.made_for().map(Console::of);
        let from_settings = entry.preference().is_none() && settings != ModelPreference::Automatic;
        let pick = |model| {
            DialogAction::CloseThen(LibraryMessage::GameModelPicked(path.clone(), model).into())
        };
        let unfold = |unfolded, on| {
            DialogAction::Stay(
                DialogMessage::UnfoldConsole {
                    path: path.clone(),
                    unfolded,
                    on,
                }
                .into(),
            )
        };

        let mut choices = Vec::new();
        for console in Console::ALL {
            let in_use = current.filter(|&model| Console::of(model) == console);
            let revision = in_use.unwrap_or(console.recommended());
            let is_unfolded = unfolded == Some(console);
            let mut choice = Choice::new(
                Icon::Gamepad2,
                console.label(),
                ChoiceStyle::Secondary,
                pick(revision),
            )
            .current(in_use.is_some());
            if made_for == Some(console) {
                choice = choice.note("Made for this console");
            }
            if in_use.is_some() && from_settings {
                choice = choice.description("Settings › Playing picks it for every game");
            }
            if console.revisions().len() > 1 {
                choice = choice.fold(Fold {
                    label: String::from(revision.name()),
                    unfolded: is_unfolded,
                    action: unfold((!is_unfolded).then_some(console), console),
                });
                choice = if is_unfolded {
                    choice.left(unfold(None, console))
                } else {
                    choice.right(unfold(Some(console), console))
                };
            }
            choices.push(choice);

            if is_unfolded {
                choices.extend(console.revisions().iter().map(|&model| {
                    let choice =
                        Choice::new(Icon::Cpu, model.name(), ChoiceStyle::Secondary, pick(model))
                            .description(model::about(model))
                            .current(current == Some(model))
                            .nested()
                            .left(unfold(None, console));
                    if model == console.recommended() {
                        choice.badge("Most common")
                    } else {
                        choice
                    }
                }));
            }
        }

        DialogContent {
            title: String::from("Console"),
            text: Some(entry.name()),
            notice: None,
            layout: Layout::List,
            choices,
            dismiss: DialogAction::Stay(
                DialogMessage::ShowGameMenu(path.clone(), GameMenuPage::Main).into(),
            ),
            click_outside_dismisses: true,
            back_button: true,
        }
    }

    /// Opens `kind`, unless another dialog is already open.
    pub(super) fn open_dialog(&mut self, kind: DialogKind) {
        if self.dialog.is_none() {
            self.dialog = Some(Dialog::new(kind));
        }
    }

    /// Works out whether the game whose menu is open has save states (see
    /// `App::game_menu_save_states`). The menu only opens in the library,
    /// where no game runs, so only deleting can change that while it is
    /// open. With another dialog open nothing changes.
    pub(super) fn refresh_game_menu_save_states(&mut self) {
        if let Some(Dialog {
            kind: DialogKind::GameMenu(path, _),
            ..
        }) = &self.dialog
        {
            self.game_menu_save_states =
                save_states::AVAILABLE && !self.fresh && save_states::any_saved(path);
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
    /// with A or Start, and dismisses the dialog with B. Left and Right do
    /// what the choice the D-pad is on gives them to do instead, if anything,
    /// such as unfolding a console's revisions.
    pub(super) fn navigate_dialog(&mut self, action: Action, held: bool) -> Task<Message> {
        let Some(content) = self.dialog_content() else {
            return Task::none();
        };
        let Some(dialog) = &mut self.dialog else {
            return Task::none();
        };
        let choices = content.choices.len();
        let sideways = content
            .choices
            .get(dialog.selection)
            .and_then(|choice| match action {
                Action::Left => choice.left.clone(),
                Action::Right => choice.right.clone(),
                _ => None,
            });
        if let Some(sideways) = sideways {
            return self.update(sideways.into_message());
        }

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

    /// Whether the player played a game before this start, which
    /// [`App::ask_about_updates`] waits for.
    pub(super) fn played_before(&self) -> bool {
        !self.library.recently_played(1).is_empty()
    }

    /// Asks whether to look for newer versions, once: on a start after the
    /// player has played a game (`played_before`, from
    /// [`App::played_before`] before the start's own game, if any, began),
    /// so it never comes on top of the first questions or the first game.
    pub(super) fn ask_about_updates(&mut self, played_before: bool) {
        if updates::AVAILABLE
            && !self.fresh
            && self.screen_scale.is_none()
            && self.settings.shared.check_for_updates.is_none()
            && played_before
        {
            self.open_dialog(DialogKind::Updates);
        }
    }

    /// Asks whether to download box art, once the library has a folder (in
    /// the browser, a game) and the player hasn't answered yet. Box art waits
    /// for the answer.
    /// Asks about the first game waiting in `App::same_title_waiting` (see
    /// `DialogKind::SameTitle`), once no other dialog is open; closing one
    /// asks again.
    pub(super) fn ask_about_same_title(&mut self) {
        if self.dialog.is_none()
            && let Some((rom, _)) = self.same_title_waiting.first()
        {
            self.dialog = Some(Dialog::new(DialogKind::SameTitle(rom.path.clone())));
        }
    }

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
        back_button: false,
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
        back_button: false,
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
        back_button: false,
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
        back_button: false,
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
    /// Whether a game is open, which grays out the boxes that delete saves,
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
    /// show or is grayed out resets nothing, such as a deleting one that a
    /// game opened meanwhile grayed out.
    pub fn within(self, offers: ResetOffers) -> ResetChoices {
        let can_delete = offers.can_delete_saves();
        ResetChoices {
            reset_settings: self.reset_settings,
            reset_controls: self.reset_controls,
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
/// first, one for the controls, unticked at first, and one for each other
/// thing `offers` lets it reset, ticked as in `choices`. A box ticks or
/// unticks itself with `DialogMessage::ResetChoicesChanged`. While a game is
/// open, the boxes that delete saves are grayed out and unticked, and say to
/// close the game first, so nobody wonders whether the reset deletes them
/// anyway. The button says what it does, and is grayed out while nothing is
/// ticked.
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

    let mut boxes = vec![
        Choice::checkbox(
            "Reset settings",
            "The theme, colors, model, sound and every other setting but the controls, and \
             the console picked for each game. Nubo asks again about box art and new versions.",
            choices.reset_settings,
            ChoiceStyle::Secondary,
            toggle(|choices| choices.reset_settings = !choices.reset_settings),
        ),
        Choice::checkbox(
            "Reset controls",
            "The keys and controller buttons for every action, and which buttons pick and go \
             back in the menus.",
            choices.reset_controls,
            ChoiceStyle::Secondary,
            toggle(|choices| choices.reset_controls = !choices.reset_controls),
        ),
    ];
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
    let resets = effective.reset_settings || effective.reset_controls;
    let (icon, label) = match (resets, effective.deletes()) {
        (true, true) => (Icon::RotateCcw, "Reset and delete"),
        (true, false) => (Icon::RotateCcw, "Reset"),
        (false, true) => (Icon::Trash2, "Delete"),
        (false, false) => (Icon::Eraser, "Clear"),
    };
    let anything = resets
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
        back_button: false,
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
        back_button: false,
    }
}

/// The question whether to switch a game to `configured`, the model it is
/// now set to start on, starting it from the title screen, or stay on
/// `saved`, the model its automatic save was made on, continuing from that
/// save. With `remembers`, it says that staying keeps the game on `saved`.
/// The models are named by their console, or by their revision when both
/// are the same console.
fn other_model_question(saved: Model, configured: Model, remembers: bool) -> DialogContent {
    let name = |model: Model| {
        if Console::of(saved) == Console::of(configured) {
            model.name()
        } else {
            Console::of(model).label()
        }
    };
    let (saved, configured) = (name(saved), name(configured));
    let answer = |answer| DialogAction::CloseThen(GameMessage::ResumeAnswered(answer).into());
    DialogContent {
        title: format!("Switch to the {configured}?"),
        text: Some(format!(
            "You last played this game on the {saved}, but it's now set to the {configured}.\n\n\
             Switching starts the game at the title screen and replaces your automatic save. Your \
             in-game saves stay.\n\nStaying continues where you left off{}.",
            if remembers {
                format!(" and keeps the game on the {saved}")
            } else {
                String::new()
            }
        )),
        notice: None,
        layout: Layout::Buttons,
        choices: vec![
            Choice::new(
                Icon::Play,
                format!("Stay on the {saved}"),
                ChoiceStyle::Primary,
                answer(ResumeAnswer::Continue),
            ),
            Choice::new(
                Icon::RotateCcw,
                format!("Switch to the {configured}"),
                ChoiceStyle::Secondary,
                answer(ResumeAnswer::StartOver),
            ),
        ],
        dismiss: answer(ResumeAnswer::Cancel),
        click_outside_dismisses: true,
        back_button: false,
    }
}

/// The question whether the game picked as `path` is a new version of the
/// one the browser keeps under the same file name and title, or another
/// game with the same title (see `nubo_frontend::library::SameTitle`).
fn same_title_question(path: &Path) -> DialogContent {
    let name = nubo_frontend::rom::display_name(path);
    let answer =
        |answer| DialogAction::CloseThen(LibraryMessage::SameTitleAnswered(Some(answer)).into());
    DialogContent {
        title: format!("Is this a new version of {name}?"),
        text: Some(String::from(
            "You already have a game with this file name and title, but this file is \
             different. A new version replaces it and keeps its saves. Another game gets saves \
             of its own.",
        )),
        notice: None,
        layout: Layout::Buttons,
        choices: vec![
            Choice::new(
                Icon::RefreshCw,
                "New version of this game (keeps its saves)",
                ChoiceStyle::Primary,
                answer(SameTitle::NewVersion),
            ),
            Choice::new(
                Icon::Plus,
                "Another game (saves of its own)",
                ChoiceStyle::Secondary,
                answer(SameTitle::AnotherGame),
            ),
        ],
        dismiss: DialogAction::CloseThen(LibraryMessage::SameTitleAnswered(None).into()),
        click_outside_dismisses: false,
        back_button: false,
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
        back_button: false,
    }
}

#[cfg(test)]
mod tests;
