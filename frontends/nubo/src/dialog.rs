//! The small dialogs the app opens over the current screen: questions such
//! as whether to download box art, and menus such as a library game's. Each
//! is a title, maybe a short text, and a few choices; a question can also
//! have checkboxes above its buttons, such as what else a reset of the
//! settings deletes. The D-pad moves between the choices, checkboxes
//! included, A picks one (or ticks it) and B or Esc dismisses the dialog (or
//! goes back a page in a menu). What each dialog says and offers is worked
//! out by the app (`app/dialogs.rs`), so the view and the D-pad handling see
//! the same choices.

use crate::app::Message;
use crate::ui::icons::Icon;
use nubo_frontend::model::Console;
use std::path::PathBuf;

/// Which dialog is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogKind {
    /// Asks once whether to download box art for the library (see
    /// `nubo_frontend::settings::Settings::box_art_asked`).
    BoxArt,
    /// Asks once whether to look for a newer Nubo when the app starts.
    Updates,
    /// The menu that sorts and filters the library's games.
    SortMenu,
    /// The menu of the library game at the path, on one of its pages.
    GameMenu(PathBuf, GameMenuPage),
    /// The menu of the library folder at this place in the settings' list.
    FolderMenu(usize),
    /// Asks whether to delete the save states of the game at the path.
    DeleteSaveStates(PathBuf),
    /// Asks what to start over with, ticked as so far: the settings, the
    /// saves, the library's folders and "Continue playing".
    ResetSettings(ResetChoices),
}

/// What "Start over" resets, as the player ticks it in its dialog. Only
/// the settings start ticked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResetChoices {
    /// Puts every setting back to how it started and forgets the model
    /// picked for each game.
    pub reset_settings: bool,
    /// Deletes every game's battery saves: the progress games save
    /// themselves.
    pub delete_battery_saves: bool,
    /// Deletes every game's save states.
    pub delete_save_states: bool,
    /// Takes every folder out of the library.
    pub remove_folders: bool,
    /// Empties "Continue playing", or in the browser forgets the last game
    /// played.
    pub clear_recently_played: bool,
}

impl Default for ResetChoices {
    fn default() -> Self {
        ResetChoices {
            reset_settings: true,
            delete_battery_saves: false,
            delete_save_states: false,
            remove_folders: false,
            clear_recently_played: false,
        }
    }
}

/// A page of a library game's menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameMenuPage {
    /// Play, Play on…, Show in folder and the like.
    Main,
    /// The consoles the game can start on.
    Consoles,
    /// The revisions of a console the game can start on.
    Revisions(Console),
}

/// An open dialog and the choice the D-pad is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    pub kind: DialogKind,
    pub selection: usize,
}

impl Dialog {
    /// A dialog that just opened, with the D-pad on its first choice.
    pub fn new(kind: DialogKind) -> Dialog {
        Dialog { kind, selection: 0 }
    }
}

/// How a choice looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceStyle {
    /// The accent-colored button for the choice most players make.
    Primary,
    /// An outlined button, or a plain row in a list.
    Secondary,
    /// A red button for a choice that deletes something for good.
    Danger,
}

/// How a dialog lays out its choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Buttons in a row at the bottom, for a question, under its
    /// checkboxes, which come first among the choices.
    Buttons,
    /// Rows from top to bottom, for a menu.
    List,
}

/// What picking a choice (or dismissing the dialog) does.
#[derive(Debug, Clone)]
pub enum DialogAction {
    /// Closes the dialog.
    Close,
    /// Closes the dialog, then sends the message.
    CloseThen(Message),
    /// Sends the message and keeps the dialog open, such as for a choice
    /// that opens another page of a menu.
    Stay(Message),
}

/// One of a dialog's choices.
pub struct Choice {
    pub icon: Icon,
    pub label: String,
    /// A short note at the right of a row in a list, such as the model a
    /// game starts on.
    pub hint: Option<String>,
    pub style: ChoiceStyle,
    /// Whether this is the option in use, shown with a check mark.
    pub checked: bool,
    /// Whether a thin line above the row starts a new group of rows in a
    /// list.
    pub starts_group: bool,
    /// Whether this is a checkbox above a question's buttons, ticked when
    /// `checked`, rather than a button.
    pub checkbox: bool,
    /// What a checkbox does, in small print under its label.
    pub description: Option<String>,
    /// Whether the choice can be picked; a question's button that would do
    /// nothing is greyed out.
    pub enabled: bool,
    pub action: DialogAction,
}

impl Choice {
    /// A choice with no hint that isn't checked.
    pub fn new(
        icon: Icon,
        label: impl Into<String>,
        style: ChoiceStyle,
        action: DialogAction,
    ) -> Choice {
        Choice {
            icon,
            label: label.into(),
            hint: None,
            style,
            checked: false,
            starts_group: false,
            checkbox: false,
            description: None,
            enabled: true,
            action,
        }
    }

    /// A checkbox of a question, ticked when `checked`, with what it does
    /// under its label. Its `action` ticks or unticks it.
    pub fn checkbox(
        label: impl Into<String>,
        description: impl Into<String>,
        checked: bool,
        style: ChoiceStyle,
        action: DialogAction,
    ) -> Choice {
        Choice {
            checkbox: true,
            description: Some(description.into()),
            ..Choice::new(Icon::Square, label, style, action).checked(checked)
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Choice {
        self.enabled = enabled;
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Choice {
        self.hint = Some(hint.into());
        self
    }

    pub fn checked(mut self, checked: bool) -> Choice {
        self.checked = checked;
        self
    }

    pub fn starts_group(mut self) -> Choice {
        self.starts_group = true;
        self
    }
}

/// What an open dialog shows and does.
pub struct DialogContent {
    pub title: String,
    pub text: Option<String>,
    /// A line in a row of its own under the text and checkboxes, such as
    /// that what the ticked boxes delete can't be brought back.
    pub notice: Option<Notice>,
    pub layout: Layout,
    pub choices: Vec<Choice>,
    /// What dismissing the dialog does (B, Esc).
    pub dismiss: DialogAction,
    /// Whether a click outside the dialog dismisses it too. Questions whose
    /// answer is kept leave it off, so a stray click doesn't answer them.
    pub click_outside_dismisses: bool,
}

/// A short line under a question's text and checkboxes, in a row of its
/// own. A question whose notice changes with its boxes keeps it to one line,
/// so the dialog keeps its height and nothing moves under the pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub text: String,
    /// Whether it warns, tinted with the warning color, rather than only
    /// tells.
    pub warning: bool,
}
