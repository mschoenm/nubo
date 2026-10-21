//! A dialog over the current screen (see `crate::dialog`): its title, its
//! text, and its choices, as buttons along the bottom for a question (under
//! its checkboxes and notice, if it has them; stacked, each as wide as the
//! question, when their labels are too long for one line) or as rows for a
//! menu, with the one the D-pad is on outlined, and an arrow before the
//! title of a menu's page that goes back. A question that doesn't fit the
//! window scrolls.

use crate::app::Message;
use crate::dialog::{Choice, ChoiceStyle, Dialog, DialogContent, Layout, Notice};
use crate::ui::icons::{Icon, icon};
use crate::ui::theme;
use crate::ui::widgets::menu_separator;
use iced::widget::{Column, Row, button, column, container, row, scrollable, space, text};
use iced::{Alignment, Element, Fill, widget};

const QUESTION_WIDTH: f32 = 440.0;
const MENU_WIDTH: f32 = 420.0;

/// The room a question's buttons have side by side: its width less its
/// padding.
const BUTTONS_WIDTH: f32 = QUESTION_WIDTH - 2.0 * 24.0;

/// Whether `buttons` fit side by side on one line of a question, judged by
/// the length of their labels: buttons whose labels would wrap stack instead,
/// each as wide as the question.
fn buttons_fit<'a>(buttons: impl Iterator<Item = &'a Choice>) -> bool {
    // About the width of an average character of the body font at its usual
    // size, and of a button around its label: focus ring, padding, icon and
    // the space after it, and the space between buttons.
    const CHARACTER: f32 = 8.5;
    const AROUND_LABEL: f32 = 6.0 + 32.0 + 18.0 + 8.0 + 6.0;
    #[expect(
        clippy::cast_precision_loss,
        reason = "labels are a few dozen characters long"
    )]
    let width: f32 = buttons
        .map(|choice| choice.label.chars().count() as f32 * CHARACTER + AROUND_LABEL)
        .sum();
    width <= BUTTONS_WIDTH
}

/// The panel of `dialog`, showing `content`.
pub fn view<'a>(dialog: &Dialog, content: DialogContent) -> Element<'a, Message> {
    let selection = dialog.selection;
    let layout = content.layout;

    let title = text(content.title).size(19).font(theme::HEADING_FONT);
    let mut panel = if content.back_button {
        let back = button(icon(Icon::ArrowLeft))
            .padding(6)
            .style(theme::icon_button)
            .on_press(content.dismiss.clone().into_message());
        column![row![back, title].spacing(8).align_y(Alignment::Center)]
    } else {
        column![title]
    }
    .spacing(12);
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
            let stacked = !buttons_fit(buttons.iter().map(|(_, choice)| choice));
            let choices = buttons.into_iter().map(|(index, choice)| {
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
                .on_press_maybe(choice.enabled.then(|| choice.action.into_message()));
                let button = if stacked { button.width(Fill) } else { button };
                container(button)
                    .padding(3)
                    .style(theme::focus_ring(index == selection))
                    .into()
            });
            panel = if stacked {
                panel.push(Column::with_children(choices).spacing(6).padding([6, 0]))
            } else {
                let choices = Row::with_children(choices)
                    .spacing(6)
                    .align_y(Alignment::Center);
                panel.push(row![space::horizontal(), choices].padding([6, 0]))
            };
            QUESTION_WIDTH
        }
        Layout::List => {
            // A menu that shows what is in use in bold has no check marks,
            // so its rows keep no room for one.
            let checks = !content.choices.iter().any(|choice| choice.current);
            let mut rows = Column::new().spacing(2);
            for (index, choice) in content.choices.into_iter().enumerate() {
                if choice.starts_group {
                    rows = rows.push(menu_separator());
                }
                rows = rows.push(list_row(choice, index == selection, checks));
            }
            panel = panel.push(scrollable(rows).spacing(8));
            MENU_WIDTH
        }
    };

    // A question scrolls as a whole in a window too short for it, so its
    // buttons stay reachable; a menu scrolls its rows instead.
    let panel = container(panel).padding(24);
    let panel: Element<'a, Message> = if scrolls {
        scrollable(panel).id(scroll_id()).into()
    } else {
        panel.into()
    };
    container(panel).max_width(width).style(theme::panel).into()
}

/// A row of a menu, outlined while the D-pad is on it: the choice's icon
/// (or, for a nested row, room for one), its label, in bold while it is in
/// use (in the accent color too where the bold font is the body font, as in
/// the browser), with its badge, note and description under it, its hint,
/// the button that unfolds or folds the rows under it, and, with `checks`,
/// a check mark while it is checked.
fn list_row<'a>(choice: Choice, focused: bool, checks: bool) -> Element<'a, Message> {
    let font = if choice.current {
        theme::HEADING_FONT
    } else {
        theme::BODY_FONT
    };
    let label = text(choice.label).font(font);
    let label = if choice.style == ChoiceStyle::Danger {
        label.style(theme::danger_text)
    } else if choice.current && theme::HEADING_FONT == theme::BODY_FONT {
        label.style(theme::accent_text)
    } else {
        label
    };
    let mut first_line = row![label].spacing(8).align_y(Alignment::Center);
    if let Some(badge) = choice.badge {
        first_line = first_line.push(
            container(text(badge).size(11))
                .padding([1, 7])
                .style(theme::pill),
        );
    }
    let mut words = column![first_line].spacing(3).width(Fill);
    if let Some(note) = choice.note {
        words = words.push(
            row![
                icon(Icon::Sparkles).size(12).style(theme::accent_text),
                text(note)
                    .size(12)
                    .font(theme::HEADING_FONT)
                    .style(theme::accent_text),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        );
    }
    if let Some(description) = choice.description {
        words = words.push(text(description).size(12).style(theme::muted_text));
    }

    let start: Element<'a, Message> = if choice.nested {
        space().width(18).into()
    } else {
        icon(choice.icon).into()
    };
    let mut content = row![start, words].spacing(12).align_y(Alignment::Center);
    if let Some(hint) = choice.hint {
        content = content.push(text(hint).size(12).style(theme::muted_text));
    }
    if let Some(fold) = choice.fold {
        let chevron = if fold.unfolded {
            Icon::ChevronUp
        } else {
            Icon::ChevronDown
        };
        content = content.push(
            button(
                row![text(fold.label).size(12), icon(chevron).size(14)]
                    .spacing(4)
                    .align_y(Alignment::Center),
            )
            .padding([4, 8])
            .style(theme::secondary_button)
            .on_press(fold.action.into_message()),
        );
    }
    if checks {
        let check: Element<'a, Message> = if choice.checked {
            icon(Icon::Check).style(theme::accent_text).into()
        } else {
            space().width(18).into()
        };
        content = content.push(check);
    }
    button(content)
        .padding([9, 12])
        .width(Fill)
        .style(theme::segment(focused))
        .on_press(choice.action.into_message())
        .into()
}

/// The scrolling area of a question, so the app can keep the choice the
/// D-pad is on in view.
pub fn scroll_id() -> widget::Id {
    widget::Id::new("dialog-question")
}

/// A question's checkboxes, `(index, choice)` by their place among its
/// choices, in a card with thin lines between them. Each is a row that
/// ticks or unticks it, highlighted while the D-pad is on it, and a ticked
/// box that deletes something shows its label in red. A box that can't be
/// ticked is greyed out, with its description, which says why, readable.
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
        let mark = if choice.checked {
            icon(Icon::SquareCheckBig).style(theme::accent_text)
        } else if choice.enabled {
            icon(Icon::Square).style(theme::muted_text)
        } else {
            icon(Icon::Square)
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
            button(row![mark, words].spacing(12).align_y(Alignment::Start))
                .padding([10, 12])
                .width(Fill)
                .style(theme::segment(index == selection))
                .on_press_maybe(choice.enabled.then(|| choice.action.into_message())),
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
