//! The library screen, shown while no game is running: the games in the
//! library folders as cards, with the recently played ones on top. Without
//! any games yet it welcomes the user and offers to add a folder.

use crate::app::{
    GameMessage, LibraryMessage, Message, SCREEN_HEIGHT, SCREEN_WIDTH, SettingsMessage,
    WindowMessage,
};
use crate::settings::{LibrarySort, Settings};
use crate::ui::icons::{self, Icon, icon};
use crate::ui::logo::logo;
use crate::ui::pictures;
use crate::ui::theme;
use crate::ui::theme::Badge;
use crate::ui::widgets::{
    bar_button, icon_button, keycap, primary_button, secondary_button, shortcut, top_bar,
    truncated_text, whole_step_screen, with_tooltip,
};
use crate::views::settings::SettingsTab;
use iced::widget::{
    Column, Row, button, center, column, container, grid, hover, image, mouse_area, responsive,
    row, scrollable, space, text, text_input, tooltip,
};
use iced::{Alignment, ContentFit, Element, Fill, widget};
use nubo_frontend::controls::{Action, Device};
use nubo_frontend::library::{self, Library, LibraryEntry};
use nubo_frontend::rom::Rom;
use nubo_frontend::save_states;
use nubo_frontend::time::{Duration, SystemTime};
use nubo_frontend::updates;
use std::cmp::Reverse;

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

/// Where the welcome screen sends players who have no games yet.
const HOMEBREW_HUB: &str = "https://hh.gbdev.io";

/// How many games "Continue playing" shows.
pub const RECENT_LIMIT: usize = 4;
/// Cards are at most this wide; the grid fits as many columns as it can.
const CARD_MAX_WIDTH: f32 = 210.0;
const CARD_SPACING: f32 = 16.0;
/// A card's width to its height.
const CARD_ASPECT: (f32, f32) = (160.0, 196.0);
const CONTENT_MAX_WIDTH: f32 = 1100.0;
const PAGE_PADDING: (f32, f32) = (20.0, 24.0);
const SECTION_SPACING: f32 = 14.0;
/// The search field in the top bar is at most this wide.
const SEARCH_MAX_WIDTH: f32 = 260.0;
/// The room kept next to the search field for its clear button.
const CLEAR_WIDTH: f32 = 34.0;
/// The height of the search field: its text, padding and border.
const SEARCH_HEIGHT: f32 = 34.0;
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
    /// The cards shown for `search`, with all games in the order and of the
    /// kind the settings pick. "Continue playing" hides while searching.
    pub fn new(library: &'a Library, settings: &Settings, search: &'a str) -> Shelves<'a> {
        let recent = if search.is_empty() {
            library.recently_played(RECENT_LIMIT)
        } else {
            Vec::new()
        };
        let mut games: Vec<&LibraryEntry> = if settings.shared.library_folders.is_empty() {
            Vec::new()
        } else {
            library
                .games(search)
                .filter(|entry| settings.library_filter.allows(entry.supports_cgb))
                .collect()
        };
        // The library lists its games by name; sorting keeps that order
        // among games played or added at the same time.
        match settings.library_sort {
            LibrarySort::Name => {}
            LibrarySort::RecentlyPlayed => games.sort_by_key(|entry| Reverse(entry.last_played)),
            LibrarySort::RecentlyAdded => games.sort_by_key(|entry| Reverse(entry.added())),
        }

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
    /// between the two shelves at their edges. At the ends, `wrap` goes
    /// around (see [`Shelves::wrapped`]); without it, the selection stays.
    pub fn step(&self, index: usize, direction: Action, window_width: f32, wrap: bool) -> usize {
        let columns = columns(window_width);
        let target = self.step_within(index, direction, columns);
        if wrap && target == index {
            self.wrapped(index, direction, columns)
        } else {
            target
        }
    }

    /// The card next to `index` in `direction` in a grid `columns` wide,
    /// or `index` itself at the edge of the cards.
    fn step_within(&self, index: usize, direction: Action, columns: usize) -> usize {
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

    /// Where a press in `direction` goes from `index`, a card at the edge
    /// of the cards in a grid `columns` wide: left from the first card to
    /// the last and right from the last to the first, up from the top row
    /// to the bottom row and down from the bottom row to the top row, in
    /// the same column or the nearest card in it.
    fn wrapped(&self, index: usize, direction: Action, columns: usize) -> usize {
        if self.len() == 0 {
            return 0;
        }
        let recent = self.recent.len();
        let column = if index < recent {
            index % columns
        } else {
            (index - recent) % columns
        };
        // The first and last shelves, as their first card and their length.
        let first = if recent > 0 {
            (0, recent)
        } else {
            (recent, self.games.len())
        };
        let last = if self.games.is_empty() {
            (0, recent)
        } else {
            (recent, self.games.len())
        };

        match direction {
            Action::Left => self.len().saturating_sub(1),
            Action::Right => 0,
            Action::Up => {
                let (start, length) = last;
                let bottom_row = start + (length - 1) / columns * columns;
                (bottom_row + column).min(start + length - 1)
            }
            Action::Down => {
                let (start, length) = first;
                (start + column).min(start + length - 1)
            }
            _ => index,
        }
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
    controllers: &'a [String],
    remembered: Option<&'a Rom>,
    top: TopBar<'a>,
) -> Element<'a, Message> {
    let shelves = Shelves::new(library, settings, search);
    let has_folders = !settings.shared.library_folders.is_empty();

    if !library::AVAILABLE || (!has_folders && library.recently_played(1).is_empty()) {
        return column![bar(None, top), welcome(settings, controllers, remembered)].into();
    }

    let mut content = Column::new().spacing(SECTION_SPACING);
    let recent_count = shelves.recent.len();

    if !shelves.recent.is_empty() {
        content = content
            .push(heading("Continue playing", None, false, None))
            .push(card_grid(
                library,
                shelves.recent,
                settings.shared.download_box_art,
                0,
                selection,
                true,
            ))
            .push(space().height(SHELF_GAP));
    }

    let games = shelves.games;
    content = content.push(heading(
        settings.library_filter.label(),
        Some(games.len()),
        library.is_scanning(),
        Some(sort_button(settings)),
    ));

    content = if !has_folders {
        content.push(notice(
            "Add the folder with your games to see them all here.",
            Some(secondary_button(
                Icon::FolderPlus,
                "Add games folder",
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
            false,
        ))
    } else if !search.is_empty() {
        content.push(notice("No games match your search.", None))
    } else if library.is_scanning() {
        content.push(notice("Looking for games…", None))
    } else {
        content.push(notice(
            "No games found in your game folders.",
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

    column![bar(Some(search), top), page].into()
}

/// What the library's top bar shows besides the search and its buttons.
#[derive(Debug, Clone, Copy)]
pub struct TopBar<'a> {
    /// A newer Nubo, if one is out.
    pub newer_version: Option<&'a str>,
    /// Whether the window fills the screen, which shows a button to leave it.
    pub fullscreen: bool,
}

/// The top bar: with games to search, a search field and Open game; on the
/// welcome screen, whose own buttons open games, only the settings on the
/// right. With a newer version out, a small note says so and opens its page,
/// and a fullscreen window gets a button back to a window.
fn bar<'a>(search: Option<&'a str>, top: TopBar<'a>) -> Element<'a, Message> {
    let mut bar = row![brand()];
    bar = match search {
        Some(search) => bar.push(search_field(search)).push(bar_button(
            Icon::FolderOpen,
            "Open game",
            format!("Open a game file ({})", shortcut("O")),
            false,
            Message::Game(GameMessage::OpenRom),
        )),
        None => bar.push(space::horizontal()),
    };
    if let Some(version) = top.newer_version {
        bar = bar.push(space().width(6)).push(
            button(
                row![
                    icon(Icon::Sparkles).size(14),
                    text(format!("Nubo {version} is out")).size(13),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([4, 10])
            .style(theme::chip(true))
            .on_press(Message::OpenLink(updates::RELEASES_PAGE)),
        );
    }

    if top.fullscreen {
        bar = bar.push(space().width(6)).push(icon_button(
            Icon::Minimize,
            String::from("Exit fullscreen (F11)"),
            Some(Message::Window(WindowMessage::ToggleFullscreen)),
        ));
    }

    top_bar(bar.push(space().width(6)).push(icon_button(
        Icon::Settings,
        format!("Settings ({})", shortcut(",")),
        Some(Message::Settings(SettingsMessage::Open)),
    )))
}

/// The search field, filling the space between the app's name and the
/// buttons on the right, up to [`SEARCH_MAX_WIDTH`] wide, so it shrinks in a
/// narrow window instead of pushing the buttons out.
fn search_field(search: &str) -> Element<'_, Message> {
    let field = responsive(move |bounds| {
        let clear: Element<'_, Message> = if search.is_empty() {
            space().into()
        } else {
            icon_button(
                Icon::X,
                String::from("Clear search"),
                Some(Message::Library(LibraryMessage::SearchChanged(
                    String::new(),
                ))),
            )
        };
        let input = text_input("Search games", search)
            .on_input(|search| Message::Library(LibraryMessage::SearchChanged(search)))
            .icon(text_input::Icon {
                font: icons::FONT,
                code_point: char::from(Icon::Search),
                size: Some(15.into()),
                spacing: 8.0,
                side: text_input::Side::Left,
            })
            .padding([6, 10])
            .width((bounds.width - CLEAR_WIDTH).clamp(0.0, SEARCH_MAX_WIDTH))
            .style(theme::search_field);
        row![input, clear]
            .spacing(4)
            .align_y(Alignment::Center)
            .into()
    });

    container(field)
        .padding([0, 12])
        .width(Fill)
        .height(SEARCH_HEIGHT)
        .into()
}

/// The app's name and logo, at the left of the top bar.
fn brand<'a>() -> Element<'a, Message> {
    row![logo(22.0), text("Nubo").size(16).font(theme::HEADING_FONT),]
        .spacing(8)
        .padding([0, 6])
        .align_y(Alignment::Center)
        .into()
}

/// The first screen a new user sees: what to do first, where to get free
/// games, and which keys (or, with one connected, which controller buttons)
/// press the Game Boy's buttons.
fn welcome<'a>(
    settings: &'a Settings,
    controllers: &'a [String],
    remembered: Option<&'a Rom>,
) -> Element<'a, Message> {
    let actions: Element<'_, Message> = if let Some(rom) = remembered {
        column![
            primary_button(
                Icon::Play,
                format!("Play {} again", rom.name()),
                Message::Game(GameMessage::PlayRemembered),
            ),
            secondary_button(
                Icon::FolderOpen,
                "Open another game",
                Message::Game(GameMessage::OpenRom)
            ),
        ]
        .spacing(10)
        .align_x(Alignment::Center)
        .into()
    } else if library::AVAILABLE {
        row![
            primary_button(
                Icon::FolderPlus,
                "Add games folder",
                Message::Library(LibraryMessage::AddFolder)
            ),
            secondary_button(
                Icon::FolderOpen,
                "Open game",
                Message::Game(GameMessage::OpenRom)
            ),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
    } else {
        primary_button(
            Icon::FolderOpen,
            "Open game",
            Message::Game(GameMessage::OpenRom),
        )
    };

    let description = if library::AVAILABLE {
        "Add the folder with your Game Boy games, or open a single game."
    } else {
        "Open a Game Boy or Game Boy Color game to start."
    };

    let free_games = button(
        text("No games yet? Get free ones at Homebrew Hub")
            .size(13)
            .style(theme::accent_text),
    )
    .padding([2, 4])
    .style(theme::text_button)
    .on_press(Message::OpenLink(HOMEBREW_HUB));

    let welcome = column![
        logo(88.0),
        text("Welcome to Nubo").size(26).font(theme::HEADING_FONT),
        text(description).style(theme::muted_text),
        space().height(8),
        actions,
        drop_hint(),
        free_games,
        space().height(20),
        controls_help(settings, controllers),
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

/// Keycaps showing which keys press which Game Boy buttons, or with a
/// controller connected, its name and which of its buttons do. Buttons
/// without a key are left out.
fn controls_help<'a>(settings: &'a Settings, controllers: &'a [String]) -> Element<'a, Message> {
    let device = if controllers.is_empty() {
        Device::Keyboard
    } else {
        Device::Controller
    };
    let controls = &settings.shared.controls;

    let groups = CONTROLS.iter().filter_map(|(actions, button)| {
        let mut keys: Vec<String> = actions
            .iter()
            .filter_map(|&action| controls.input_label(device, action))
            .collect();
        if keys.is_empty() {
            return None;
        }
        // A controller's D-pad says "D-pad" once, not once per direction.
        if device == Device::Controller && keys.iter().all(|key| key.starts_with("D-pad")) {
            keys = vec![String::from("D-pad")];
        }

        let keycaps = Row::with_children(keys.into_iter().map(keycap)).spacing(3);

        Some(
            row![keycaps, text(*button).size(13).style(theme::muted_text)]
                .spacing(6)
                .align_y(Alignment::Center)
                .into(),
        )
    });

    let legend = Row::with_children(groups)
        .spacing(20)
        .align_y(Alignment::Center)
        .wrap();

    match controllers.first() {
        Some(name) => column![
            row![
                icon(Icon::Gamepad2).size(16).style(theme::accent_text),
                text(format!("{name} is ready")).size(13),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            legend,
        ]
        .spacing(10)
        .align_x(Alignment::Center)
        .into(),
        None => legend.into(),
    }
}

/// A section title, with the number of games and a scanning label if given.
fn heading<'a>(
    title: &'a str,
    count: Option<usize>,
    scanning: bool,
    action: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut heading = row![text(title).size(17).font(theme::HEADING_FONT)]
        .spacing(10)
        .align_y(Alignment::Center);

    if let Some(count) = count {
        heading = heading.push(text(count.to_string()).size(14).style(theme::muted_text));
    }

    heading = heading.push(space::horizontal());
    if scanning {
        heading = heading.push(
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
    if let Some(action) = action {
        heading = heading.push(action);
    }

    heading.height(HEADING_HEIGHT).into()
}

/// The button next to the list of all games that opens the menu to sort
/// and filter it, showing the order in use.
fn sort_button<'a>(settings: &Settings) -> Element<'a, Message> {
    button(
        row![
            icon(Icon::ArrowDownUp).size(14),
            text(settings.library_sort.label()).size(13),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([3, 10])
    .style(theme::secondary_button)
    .on_press(Message::Library(LibraryMessage::OpenSortMenu))
    .into()
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
/// find the selected one. With `recent`, for "Continue playing", each card
/// says when the game was played.
fn card_grid<'a>(
    library: &'a Library,
    games: Vec<&'a LibraryEntry>,
    show_box_art: bool,
    first_index: usize,
    selection: Option<usize>,
    recent: bool,
) -> Element<'a, Message> {
    grid(games.into_iter().enumerate().map(|(index, entry)| {
        let card = card(
            entry,
            CardPicture::of(library, entry, show_box_art),
            selection == Some(first_index + index),
            recent,
        );
        let card = mouse_area(card).on_right_press(Message::Library(LibraryMessage::OpenGameMenu(
            entry.path.clone(),
        )));
        hover(card, menu_button(entry))
    }))
    .fluid(CARD_MAX_WIDTH)
    .spacing(CARD_SPACING)
    .height(grid::aspect_ratio(CARD_ASPECT.0, CARD_ASPECT.1))
    .into()
}

/// A game's card: a picture of the game (or a placeholder), its name and
/// badges for the consoles it's made for, and with `recent` when it was
/// played. Clicking it starts the game; a right click opens its menu.
fn card(
    entry: &LibraryEntry,
    picture: CardPicture,
    selected: bool,
    recent: bool,
) -> Element<'_, Message> {
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

    let badge = |label, kind| {
        container(text(label).size(10))
            .padding([1, 6])
            .style(theme::badge(kind))
    };
    let mut caption = row![truncated_text(entry.name(), 14.0, theme::BODY_FONT)]
        .spacing(6)
        .align_y(Alignment::Center);
    if entry.supports_sgb == Some(true) {
        caption = caption.push(badge("SGB", Badge::SuperGameBoy));
    }
    caption = caption.push(if entry.supports_cgb {
        badge("GBC", Badge::Color)
    } else {
        badge("GB", Badge::GameBoy)
    });

    let mut content = column![picture, caption].spacing(8);
    if recent && let Some(played) = entry.last_played {
        content = content.push(
            text(save_states::time_ago(
                SystemTime::UNIX_EPOCH + Duration::from_secs(played),
            ))
            .size(12)
            .style(theme::muted_text),
        );
    }

    button(content.spacing(6))
        .padding(8)
        .width(Fill)
        .height(Fill)
        .style(theme::selectable_card(selected))
        .on_press(Message::Game(GameMessage::PlayGame(entry.path.clone())))
        .into()
}

/// The button in the corner of a card that opens the game's menu, shown
/// while the pointer is over the card.
fn menu_button(entry: &LibraryEntry) -> Element<'_, Message> {
    let button = button(icon(Icon::Ellipsis).size(14))
        .padding(4)
        .style(theme::card_corner_button)
        .on_press(Message::Library(LibraryMessage::OpenGameMenu(
            entry.path.clone(),
        )));

    container(with_tooltip(
        button,
        String::from("More (right click)"),
        tooltip::Position::Bottom,
    ))
    .padding(14)
    .width(Fill)
    .align_x(Alignment::End)
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
