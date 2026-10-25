//! Small building blocks shared by the screens, so buttons, labels and
//! sections look the same everywhere.

use crate::ui::icons::{self, Icon};
use crate::ui::theme;
use iced::widget::text::Wrapping;
use iced::widget::{
    button, center, column, container, image, responsive, row, stack, text, tooltip,
};
use iced::{Alignment, Color, Element, Fill, Font, Size};

/// The average width of a character as a fraction of the font size, used to
/// guess how much text fits. Game titles measure about 0.46; a bit more
/// leaves room for titles with many wide capitals.
const AVERAGE_CHARACTER_WIDTH: f32 = 0.5;

/// The name of a keyboard shortcut using the platform's command key, such as
/// "⌘O" on macOS and "Ctrl+O" elsewhere.
pub fn shortcut(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("⌘{key}")
    } else {
        format!("Ctrl+{key}")
    }
}

/// A tooltip naming the key the player bound for what a button does, such as
/// "Fullscreen (F11)", or the label alone when the action has no key.
pub fn with_key(label: &str, key: Option<String>) -> String {
    match key {
        Some(key) => format!("{label} ({key})"),
        None => label.to_owned(),
    }
}

/// A borderless icon button with a tooltip below it saying what it does.
/// Without a message the button is shown disabled.
pub fn icon_button<'a, Message: Clone + 'a>(
    icon: Icon,
    label: String,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    let button = button(icons::icon(icon))
        .padding(8)
        .style(theme::icon_button)
        .on_press_maybe(on_press);

    with_tooltip(button, label, tooltip::Position::Bottom)
}

/// An icon button that stays highlighted while its feature is on, such as
/// fast forward, with its tooltip below it.
pub fn toggle_icon_button<'a, Message: Clone + 'a>(
    icon: Icon,
    label: String,
    active: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let button = button(icons::icon(icon))
        .padding(8)
        .style(theme::segment(active))
        .on_press(on_press);

    with_tooltip(button, label, tooltip::Position::Bottom)
}

/// `content` with `label` shown in a small dark box next to it while the
/// pointer is over it.
pub fn with_tooltip<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    label: String,
    position: tooltip::Position,
) -> Element<'a, Message> {
    tooltip(
        content,
        container(text(label).size(12))
            .padding([4, 8])
            .style(theme::tooltip),
        position,
    )
    .gap(6)
    .into()
}

/// A top bar button with an icon and its name, for the buttons a player has
/// to find at a glance, such as Library and Open game, and a tooltip below
/// it with `tip`, such as the shortcut. `primary` fills it with the accent
/// color, as the Resume button is while the game is paused.
pub fn bar_button<'a, Message: Clone + 'a>(
    icon: Icon,
    label: &'a str,
    tip: String,
    primary: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let button = labeled_button(icon, label)
        .padding([6, 12])
        .style(bar_button_style(primary))
        .on_press(on_press);

    with_tooltip(button, tip, tooltip::Position::Bottom)
}

/// A [`bar_button`] whose label changes, such as Pause and Resume, that is
/// always as wide as it is with `widest`, its widest label, so it keeps its
/// size and nothing beside it in the bar moves. An invisible copy of the
/// button with `widest` takes the room, and the label shown sits in the
/// middle of it.
pub fn steady_bar_button<'a, Message: Clone + 'a>(
    icon: Icon,
    (label, widest): (&'a str, &'a str),
    tip: String,
    primary: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let room = row![
        icons::icon(icon).color(Color::TRANSPARENT),
        text(widest).color(Color::TRANSPARENT)
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let shown = container(
        row![icons::icon(icon), text(label)]
            .spacing(8)
            .align_y(Alignment::Center),
    )
    .center_x(Fill);

    let button = button(stack![room, shown])
        .padding([6, 12])
        .style(bar_button_style(primary))
        .on_press(on_press);

    with_tooltip(button, tip, tooltip::Position::Bottom)
}

/// The style of a top bar button: filled with the accent color when
/// `primary`.
fn bar_button_style(primary: bool) -> fn(&iced::Theme, button::Status) -> button::Style {
    if primary {
        theme::primary_button
    } else {
        theme::secondary_button
    }
}

/// A short upright line between groups of buttons in the top bar.
pub fn bar_separator<'a, Message: 'a>() -> Element<'a, Message> {
    container(iced::widget::space())
        .width(1)
        .height(22)
        .style(theme::divider)
        .into()
}

/// A thin line across a menu, between groups of rows.
pub fn menu_separator<'a, Message: 'a>() -> Element<'a, Message> {
    container(divider()).padding([5, 4]).into()
}

/// The filled accent-colored button for the main action on a screen.
pub fn primary_button<'a, Message: Clone + 'a>(
    icon: Icon,
    label: impl text::IntoFragment<'a>,
    on_press: Message,
) -> Element<'a, Message> {
    labeled_button(icon, label)
        .padding([10, 18])
        .style(theme::primary_button)
        .on_press(on_press)
        .into()
}

/// An outlined button for less important actions.
pub fn secondary_button<'a, Message: Clone + 'a>(
    icon: Icon,
    label: impl text::IntoFragment<'a>,
    on_press: Message,
) -> Element<'a, Message> {
    labeled_button(icon, label)
        .padding([7, 14])
        .style(theme::secondary_button)
        .on_press(on_press)
        .into()
}

/// A button showing `icon` before `label`, without a style or an action
/// yet.
fn labeled_button<'a, Message: 'a>(
    icon: Icon,
    label: impl text::IntoFragment<'a>,
) -> button::Button<'a, Message> {
    button(
        row![icons::icon(icon), text(label)]
            .spacing(8)
            .align_y(Alignment::Center),
    )
}

/// A keyboard key drawn as a small keycap.
pub fn keycap<'a, Message: 'a>(label: String) -> Element<'a, Message> {
    container(text(label).size(12))
        .padding([2, 7])
        .style(theme::keycap)
        .into()
}

/// A titled group of settings.
pub fn section<'a, Message: 'a>(
    title: &'a str,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    column![
        text(title).size(13).style(theme::muted_text),
        content.into()
    ]
    .spacing(10)
    .into()
}

/// A row of buttons of which exactly one is selected, such as
/// System / Light / Dark. `icon_for` gives each option's icon, if it has one.
pub fn segmented<'a, T, Message>(
    options: &[T],
    selected: T,
    icon_for: impl Fn(T) -> Option<Icon>,
    on_select: impl Fn(T) -> Message,
) -> Element<'a, Message>
where
    T: Copy + PartialEq + ToString,
    Message: Clone + 'a,
{
    let buttons = options.iter().map(|&option| {
        let mut label = row![].spacing(8).align_y(Alignment::Center);
        if let Some(icon) = icon_for(option) {
            label = label.push(icons::icon(icon));
        }

        button(label.push(text(option.to_string())))
            .padding([6, 14])
            .style(theme::segment(option == selected))
            .on_press(on_select(option))
            .into()
    });

    container(row(buttons).spacing(2))
        .padding(3)
        .style(theme::segmented_control)
        .into()
}

/// Buttons of which exactly one is picked, in rows that wrap when the
/// space runs out, for choices too many or too long for [`segmented`].
pub fn chips<'a, T, Message>(
    options: &[T],
    selected: T,
    label: impl Fn(T) -> String,
    on_select: impl Fn(T) -> Message,
) -> Element<'a, Message>
where
    T: Copy + PartialEq,
    Message: Clone + 'a,
{
    let buttons = options.iter().map(|&option| {
        button(text(label(option)).size(13))
            .padding([6, 12])
            .style(theme::chip(option == selected))
            .on_press(on_select(option))
            .into()
    });

    row(buttons).spacing(6).wrap().into()
}

/// Like [`chips`], in rows of `columns` chips that share each row's width
/// equally, so a choice too long for one row breaks into an even grid
/// rather than a ragged second row.
pub fn chip_grid<'a, T, Message>(
    options: &[T],
    columns: usize,
    selected: T,
    label: impl Fn(T) -> String,
    on_select: impl Fn(T) -> Message,
) -> Element<'a, Message>
where
    T: Copy + PartialEq,
    Message: Clone + 'a,
{
    let rows = options.chunks(columns.max(1)).map(|chunk| {
        let mut cells = row![].spacing(6);
        for &option in chunk {
            cells = cells.push(
                button(
                    text(label(option))
                        .size(13)
                        .align_x(Alignment::Center)
                        .width(Fill),
                )
                .padding([6, 12])
                .width(Fill)
                .style(theme::chip(option == selected))
                .on_press(on_select(option)),
            );
        }
        for _ in chunk.len()..columns {
            cells = cells.push(iced::widget::space().width(Fill));
        }
        cells.into()
    });

    column(rows).spacing(6).into()
}

/// The bar along the top of a screen, with a thin line under it.
pub fn top_bar<'a, Message: 'a>(content: iced::widget::Row<'a, Message>) -> Element<'a, Message> {
    column![bar_body(content), divider()].into()
}

/// The inside of the top bar: `content` in a row, centered vertically, on
/// the bar's background.
fn bar_body<'a, Message: 'a>(content: iced::widget::Row<'a, Message>) -> Element<'a, Message> {
    container(content.spacing(4).align_y(Alignment::Center))
        .padding([6, 10])
        .width(Fill)
        .style(theme::top_bar)
        .into()
}

/// A thin line across the width it gets, such as under the top bar or
/// between the rows of a group.
pub fn divider<'a, Message: 'a>() -> Element<'a, Message> {
    container(iced::widget::space())
        .width(Fill)
        .height(1)
        .style(theme::divider)
        .into()
}

/// A picture of the Game Boy screen, `width` by `height` pixels (the screen
/// alone or a Super Game Boy's picture with its border), grown by the
/// largest whole factor that fits in the space it gets, so every Game Boy
/// pixel is the same number of screen pixels.
pub fn whole_step_screen<'a, Message: 'a>(
    handle: image::Handle,
    (width, height): (u32, u32),
) -> Element<'a, Message> {
    responsive(move |available| {
        let factor = whole_step_factor(available, width, height);

        center(
            image(handle.clone())
                .filter_method(image::FilterMethod::Nearest)
                .width(width as f32 * factor)
                .height(height as f32 * factor),
        )
        .into()
    })
    .into()
}

/// A picture of the Game Boy screen, `width` by `height` pixels, in a frame
/// `border` wide drawn with `style` that hugs it, in the middle of the space
/// it gets, such as a library card's. The screen grows in whole steps like
/// [`whole_step_screen`]; where even its own size doesn't fit, it shrinks to
/// fit instead of being cut off.
pub fn framed_screen<'a, Message: 'a>(
    handle: image::Handle,
    (width, height): (u32, u32),
    border: f32,
    style: fn(&iced::Theme) -> container::Style,
) -> Element<'a, Message> {
    responsive(move |available| {
        let inside = Size::new(
            (available.width - 2.0 * border).max(0.0),
            (available.height - 2.0 * border).max(0.0),
        );
        let factor = whole_step_or_fitting_factor(inside, width, height);

        center(
            container(
                image(handle.clone())
                    .filter_method(image::FilterMethod::Nearest)
                    .width(width as f32 * factor)
                    .height(height as f32 * factor),
            )
            .padding(border)
            .style(style),
        )
        .into()
    })
    .into()
}

/// The largest whole factor a picture `width` by `height` pixels can grow by
/// in `available`, or, where even its own size doesn't fit, the factor
/// below 1 that shrinks it to fit.
fn whole_step_or_fitting_factor(available: Size, width: u32, height: u32) -> f32 {
    let fitting = (available.width / width as f32).min(available.height / height as f32);
    if fitting >= 1.0 {
        fitting.floor()
    } else {
        fitting
    }
}

/// The largest whole factor a picture `width` by `height` pixels can grow by
/// in `available`, and at least 1.
fn whole_step_factor(available: Size, width: u32, height: u32) -> f32 {
    (available.width / width as f32)
        .min(available.height / height as f32)
        .floor()
        .max(1.0)
}

/// One line of text that takes the width it is given and ends in "…" when
/// the text doesn't fit, such as a long game title in the top bar.
pub fn truncated_text<'a, Message: 'a>(
    content: String,
    size: f32,
    font: Font,
) -> Element<'a, Message> {
    container(responsive(move |bounds| {
        text(truncate(&content, bounds.width, size))
            .size(size)
            .font(font)
            .wrapping(Wrapping::None)
            .into()
    }))
    .width(Fill)
    .height(size * 1.4)
    .clip(true)
    .into()
}

/// Shortens `content` with "…" so it fits in `width` at font size `size`.
/// Iced can't measure text before laying it out, so this estimates the width
/// from an average character width.
fn truncate(content: &str, width: f32, size: f32) -> String {
    let fitting = (width / (size * AVERAGE_CHARACTER_WIDTH)).floor().max(0.0) as usize;

    if content.chars().count() <= fitting {
        return content.to_owned();
    }

    let mut shortened: String = content.chars().take(fitting.saturating_sub(1)).collect();
    shortened.truncate(shortened.trim_end().len());
    shortened.push('…');
    shortened
}

#[cfg(test)]
mod tests;
