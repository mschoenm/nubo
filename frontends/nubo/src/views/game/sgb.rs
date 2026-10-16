//! The pause menu's Super Game Boy page: the colors the game shows (its own,
//! one of Nubo's palettes, or the player's own, with an editor for those
//! that opens when asked) and the border around the screen. It stands in for
//! the menu a real Super Game Boy opens with the Super Nintendo's L and R
//! buttons (see `nubo_frontend::sgb_menu`).

use super::page_header;
use crate::app::{GameMessage, Message};
use crate::game::{Game, SgbBorderChoice};
use crate::settings::Settings;
use crate::ui::icons::{Icon, icon};
use crate::ui::theme;
use crate::ui::widgets::section;
use iced::widget::{Column, Row, button, column, container, row, slider, space, text};
use iced::{Alignment, Color, Element, Fill, Theme};
use nubo_core::SgbBorder;
use nubo_frontend::sgb_menu::{self, ColorChannel, SgbColors};

/// The names of the four shades of a palette, lightest first.
const SHADES: [&str; 4] = ["Lightest", "Light", "Dark", "Darkest"];

/// A row of the page the D-pad can be on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SgbRow {
    /// A choice of colors, which A picks.
    Colors(SgbColors),
    /// The button that shows or hides the editor for the player's own
    /// colors.
    EditOwnColors,
    /// A channel of one of the player's own colors (shade 0 to 3, lightest
    /// first), which left and right change.
    OwnColor(usize, ColorChannel),
    /// A choice of border, which A picks.
    Border(SgbBorderChoice),
}

/// The page's rows, in order: the colors, the button for the editor of the
/// player's own and, while it is open, the editor (all left out while the
/// game doesn't let the player change its colors), then the borders. The
/// view and the app's D-pad handling both use it.
pub fn rows(game: &Game) -> Vec<SgbRow> {
    let mut rows = Vec::new();
    if palettes_allowed(game) {
        rows.extend(SgbColors::ALL.map(SgbRow::Colors));
        rows.push(SgbRow::EditOwnColors);
    }
    if palettes_allowed(game) && game.sgb_editor_open {
        for shade in 0..SHADES.len() {
            rows.extend(
                ColorChannel::ALL
                    .into_iter()
                    .map(|channel| SgbRow::OwnColor(shade, channel)),
            );
        }
    }
    rows.extend(SgbBorderChoice::ALL.map(SgbRow::Border));
    rows
}

/// What A does on `row`.
pub fn pick(row: SgbRow) -> Option<Message> {
    match row {
        SgbRow::Colors(colors) => Some(GameMessage::SgbColorsPicked(colors).into()),
        SgbRow::Border(border) => Some(GameMessage::SgbBorderPicked(border).into()),
        SgbRow::EditOwnColors => Some(GameMessage::ToggleSgbEditor.into()),
        SgbRow::OwnColor(..) => None,
    }
}

/// What left (`step` -1) or right (`step` 1) does on `row`.
pub fn adjust(row: SgbRow, step: i8) -> Option<Message> {
    match row {
        SgbRow::OwnColor(shade, channel) => {
            Some(GameMessage::SgbOwnColorStepped(shade, channel, step).into())
        }
        SgbRow::Colors(_) | SgbRow::EditOwnColors | SgbRow::Border(_) => None,
    }
}

/// The border shown now: none when borders are switched off, and otherwise
/// the one the player picked.
pub fn current_border(game: &Game, settings: &Settings) -> SgbBorderChoice {
    if !settings.shared.sgb_border {
        return SgbBorderChoice::None;
    }
    match game.session.sgb_menu().map(|menu| menu.border) {
        Some(SgbBorder::Nubo) => SgbBorderChoice::Nubo,
        Some(SgbBorder::Game) | None => SgbBorderChoice::Game,
    }
}

/// Whether the game lets the player change its colors.
fn palettes_allowed(game: &Game) -> bool {
    game.session
        .sgb_menu()
        .is_some_and(|menu| menu.palettes_allowed)
}

/// The page, with the D-pad on row `selection` of [`rows`].
pub fn page<'a>(game: &'a Game, settings: &'a Settings, selection: usize) -> Element<'a, Message> {
    let rows = rows(game);
    let selected = |row: SgbRow| rows.get(selection) == Some(&row);
    let own = settings.shared.sgb_own_palette;

    let colors: Element<'a, Message> = if palettes_allowed(game) {
        let current = game
            .session
            .sgb_menu()
            .map_or(SgbColors::Game, |menu| menu.colors(own));
        let choices = SgbColors::ALL.map(|colors| {
            choice(
                colors.label(),
                colors.palette(own),
                colors == current,
                selected(SgbRow::Colors(colors)),
                GameMessage::SgbColorsPicked(colors).into(),
            )
        });
        let open = game.sgb_editor_open;
        let toggle = container(
            button(
                row![
                    icon(if open {
                        Icon::ChevronUp
                    } else {
                        Icon::SlidersHorizontal
                    })
                    .size(15),
                    text(if open {
                        "Hide the editor"
                    } else {
                        "Edit your own colors…"
                    })
                    .size(13),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([4, 8])
            .style(theme::text_button)
            .on_press(GameMessage::ToggleSgbEditor.into()),
        )
        .style(theme::focus_ring(selected(SgbRow::EditOwnColors)));

        let mut colors = column![Column::with_children(choices).spacing(2), toggle].spacing(10);
        if open {
            let editor = (0..SHADES.len()).map(|shade| own_color(own[shade], shade, &selected));
            colors = colors
                .push(
                    text("Your own colors, lightest first: changing one shows them.")
                        .size(12)
                        .style(theme::muted_text),
                )
                .push(Column::with_children(editor).spacing(8));
        }
        colors.into()
    } else {
        text("This game keeps its own colors.")
            .size(13)
            .style(theme::muted_text)
            .into()
    };

    let current_border = current_border(game, settings);
    let borders = SgbBorderChoice::ALL.map(|border| {
        choice(
            border.label(),
            None,
            border == current_border,
            selected(SgbRow::Border(border)),
            GameMessage::SgbBorderPicked(border).into(),
        )
    });

    column![
        page_header("Super Game Boy"),
        section("Colors", colors),
        section("Border", Column::with_children(borders).spacing(2)),
    ]
    .spacing(16)
    .into()
}

/// A choice on the page: a check mark when it is the one in use, its name,
/// and its colors if it has some. The row the D-pad is on is highlighted.
fn choice<'a>(
    label: &'static str,
    palette: Option<[u16; 4]>,
    in_use: bool,
    selected: bool,
    message: Message,
) -> Element<'a, Message> {
    let check: Element<'a, Message> = if in_use {
        icon(Icon::Check).into()
    } else {
        space().width(18).into()
    };
    let mut content = row![check, text(label).width(Fill)]
        .spacing(12)
        .align_y(Alignment::Center);
    if let Some(palette) = palette {
        content = content.push(swatches(palette, 14.0));
    }

    button(content)
        .padding([8, 12])
        .width(Fill)
        .style(theme::segment(selected))
        .on_press(message)
        .into()
}

/// The four colors of `palette` side by side, `size` pixels each.
fn swatches<'a>(palette: [u16; 4], size: f32) -> Element<'a, Message> {
    let swatches = Row::with_children(palette.map(|color| swatch(color, size, size)));
    container(swatches).style(theme::swatch_frame).into()
}

/// A box of `color`, `width` by `height` pixels.
fn swatch<'a>(color: u16, width: f32, height: f32) -> Element<'a, Message> {
    let [red, green, blue] = sgb_menu::rgb(color);
    container(space())
        .width(width)
        .height(height)
        .style(move |_: &Theme| container::Style {
            background: Some(Color::from_rgb8(red, green, blue).into()),
            ..container::Style::default()
        })
        .into()
}

/// The editor for one of the player's own colors: the color, its shade's
/// name, and a slider for each of its red, green and blue.
fn own_color<'a>(
    color: u16,
    shade: usize,
    selected: &impl Fn(SgbRow) -> bool,
) -> Element<'a, Message> {
    let sliders = ColorChannel::ALL.map(|channel| {
        let value = channel.of(color);
        let content = row![
            text(channel.label()).size(12).width(40),
            slider(0..=ColorChannel::MAX, value, move |value| {
                GameMessage::SgbOwnColorChanged(shade, channel, value).into()
            })
            .on_release(GameMessage::SgbOwnColorReleased.into()),
            text(value.to_string()).size(12).width(18),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        container(content)
            .padding([2, 8])
            .style(theme::selectable_row(selected(SgbRow::OwnColor(
                shade, channel,
            ))))
            .into()
    });

    row![
        column![
            container(swatch(color, 36.0, 36.0)).style(theme::swatch_frame),
            text(SHADES[shade]).size(11).style(theme::muted_text),
        ]
        .spacing(4)
        .width(56)
        .align_x(Alignment::Center),
        Column::with_children(sliders).spacing(2).width(Fill),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .into()
}
