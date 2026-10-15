//! The dialogs over the current screen (see `crate::dialog`): what each one
//! says and offers, picking a choice, and moving between the choices with
//! the D-pad.

use super::{App, LibraryMessage, Message};
use crate::dialog::{Choice, ChoiceStyle, Dialog, DialogContent, DialogKind};
use crate::ui::icons::Icon;
use iced::Task;
use nubo_frontend::controls::Action;
use nubo_frontend::library::box_art;

/// What happens to the open dialog.
#[derive(Debug, Clone)]
pub enum DialogMessage {
    /// The player picked a choice or dismissed the dialog: it closes, and
    /// then the message runs.
    Closed(Box<Message>),
}

impl From<DialogMessage> for Message {
    fn from(message: DialogMessage) -> Self {
        Message::Dialog(message)
    }
}

impl App {
    pub(super) fn update_dialog(&mut self, message: DialogMessage) -> Task<Message> {
        match message {
            DialogMessage::Closed(message) => {
                self.dialog = None;
                self.update(*message)
            }
        }
    }

    /// What the open dialog shows, or `None` while none is open.
    pub(crate) fn dialog_content(&self) -> Option<DialogContent> {
        let dialog = self.dialog.as_ref()?;
        Some(match dialog.kind {
            DialogKind::BoxArt => DialogContent {
                title: String::from("Show box art for your games?"),
                text: Some(String::from(
                    "Nubo can download box art for the games in your library. To find it, \
                     Nubo sends their names to libretro's thumbnail server. You can change \
                     this later in Settings › Library.",
                )),
                choices: vec![
                    Choice {
                        icon: Icon::Download,
                        label: String::from("Download box art"),
                        style: ChoiceStyle::Primary,
                        message: LibraryMessage::BoxArtAnswered(true).into(),
                    },
                    Choice {
                        icon: Icon::X,
                        label: String::from("Not now"),
                        style: ChoiceStyle::Secondary,
                        message: LibraryMessage::BoxArtAnswered(false).into(),
                    },
                ],
                dismiss: LibraryMessage::BoxArtAnswered(false).into(),
                click_outside_dismisses: false,
            },
        })
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
            Some(content) => self.update(DialogMessage::Closed(Box::new(content.dismiss)).into()),
            None => Task::none(),
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
                    return self.update(DialogMessage::Closed(Box::new(choice.message)).into());
                }
            }
            Action::B => return self.dismiss_dialog(),
            _ => {}
        }
        Task::none()
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
