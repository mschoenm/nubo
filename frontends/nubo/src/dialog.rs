//! The small dialogs the app opens over the current screen to ask the
//! player something, such as whether to download box art. Each is a title,
//! a short text and a few choices; the D-pad moves between the choices, A
//! picks one and B or Esc dismisses the dialog. What each dialog says and
//! offers is worked out by the app (`app/dialogs.rs`), so the view and the
//! D-pad handling see the same choices.

use crate::app::Message;
use crate::ui::icons::Icon;

/// Which dialog is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogKind {
    /// Asks once whether to download box art for the library (see
    /// `nubo_frontend::settings::Settings::box_art_asked`).
    BoxArt,
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
}

/// One of a dialog's choices.
pub struct Choice {
    pub icon: Icon,
    pub label: String,
    pub style: ChoiceStyle,
    /// What picking it does, after the dialog closes.
    pub message: Message,
}

/// What an open dialog shows and does.
pub struct DialogContent {
    pub title: String,
    pub text: Option<String>,
    pub choices: Vec<Choice>,
    /// What dismissing the dialog does (B, Esc), after it closes.
    pub dismiss: Message,
    /// Whether a click outside the dialog dismisses it too. Questions whose
    /// answer is kept leave it off, so a stray click doesn't answer them.
    pub click_outside_dismisses: bool,
}
