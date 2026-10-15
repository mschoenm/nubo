//! The dialogs over the current screen (see `crate::dialog`): what each one
//! says and offers, picking a choice, and moving between the choices with
//! the D-pad.

use super::{App, GameMessage, LibraryMessage, Message, SettingsMessage};
use crate::dialog::{
    Choice, ChoiceStyle, Dialog, DialogAction, DialogContent, DialogKind, GameMenuPage, Layout,
};
use crate::settings::{LibraryFilter, LibrarySort, Settings};
use crate::ui::icons::Icon;
use iced::Task;
use nubo_frontend::controls::Action;
use nubo_frontend::library::{LibraryEntry, box_art};
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
    pub(super) fn update_dialog(&mut self, message: DialogMessage) -> Task<Message> {
        match message {
            DialogMessage::Closed(message) => {
                self.dialog = None;
                if let Some(message) = message {
                    return self.update(*message);
                }
            }
            DialogMessage::ShowGameMenu(path, page) => self.show_game_menu(path, page),
        }
        Task::none()
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

        DialogContent {
            title: entry.name(),
            text: None,
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

    /// Moves between the open dialog's choices with a direction, picks one
    /// with A or Start, and dismisses the dialog with B.
    pub(super) fn navigate_dialog(&mut self, action: Action) -> Task<Message> {
        let Some(content) = self.dialog_content() else {
            return Task::none();
        };
        let Some(dialog) = &mut self.dialog else {
            return Task::none();
        };
        let last = content.choices.len().saturating_sub(1);

        match action {
            Action::Up | Action::Left => dialog.selection = dialog.selection.saturating_sub(1),
            Action::Down | Action::Right => dialog.selection = (dialog.selection + 1).min(last),
            Action::A | Action::Start => {
                if let Some(choice) = content.choices.into_iter().nth(dialog.selection) {
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

    /// Asks whether to download box art, once the library has a folder and
    /// the player hasn't answered yet. Box art waits for the answer.
    pub(super) fn ask_about_box_art(&mut self) {
        if box_art::AVAILABLE
            && !self.fresh
            && self.screen_scale.is_none()
            && !self.settings.shared.box_art_asked
            && !self.settings.shared.library_folders.is_empty()
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
        text: Some(String::from(
            "Nubo can download box art for the games in your library. To find it, Nubo \
             sends their names to libretro's thumbnail server. You can change this later in \
             Settings › Library.",
        )),
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
        layout: Layout::List,
        choices,
        dismiss: DialogAction::Stay(
            DialogMessage::ShowGameMenu(path.clone(), GameMenuPage::Consoles).into(),
        ),
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
