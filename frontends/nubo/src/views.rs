//! The screens of the app and the layers drawn over them.

use crate::app::Message;
use crate::ui::icons::{Icon, icon};
use crate::ui::theme;
use iced::widget::{center, column, container, mouse_area, opaque, text};
use iced::{Alignment, Element};

/// The least space between a dialog and the edges of the window.
const MODAL_MARGIN: f32 = 24.0;

/// Shows `content` as a dialog over a dimmed screen, at least
/// [`MODAL_MARGIN`] from the window's edges, so a dialog that fills the
/// height it gets still shows the screen around it. Clicking outside the
/// dialog sends `on_dismiss`, if there is one.
pub fn modal<'a>(
    content: Element<'a, Message>,
    on_dismiss: Option<Message>,
) -> Element<'a, Message> {
    let mut area = mouse_area(
        center(opaque(content))
            .padding(MODAL_MARGIN)
            .style(theme::backdrop),
    );
    if let Some(on_dismiss) = on_dismiss {
        area = area.on_press(on_dismiss);
    }
    opaque(area)
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

pub mod dialog;
pub mod game;
pub mod library;
pub mod settings;
