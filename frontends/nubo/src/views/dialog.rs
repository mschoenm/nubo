//! A dialog over the current screen (see `crate::dialog`): its title, its
//! text, and its choices, as buttons along the bottom for a question (under
//! its checkboxes and notice, if it has them) or as rows for a menu, with
//! the one the D-pad is on outlined. A question that doesn't fit the window
//! scrolls.

use crate::app::Message;
use crate::dialog::{Choice, ChoiceStyle, Dialog, DialogContent, Layout, Notice};
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

    let scrolls = layout == Layout::Buttons;
    let width = match layout {
        Layout::Buttons => {
            let (boxes, buttons): (Vec<_>, Vec<_>) = content
                .choices
                .into_iter()
                .enumerate()
                .partition(|(_, choice)| choice.checkbox);
            if !boxes.is_empty() {
                panel = panel.push(checkboxes(boxes, selection));
            }
            if let Some(notice) = content.notice {
                panel = panel.push(notice_row(notice));
            }
            let choices = Row::with_children(buttons.into_iter().map(|(index, choice)| {
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
            }))
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

    // A question scrolls as a whole in a window too short for it, so its
    // buttons stay reachable; a menu scrolls its rows instead.
    let panel = container(panel).padding(24);
    let panel: Element<'a, Message> = if scrolls {
        scrollable(panel).into()
    } else {
        panel.into()
    };
    container(panel).max_width(width).style(theme::panel).into()
}

/// A question's checkboxes, `(index, choice)` by their place among its
/// choices, in a card with thin lines between them. Each is a row that
/// ticks or unticks it, highlighted while the D-pad is on it, and a ticked
/// box that deletes something shows its label in red.
fn checkboxes<'a>(boxes: Vec<(usize, Choice)>, selection: usize) -> Element<'a, Message> {
    let mut card = Column::new();
    for (place, (index, choice)) in boxes.into_iter().enumerate() {
        if place > 0 {
            card = card.push(
                container(space())
                    .width(Fill)
                    .height(1)
                    .style(theme::divider),
            );
        }
        let (mark, mark_style): (Icon, fn(&iced::Theme) -> text::Style) = if choice.checked {
            (Icon::SquareCheckBig, theme::accent_text)
        } else {
            (Icon::Square, theme::muted_text)
        };
        let label = text(choice.label).size(14);
        let label = if choice.checked && choice.style == ChoiceStyle::Danger {
            label.style(theme::danger_text)
        } else {
            label
        };
        let mut words = column![label].spacing(3).width(Fill);
        if let Some(description) = choice.description {
            words = words.push(text(description).size(12).style(theme::muted_text));
        }
        card = card.push(
            button(
                row![icon(mark).style(mark_style), words]
                    .spacing(12)
                    .align_y(Alignment::Start),
            )
            .padding([10, 12])
            .width(Fill)
            .style(theme::segment(index == selection))
            .on_press(choice.action.into_message()),
        );
    }
    container(card)
        .width(Fill)
        .style(theme::settings_card)
        .into()
}

/// A notice in a row of its own with its icon: a warning tinted with the
/// warning color, anything else plain, the same height either way.
fn notice_row<'a>(notice: Notice) -> Element<'a, Message> {
    let (symbol, color): (Icon, fn(&iced::Theme) -> text::Style) = if notice.warning {
        (Icon::TriangleAlert, theme::warning_text)
    } else {
        (Icon::Undo2, theme::muted_text)
    };
    let text = text(notice.text).size(13).width(Fill);
    let text = if notice.warning {
        text
    } else {
        text.style(theme::muted_text)
    };
    let row = container(
        row![icon(symbol).style(color), text]
            .spacing(12)
            .align_y(Alignment::Center),
    )
    .padding([10, 14])
    .width(Fill);
    if notice.warning {
        row.style(theme::notice_row(false)).into()
    } else {
        row.style(theme::focus_ring(false)).into()
    }
}
