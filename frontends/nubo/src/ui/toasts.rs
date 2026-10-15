//! Short notifications ("toasts") that pop up in the bottom-right corner,
//! such as an error loading a ROM, and disappear by themselves. Over a game
//! only the newest one shows, as a small card at the top, so the game stays
//! in view. A toast can offer to undo what it reports.

use crate::ui::icons::{self, Icon};
use crate::ui::theme;
use iced::time::{Duration, Instant};
use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Element, Fill, Theme};

/// How long confirmations, such as "Saved to slot 1", stay on screen.
const INFO_DURATION: Duration = Duration::from_secs(3);
/// Errors stay longer, so there is time to read them.
const ERROR_DURATION: Duration = Duration::from_secs(8);
/// A toast with an Undo button stays long enough to reach for it.
const UNDO_DURATION: Duration = Duration::from_secs(6);
/// Older toasts make way when there are more than this many.
const MAX_VISIBLE: usize = 4;
const WIDTH: f32 = 340.0;
/// How wide a toast over a game gets at most.
const COMPACT_WIDTH: f32 = 380.0;
/// The room above a toast over a game, which keeps it under the game's bar.
const COMPACT_TOP: f32 = 60.0;

/// Whether a toast confirms something or reports a problem, which sets its
/// icon, color and how long it stays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToastKind {
    Info,
    Error,
}

#[derive(Debug)]
struct Toast {
    kind: ToastKind,
    message: String,
    shown_at: Instant,
    /// How many more times the same message came while it showed.
    repeats: u32,
    /// Whether the toast has an Undo button.
    undo: bool,
}

impl Toast {
    fn duration(&self) -> Duration {
        match self.kind {
            ToastKind::Info if self.undo => UNDO_DURATION,
            ToastKind::Info => INFO_DURATION,
            ToastKind::Error => ERROR_DURATION,
        }
    }
}

/// The toasts on screen, oldest first.
#[derive(Debug, Default)]
pub struct Toasts {
    toasts: Vec<Toast>,
    /// Whether messages are printed on stderr instead of shown, for a
    /// window that shows only the game or a run that keeps nothing (see
    /// `command_line`).
    printed: bool,
}

impl Toasts {
    /// Toasts that are printed on stderr instead of shown.
    pub fn printed() -> Self {
        Toasts {
            toasts: Vec::new(),
            printed: true,
        }
    }

    pub fn info(&mut self, message: impl Into<String>) {
        self.push(ToastKind::Info, message.into(), false);
    }

    /// A confirmation with an Undo button, such as "Loaded Quick save".
    /// Only the newest toast keeps its button, since only the last thing
    /// done can be undone.
    pub fn info_with_undo(&mut self, message: impl Into<String>) {
        for toast in &mut self.toasts {
            toast.undo = false;
        }
        self.push(ToastKind::Info, message.into(), true);
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.push(ToastKind::Error, message.into(), false);
    }

    /// Takes the Undo button off the toast that has it, once the undo is
    /// done or no longer possible.
    pub fn forget_undo(&mut self) {
        self.toasts.retain(|toast| !toast.undo);
    }

    /// Shows a toast, or prints it. A message that is already on screen is
    /// not repeated; it moves to the newest place, its timer starts again
    /// and it counts the repeat instead, so a problem reported every second
    /// shows up once.
    fn push(&mut self, kind: ToastKind, message: String, undo: bool) {
        if self.printed {
            match kind {
                ToastKind::Info => eprintln!("{message}"),
                ToastKind::Error => eprintln!("error: {message}"),
            }
            return;
        }

        let now = Instant::now();

        let repeats = match self
            .toasts
            .iter()
            .position(|toast| toast.kind == kind && toast.message == message)
        {
            Some(index) => self.toasts.remove(index).repeats + 1,
            None => 0,
        };

        self.toasts.push(Toast {
            kind,
            message,
            shown_at: now,
            repeats,
            undo,
        });

        if self.toasts.len() > MAX_VISIBLE {
            self.toasts.remove(0);
        }
    }

    /// Removes the toasts that have been shown long enough.
    pub fn remove_expired(&mut self, now: Instant) {
        self.toasts
            .retain(|toast| now.saturating_duration_since(toast.shown_at) < toast.duration());
    }

    /// Removes the toast at `index`, when the user closes it.
    pub fn dismiss(&mut self, index: usize) {
        if index < self.toasts.len() {
            self.toasts.remove(index);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    /// The toasts stacked in the bottom-right corner of a full-window layer,
    /// or with `over_game` only the newest one, at the top in the middle.
    /// `on_dismiss` makes the message sent when a toast's close button is
    /// pressed, and `on_undo` is sent by an Undo button.
    pub fn view<'a, Message: Clone + 'a>(
        &'a self,
        on_dismiss: impl Fn(usize) -> Message,
        on_undo: Message,
        over_game: bool,
    ) -> Element<'a, Message> {
        if over_game {
            let newest = self.toasts.len().saturating_sub(1);
            let card = self
                .toasts
                .last()
                .map(|toast| toast_card(toast, on_dismiss(newest), on_undo.clone(), true));
            return container(column(card).max_width(COMPACT_WIDTH))
                .align_top(Fill)
                .center_x(Fill)
                .padding(iced::Padding::from([COMPACT_TOP, 16.0]))
                .into();
        }

        let cards = self
            .toasts
            .iter()
            .enumerate()
            .map(|(index, toast)| toast_card(toast, on_dismiss(index), on_undo.clone(), false));

        container(column(cards).spacing(8).width(WIDTH))
            .align_right(Fill)
            .align_bottom(Fill)
            .padding(16)
            .into()
    }
}

/// A toast's card: its icon, its message (with how often it came, if more
/// than once), an Undo button if it has one, and a close button. A compact
/// card, over a game, uses smaller text.
fn toast_card<'a, Message: Clone + 'a>(
    toast: &'a Toast,
    on_dismiss: Message,
    on_undo: Message,
    compact: bool,
) -> Element<'a, Message> {
    let kind = toast.kind;
    let symbol = icons::icon(match kind {
        ToastKind::Info => Icon::CircleCheck,
        ToastKind::Error => Icon::CircleAlert,
    })
    .style(move |theme: &Theme| text::Style {
        color: Some(match kind {
            ToastKind::Info => theme.palette().success,
            ToastKind::Error => theme.palette().danger,
        }),
    });

    let close = button(icons::icon(Icon::X).size(14))
        .padding(4)
        .style(theme::icon_button)
        .on_press(on_dismiss);

    let message = if toast.repeats > 0 {
        format!("{} (×{})", toast.message, toast.repeats + 1)
    } else {
        toast.message.clone()
    };
    let message = text(message).size(if compact { 13 } else { 14 });
    let mut content = row![
        symbol,
        if compact {
            message
        } else {
            message.width(Fill)
        }
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    if toast.undo {
        content = content.push(
            button(text("Undo").size(13))
                .padding([3, 10])
                .style(theme::secondary_button)
                .on_press(on_undo),
        );
    }

    container(content.push(close))
        .padding(if compact { [6, 10] } else { [10, 12] })
        .width(if compact { iced::Length::Shrink } else { Fill })
        .style(theme::floating_card)
        .into()
}

#[cfg(test)]
mod tests;
