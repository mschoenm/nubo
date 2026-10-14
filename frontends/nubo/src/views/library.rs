//! The library screen, shown while no game is running: the games in the
//! library folders as cards, with the recently played ones on top. Without
//! any games yet it welcomes the user and offers to add a folder.

use crate::app::{
    GameMessage, LibraryMessage, Message, SCREEN_HEIGHT, SCREEN_WIDTH, SettingsMessage,
};
use crate::settings::Settings;
use crate::ui::icons::{self, Icon, icon};
use crate::ui::logo::logo;
use crate::ui::pictures;
use crate::ui::theme;
use crate::ui::widgets::{
    icon_button, keycap, primary_button, secondary_button, shortcut, top_bar, truncated_text,
    whole_step_screen,
};
use crate::views::settings::SettingsTab;
use iced::widget::{
    Column, Row, button, center, column, container, grid, image, row, scrollable, space, text,
    text_input,
};
use iced::{Alignment, ContentFit, Element, Fill, widget};
use nubo_frontend::controls::Action;
use nubo_frontend::library::{self, Library, LibraryEntry};

/// The Game Boy controls the welcome screen explains, and the actions whose
/// keys press them.
const CONTROLS: [(&[Action], &str); 5] = [
    (
        &[Action::Up, Action::Down, Action::Left, Action::Right],
        "D-pad",
    ),
    (&[Action::A], "A"),
    (&[Action::B], "B"),
    (&[Action::Start], "Start"),
    (&[Action::Select], "Select"),
];

/// How many games "Continue playing" shows.
const RECENT_LIMIT: usize = 4;
/// Cards are at most this wide; the grid fits as many columns as it can.
const CARD_MAX_WIDTH: f32 = 210.0;
const CARD_SPACING: f32 = 16.0;
/// A card's width to its height.
const CARD_ASPECT: (f32, f32) = (160.0, 196.0);
const CONTENT_MAX_WIDTH: f32 = 1100.0;
const PAGE_PADDING: (f32, f32) = (20.0, 24.0);
const SECTION_SPACING: f32 = 14.0;
/// The space above "All games" when "Continue playing" comes first.
const SHELF_GAP: f32 = 12.0;
/// The height of a section heading, at 17 points.
const HEADING_HEIGHT: f32 = 22.0;

/// The width of a window that fits a whole "Continue playing" row of
/// full-size cards side by side, used as the window's starting width.
pub const FULL_ROW_WINDOW_WIDTH: f32 = RECENT_LIMIT as f32 * CARD_MAX_WIDTH
    + (RECENT_LIMIT - 1) as f32 * CARD_SPACING
    + 2.0 * PAGE_PADDING.1;

/// The cards the library shows, in the order the D-pad moves through them:
/// the recently played games, then all games.
pub struct Shelves<'a> {
    pub recent: Vec<&'a LibraryEntry>,
    pub games: Vec<&'a LibraryEntry>,
}

impl<'a> Shelves<'a> {
    /// The cards shown for `search`. "Continue playing" hides while
    /// searching.
    pub fn new(library: &'a Library, settings: &Settings, search: &'a str) -> Shelves<'a> {
        let recent = if search.is_empty() {
            library.recently_played(RECENT_LIMIT)
        } else {
            Vec::new()
        };
        let games = if settings.shared.library_folders.is_empty() {
            Vec::new()
        } else {
            library.games(search).collect()
        };

        Shelves { recent, games }
    }

    pub fn len(&self) -> usize {
        self.recent.len() + self.games.len()
    }

    /// The game of the card at `index`, counting across both shelves.
    pub fn entry(&self, index: usize) -> Option<&'a LibraryEntry> {
        self.recent
            .get(index)
            .or_else(|| self.games.get(index.checked_sub(self.recent.len())?))
            .copied()
    }

    /// The card the D-pad moves to from `index` in `direction`, in a
    /// window `window_width` wide. Up and down move between rows, and
    /// between the two shelves at their edges.
    pub fn step(&self, index: usize, direction: Action, window_width: f32) -> usize {
        let columns = columns(window_width);
        let recent = self.recent.len();
        let last = self.len().saturating_sub(1);

        let target = match direction {
            Action::Left => index.saturating_sub(1),
            Action::Right => index + 1,
            Action::Up if index < recent => index.checked_sub(columns).unwrap_or(index),
            Action::Up => match (index - recent).checked_sub(columns) {
                Some(above) => recent + above,
                None if recent > 0 => {
                    let last_row_start = (recent - 1) / columns * columns;
                    (last_row_start + (index - recent) % columns).min(recent - 1)
                }
                None => index,
            },
            Action::Down if index < recent => {
                if index + columns < recent {
                    index + columns
                } else {
                    recent + index % columns
                }
            }
            Action::Down => {
                let in_games = index - recent;
                let rows = self.games.len().div_ceil(columns);
                if in_games / columns + 1 < rows {
                    recent + in_games + columns
                } else {
                    index
                }
            }
            _ => index,
        };

        target.min(last)
    }

    /// The top and bottom of the card at `index` in the scrolling page,
    /// estimated from the layout, so the app can scroll it into view.
    pub fn card_bounds(&self, index: usize, window_width: f32) -> (f32, f32) {
        let columns = columns(window_width);
        let row_height = card_height(window_width) + CARD_SPACING;
        let shelf_top = PAGE_PADDING.0 + HEADING_HEIGHT + SECTION_SPACING;

        let top = if index < self.recent.len() {
            shelf_top + (index / columns) as f32 * row_height
        } else {
            let recent_rows = self.recent.len().div_ceil(columns) as f32;
            let games_top = if self.recent.is_empty() {
                shelf_top
            } else {
                shelf_top
                    + recent_rows * row_height
                    + SECTION_SPACING * 2.0
                    + SHELF_GAP
                    + HEADING_HEIGHT
            };
            games_top + ((index - self.recent.len()) / columns) as f32 * row_height
        };

        (top, top + card_height(window_width))
    }
}

/// The width the cards share in a window `window_width` wide.
fn content_width(window_width: f32) -> f32 {
    (window_width - PAGE_PADDING.1 * 2.0).clamp(CARD_MAX_WIDTH, CONTENT_MAX_WIDTH)
}

/// How many cards fit side by side, the way Iced's grid works it out.
fn columns(window_width: f32) -> usize {
    (((content_width(window_width) + CARD_SPACING) / (CARD_MAX_WIDTH + CARD_SPACING)).ceil()
        as usize)
        .max(1)
}

fn card_height(window_width: f32) -> f32 {
    let columns = columns(window_width) as f32;
    let width = (content_width(window_width) - CARD_SPACING * (columns - 1.0)) / columns;
    width * CARD_ASPECT.1 / CARD_ASPECT.0
}

/// The scrolling page of the library, so the app can scroll it.
pub fn scroll_id() -> widget::Id {
    widget::Id::new("library")
}

/// The library screen. `selection` is the card the D-pad is on, if the
/// player has used it.
pub fn view<'a>(
    library: &'a Library,
    settings: &'a Settings,
    search: &'a str,
    selection: Option<usize>,
) -> Element<'a, Message> {
    let shelves = Shelves::new(library, settings, search);
    let has_folders = !settings.shared.library_folders.is_empty();

    if !library::AVAILABLE || (!has_folders && library.recently_played(1).is_empty()) {
        return column![bar(None), welcome(settings)].into();
    }

    let mut content = Column::new().spacing(SECTION_SPACING);
    let recent_count = shelves.recent.len();

    if !shelves.recent.is_empty() {
        content = content
            .push(heading("Continue playing", None, false))
            .push(card_grid(
                library,
                shelves.recent,
                settings.shared.download_box_art,
                0,
                selection,
            ))
            .push(space().height(SHELF_GAP));
    }

    let games = shelves.games;
    content = content.push(heading(
        "All games",
        Some(games.len()),
        library.is_scanning(),
    ));

    content = if !has_folders {
        content.push(notice(
            "Add a ROM folder to see all your games here.",
            Some(secondary_button(
                Icon::FolderPlus,
                "Add ROM folder",
                Message::Library(LibraryMessage::AddFolder),
            )),
        ))
    } else if !games.is_empty() {
        content.push(card_grid(
            library,
            games,
            settings.shared.download_box_art,
            recent_count,
            selection,
        ))
    } else if !search.is_empty() {
        content.push(notice("No games match your search.", None))
    } else if library.is_scanning() {
        content.push(notice("Looking for games…", None))
    } else {
        content.push(notice(
            "No games found in your ROM folders.",
            Some(secondary_button(
                Icon::FolderCog,
                "Manage folders",
                Message::Settings(SettingsMessage::TabSelected(SettingsTab::Library)),
            )),
        ))
    };

    let page = container(content.max_width(CONTENT_MAX_WIDTH))
        .padding([PAGE_PADDING.0, PAGE_PADDING.1])
        .center_x(Fill);

    let page = scrollable(page)
        .id(scroll_id())
        .on_scroll(|viewport| {
            Message::Library(LibraryMessage::Scrolled(
                viewport.absolute_offset().y,
                viewport.bounds().height,
            ))
        })
        .height(Fill);

    column![bar(Some(search)), page].into()
}

/// The top bar, with a search field when there are games to search.
fn bar(search: Option<&str>) -> Element<'_, Message> {
    let mut bar = row![brand()];

    if let Some(search) = search {
        bar = bar.push(space().width(12)).push(
            text_input("Search games", search)
                .on_input(|search| Message::Library(LibraryMessage::SearchChanged(search)))
                .icon(text_input::Icon {
                    font: icons::FONT,
                    code_point: char::from(Icon::Search),
                    size: Some(15.into()),
                    spacing: 8.0,
                    side: text_input::Side::Left,
                })
                .padding([6, 10])
                .width(260)
                .style(theme::search_field),
        );
    }

    let mut bar = bar.push(space::horizontal());
    // The library screen with games gets a button to scan the folders
    // again, for folders whose changes aren't noticed, such as network
    // shares.
    if search.is_some() {
        bar = bar.push(icon_button(
            Icon::RefreshCw,
            String::from("Rescan folders"),
            Some(Message::Library(LibraryMessage::Rescan)),
        ));
    }

    top_bar(bar.extend([
        icon_button(
            Icon::FolderOpen,
            format!("Open ROM ({})", shortcut("O")),
            Some(Message::Game(GameMessage::OpenRom)),
        ),
        icon_button(
            Icon::Settings,
            format!("Settings ({})", shortcut(",")),
            Some(Message::Settings(SettingsMessage::Open)),
        ),
    ]))
}

/// The app's name and logo, at the left of the top bar.
fn brand<'a>() -> Element<'a, Message> {
    row![logo(22.0), text("Nubo").size(16).font(theme::HEADING_FONT),]
        .spacing(8)
        .padding([0, 6])
        .align_y(Alignment::Center)
        .into()
}

/// The first screen a new user sees.
fn welcome(settings: &Settings) -> Element<'_, Message> {
    let actions: Element<'_, Message> = if library::AVAILABLE {
        row![
            primary_button(
                Icon::FolderPlus,
                "Add ROM folder",
                Message::Library(LibraryMessage::AddFolder)
            ),
            secondary_button(
                Icon::FolderOpen,
                "Open ROM",
                Message::Game(GameMessage::OpenRom)
            ),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
    } else {
        primary_button(
            Icon::FolderOpen,
            "Open ROM",
            Message::Game(GameMessage::OpenRom),
        )
    };

    let description = if library::AVAILABLE {
        "Add the folder with your Game Boy games, or open a single ROM."
    } else {
        "Open a Game Boy or Game Boy Color ROM to start."
    };

    let welcome = column![
        logo(88.0),
        text("Welcome to Nubo").size(26).font(theme::HEADING_FONT),
        text(description).style(theme::muted_text),
        space().height(8),
        actions,
        drop_hint(),
        space().height(28),
        controls_help(settings),
    ]
    .spacing(10)
    .align_x(Alignment::Center);

    container(welcome).center(Fill).padding(24).into()
}

/// Browsers don't let pages receive files dropped on the canvas, so the
/// hint is only shown on the desktop.
fn drop_hint<'a>() -> Element<'a, Message> {
    if cfg!(target_arch = "wasm32") {
        space().into()
    } else {
        text("or drop a .gb, .gbc or .zip file onto this window")
            .size(13)
            .style(theme::muted_text)
            .into()
    }
}

/// Keycaps showing which keys press which Game Boy buttons. Buttons without
/// a key are left out.
fn controls_help(settings: &Settings) -> Element<'_, Message> {
    let groups = CONTROLS.iter().filter_map(|(actions, button)| {
        let keys: Vec<_> = actions
            .iter()
            .filter_map(|&action| settings.shared.controls.keyboard.input(action))
            .collect();
        if keys.is_empty() {
            return None;
        }

        let keycaps =
            Row::with_children(keys.into_iter().map(|key| keycap(key.to_string()))).spacing(3);

        Some(
            row![keycaps, text(*button).size(13).style(theme::muted_text)]
                .spacing(6)
                .align_y(Alignment::Center)
                .into(),
        )
    });

    Row::with_children(groups)
        .spacing(20)
        .align_y(Alignment::Center)
        .wrap()
        .into()
}

/// A section title, with the number of games and a scanning label if given.
fn heading<'a>(title: &'a str, count: Option<usize>, scanning: bool) -> Element<'a, Message> {
    let mut heading = row![text(title).size(17).font(theme::HEADING_FONT)]
        .spacing(10)
        .align_y(Alignment::Center);

    if let Some(count) = count {
        heading = heading.push(text(count.to_string()).size(14).style(theme::muted_text));
    }

    if scanning {
        heading = heading.push(space::horizontal()).push(
            row![
                icons::icon(Icon::RefreshCw)
                    .size(13)
                    .style(theme::muted_text),
                text("Scanning…").size(13).style(theme::muted_text),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }

    heading.into()
}

/// A message in place of the game grid, with an optional button.
fn notice<'a>(message: &'a str, action: Option<Element<'a, Message>>) -> Element<'a, Message> {
    let mut notice = column![text(message).style(theme::muted_text)]
        .spacing(12)
        .align_x(Alignment::Center);

    if let Some(action) = action {
        notice = notice.push(action);
    }

    container(notice)
        .center_x(Fill)
        .padding(32)
        .style(theme::notice)
        .into()
}

/// The picture on a game's card.
#[derive(Clone)]
enum CardPicture {
    /// Box art downloaded for the game.
    BoxArt(image::Handle),
    /// A frame of the Game Boy screen: the title screen or the last screen
    /// played.
    Screen(image::Handle),
    /// Nothing yet.
    None,
}

impl CardPicture {
    /// The best picture of a game: its box art (if shown), then its title
    /// screen, then the last screen played.
    fn of(library: &Library, entry: &LibraryEntry, show_box_art: bool) -> CardPicture {
        if let Some(box_art) = library.box_art(&entry.path).filter(|_| show_box_art) {
            return CardPicture::BoxArt(pictures::handle(box_art));
        }

        library
            .title_screen(&entry.path)
            .or_else(|| library.thumbnail(&entry.path))
            .map_or(CardPicture::None, |screen| {
                CardPicture::Screen(pictures::handle(screen))
            })
    }
}

/// The games as a grid of cards that fits as many columns as the window
/// allows. `first_index` is the first card's place across both shelves, to
/// find the selected one.
fn card_grid<'a>(
    library: &'a Library,
    games: Vec<&'a LibraryEntry>,
    show_box_art: bool,
    first_index: usize,
    selection: Option<usize>,
) -> Element<'a, Message> {
    grid(games.into_iter().enumerate().map(|(index, entry)| {
        card(
            entry,
            CardPicture::of(library, entry, show_box_art),
            selection == Some(first_index + index),
        )
    }))
    .fluid(CARD_MAX_WIDTH)
    .spacing(CARD_SPACING)
    .height(grid::aspect_ratio(CARD_ASPECT.0, CARD_ASPECT.1))
    .into()
}

/// A game's card: a picture of the game (or a placeholder), its name and
/// whether it's a Game Boy or Game Boy Color game. Clicking it starts the game.
fn card(entry: &LibraryEntry, picture: CardPicture, selected: bool) -> Element<'_, Message> {
    let picture: Element<'_, Message> = match picture {
        CardPicture::BoxArt(handle) => image(handle)
            .content_fit(ContentFit::Contain)
            .width(Fill)
            .height(Fill)
            .border_radius(4)
            .into(),
        CardPicture::Screen(handle) => screen_picture(handle),
        CardPicture::None => center(icon(Icon::Gamepad2).size(36).style(theme::muted_text))
            .style(theme::thumbnail_placeholder)
            .into(),
    };

    let badge = container(text(if entry.supports_cgb { "GBC" } else { "GB" }).size(10))
        .padding([1, 6])
        .style(theme::badge(entry.supports_cgb));

    let caption = row![truncated_text(entry.name(), 14.0, theme::BODY_FONT), badge,]
        .spacing(8)
        .align_y(Alignment::Center);

    button(column![picture, caption].spacing(8))
        .padding(8)
        .width(Fill)
        .height(Fill)
        .style(theme::selectable_card(selected))
        .on_press(Message::Game(GameMessage::PlayGame(entry.path.clone())))
        .into()
}

/// A picture of the Game Boy screen on a dark frame, grown by the largest
/// whole factor that fits so every pixel is the same size, like the game
/// screen with sharp pixels on.
fn screen_picture<'a>(handle: image::Handle) -> Element<'a, Message> {
    container(whole_step_screen(handle, (SCREEN_WIDTH, SCREEN_HEIGHT)))
        .width(Fill)
        .height(Fill)
        .clip(true)
        .style(theme::card_screen)
        .into()
}

#[cfg(test)]
mod tests;
