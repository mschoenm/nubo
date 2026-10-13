//! The screens of the app and the layers drawn over them.

use crate::app::Message;
use crate::ui::icons::{Icon, icon};
use crate::ui::theme;
use iced::widget::{center, column, container, mouse_area, opaque, text};
use iced::{Alignment, Element};

/// Shows `content` as a dialog over a dimmed screen. Clicking outside the
/// dialog sends `on_dismiss`.
pub fn modal<'a>(content: Element<'a, Message>, on_dismiss: Message) -> Element<'a, Message> {
    opaque(mouse_area(center(opaque(content)).style(theme::backdrop)).on_press(on_dismiss))
}

/// The layer shown while a file is dragged over the window.
pub fn drop_target<'a>() -> Element<'a, Message> {
    center(
        container(
            column![
                icon(Icon::FileUp).size(40).style(theme::accent_text),
                text("Drop to play").size(18).font(theme::HEADING_FONT),
            ]
            .spacing(12)
            .align_x(Alignment::Center),
        )
        .padding([28, 40])
        .style(theme::panel),
    )
    .style(theme::backdrop)
    .into()
}

pub mod game;
pub mod library;
pub mod settings;
