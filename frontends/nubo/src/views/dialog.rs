//! A dialog over the current screen (see `crate::dialog`): its title, its
//! text, and its choices as buttons along the bottom, with the one the D-pad
//! is on outlined.

use crate::app::Message;
use crate::dialog::{ChoiceStyle, Dialog, DialogContent};
use crate::ui::icons::icon;
use crate::ui::theme;
use iced::widget::{Row, button, column, container, row, space, text};
use iced::{Alignment, Element};

const WIDTH: f32 = 440.0;

/// The panel of `dialog`, showing `content`.
pub fn view<'a>(dialog: &Dialog, content: DialogContent) -> Element<'a, Message> {
    let selection = dialog.selection;
    let choices = Row::with_children(content.choices.into_iter().enumerate().map(
        |(index, choice)| {
            let style = match choice.style {
                ChoiceStyle::Primary => theme::primary_button,
                ChoiceStyle::Secondary => theme::secondary_button,
            };
            let button = button(
                row![icon(choice.icon), text(choice.label)]
                    .spacing(8)
                    .align_y(Alignment::Center),
            )
            .padding([8, 16])
            .style(style)
            .on_press(Message::Dialog(crate::app::DialogMessage::Closed(
                Box::new(choice.message),
            )));
            container(button)
                .padding(3)
                .style(theme::focus_ring(index == selection))
                .into()
        },
    ))
    .spacing(6)
    .align_y(Alignment::Center);

    let mut panel = column![text(content.title).size(19).font(theme::HEADING_FONT)].spacing(12);
    if let Some(body) = content.text {
        panel = panel.push(text(body).size(14).style(theme::muted_text));
    }
    panel = panel.push(row![space::horizontal(), choices].padding([6, 0]));

    container(panel)
        .padding(24)
        .max_width(WIDTH)
        .style(theme::panel)
        .into()
}
