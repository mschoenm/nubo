//! Short notifications ("toasts") that pop up in the bottom-right corner,
//! such as an error loading a ROM, and disappear by themselves.

use crate::ui::icons::{self, Icon};
use crate::ui::theme;
use iced::time::{Duration, Instant};
use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Element, Fill, Theme};

/// How long confirmations, such as "Saved to slot 1", stay on screen.
const INFO_DURATION: Duration = Duration::from_secs(3);
/// Errors stay longer, so there is time to read them.
const ERROR_DURATION: Duration = Duration::from_secs(8);
/// Older toasts make way when there are more than this many.
const MAX_VISIBLE: usize = 4;
const WIDTH: f32 = 340.0;

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
}

impl Toast {
    fn duration(&self) -> Duration {
        match self.kind {
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
        self.push(ToastKind::Info, message.into());
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.push(ToastKind::Error, message.into());
    }

    /// Shows a toast, or prints it. A message that is already on screen is
    /// not repeated; its timer starts again instead, so a problem reported
    /// every second shows up once.
    fn push(&mut self, kind: ToastKind, message: String) {
        if self.printed {
            match kind {
                ToastKind::Info => eprintln!("{message}"),
                ToastKind::Error => eprintln!("error: {message}"),
            }
            return;
        }

        let now = Instant::now();

        if let Some(existing) = self
            .toasts
            .iter_mut()
            .find(|toast| toast.kind == kind && toast.message == message)
        {
            existing.shown_at = now;
            return;
        }

        self.toasts.push(Toast {
            kind,
            message,
            shown_at: now,
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

    /// The toasts stacked in the bottom-right corner of a full-window layer.
    /// `on_dismiss` makes the message sent when a toast's close button is
    /// pressed.
    pub fn view<'a, Message: Clone + 'a>(
        &'a self,
        on_dismiss: impl Fn(usize) -> Message,
    ) -> Element<'a, Message> {
        let cards = self
            .toasts
            .iter()
            .enumerate()
            .map(|(index, toast)| toast_card(toast, on_dismiss(index)));

        container(column(cards).spacing(8).width(WIDTH))
            .align_right(Fill)
            .align_bottom(Fill)
            .padding(16)
            .into()
    }
}

fn toast_card<'a, Message: Clone + 'a>(
    toast: &'a Toast,
    on_dismiss: Message,
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

    container(
        row![symbol, text(&toast.message).size(14).width(Fill), close]
            .spacing(10)
            .align_y(Alignment::Center),
    )
    .padding([10, 12])
    .width(Fill)
    .style(theme::floating_card)
    .into()
}

#[cfg(test)]
mod tests;
