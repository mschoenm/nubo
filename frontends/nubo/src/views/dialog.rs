//! A dialog over the current screen (see `crate::dialog`): its title, its
//! text, and its choices, as buttons along the bottom for a question or as
//! rows for a menu, with the one the D-pad is on outlined.

use crate::app::Message;
use crate::dialog::{ChoiceStyle, Dialog, DialogContent, Layout};
use crate::ui::icons::{Icon, icon};
use crate::ui::theme;
use crate::ui::widgets::menu_separator;
use iced::widget::{Column, Row, button, column, container, row, scrollable, space, text};
use iced::{Alignment, Element, Fill};

const QUESTION_WIDTH: f32 = 440.0;
const MENU_WIDTH: f32 = 420.0;

/// The panel of `dialog`, showing `content`.
pub fn view<'a>(dialog: &Dialog, content: DialogContent) -> Element<'a, Message> {
    let selection = dialog.selection;
    let layout = content.layout;

    let mut panel = column![text(content.title).size(19).font(theme::HEADING_FONT)].spacing(12);
    if let Some(body) = content.text {
        panel = panel.push(text(body).size(14).style(theme::muted_text));
    }

    let width = match layout {
        Layout::Buttons => {
            let choices = Row::with_children(content.choices.into_iter().enumerate().map(
                |(index, choice)| {
                    let style = match choice.style {
                        ChoiceStyle::Primary => theme::primary_button,
                        ChoiceStyle::Secondary => theme::secondary_button,
                        ChoiceStyle::Danger => theme::danger_button,
                    };
                    let button = button(
                        row![icon(choice.icon), text(choice.label)]
                            .spacing(8)
                            .align_y(Alignment::Center),
                    )
                    .padding([8, 16])
                    .style(style)
                    .on_press(choice.action.into_message());
                    container(button)
                        .padding(3)
                        .style(theme::focus_ring(index == selection))
                        .into()
                },
            ))
            .spacing(6)
            .align_y(Alignment::Center);
            panel = panel.push(row![space::horizontal(), choices].padding([6, 0]));
            QUESTION_WIDTH
        }
        Layout::List => {
            let mut rows = Column::new().spacing(2);
            for (index, choice) in content.choices.into_iter().enumerate() {
                if choice.starts_group {
                    rows = rows.push(menu_separator());
                }
                rows = rows.push({
                    let check: Element<'a, Message> = if choice.checked {
                        icon(Icon::Check).style(theme::accent_text).into()
                    } else {
                        space().width(18).into()
                    };
                    let label = text(choice.label).width(Fill);
                    let label = if choice.style == ChoiceStyle::Danger {
                        label.style(theme::danger_text)
                    } else {
                        label
                    };
                    let mut content = row![icon(choice.icon), label]
                        .spacing(12)
                        .align_y(Alignment::Center);
                    if let Some(hint) = choice.hint {
                        content = content.push(text(hint).size(12).style(theme::muted_text));
                    }
                    button(content.push(check))
                        .padding([9, 12])
                        .width(Fill)
                        .style(theme::segment(index == selection))
                        .on_press(choice.action.into_message())
                });
            }
            panel = panel.push(scrollable(rows).spacing(8));
            MENU_WIDTH
        }
    };

    container(panel)
        .padding(24)
        .max_width(width)
        .style(theme::panel)
        .into()
}
