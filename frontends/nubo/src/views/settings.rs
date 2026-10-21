//! The settings window, drawn over the current screen, with a tab per
//! group of settings. Every page is built the same way: titled groups of
//! rows in a rounded card, each row with its name and a short description
//! at the left and its control at the right (or under them, for a control
//! too wide to share the row), and any longer note under its group. The
//! window is as tall as its tallest page needs on every tab, so it keeps
//! its size from tab to tab, and no taller than the app's window, where
//! the page scrolls instead (see [`tallest`]). It arranges itself for the
//! window's width (see [`SettingsLayout`]): the tabs are in a sidebar in a
//! wide window and in a row above the page in a narrow one.

use crate::app::{GameMessage, LibraryMessage, Message, SettingsMessage};
use crate::settings::{Appearance, Settings};
use crate::ui::icons::{Icon, icon};
use crate::ui::logo::logo;
use crate::ui::tallest::tallest;
use crate::ui::theme;
use crate::ui::widgets::{chip_grid, chips, icon_button, segmented, with_tooltip};
use crate::version;
use iced::widget::text::Wrapping;
use iced::widget::{
    Column, Row, button, column, container, row, scrollable, slider, space, text, toggler, tooltip,
};
use iced::{Alignment, Color, Element, Fill, Padding, Shrink, Theme, widget};
use nubo_core::Model;
use nubo_frontend::controls::{Action, Controls, Device, MenuButtons};
use nubo_frontend::library::{self, Library, box_art};
use nubo_frontend::model::{self, Console, ModelPreference};
use nubo_frontend::palettes::DmgPalette;
use nubo_frontend::save_states;
use nubo_frontend::settings::{FastForwardSpeed, LibraryFolder};
use nubo_frontend::updates;

const WIDTH: f32 = 700.0;
/// The page of this version's release notes.
const RELEASE_NOTES: &str = concat!(
    env!("CARGO_PKG_REPOSITORY"),
    "/releases/tag/v",
    env!("CARGO_PKG_VERSION")
);
const SIDEBAR_WIDTH: f32 = 170.0;
/// The space between the settings window's edge and what it shows.
const PANEL_PADDING: f32 = 24.0;
/// The space between the tabs and the page.
const BODY_SPACING: f32 = 20.0;
/// The space around the tabs that the D-pad's outline takes.
const RING_PADDING: f32 = 4.0;
/// The width a page loses to its own padding and its scroll bar.
const PAGE_GUTTER: f32 = 30.0;
/// The least width a page needs next to the sidebar. In a narrower window
/// the tabs go in a row above the page instead.
const SIDEBAR_MIN_PAGE_WIDTH: f32 = 400.0;
/// The width the row of tabs needs to name every tab. In a narrower window
/// only the open tab shows its name, and the others only their icons.
const TAB_ROW_NAMED_WIDTH: f32 = 560.0;
/// Below this page width, controls too wide to share a row with their
/// setting's name go under it.
const STACK_BELOW_PAGE_WIDTH: f32 = 420.0;
/// The space inside a row of a group, around its text and control.
const ROW_PADDING: Padding = Padding {
    top: 10.0,
    right: 14.0,
    bottom: 10.0,
    left: 14.0,
};
/// The tighter space inside the rows of key and button bindings, which
/// come in long lists.
const BINDING_ROW_PADDING: Padding = Padding {
    top: 6.0,
    bottom: 6.0,
    ..ROW_PADDING
};
/// The least height of a group's title row, which grows to fit a button
/// next to the title.
const HEADING_HEIGHT: f32 = 28.0;
/// The Game Boy model buttons come in rows of this many, all the same width.
const MODEL_COLUMNS: usize = 3;
/// The width of the chips that show the key or button of an action.
const CHIP_WIDTH: f32 = 140.0;

/// A page of the settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    /// How the window and the game screen look.
    Display,
    /// The hardware games run on, continuing games, pausing in the
    /// background, fast forward and sound.
    Playing,
    Controls,
    Library,
    /// The version and new ones, starting over, how Nubo was made, the
    /// license and where to get the source code.
    About,
}

impl SettingsTab {
    /// The tabs to show. The browser build has no library to set up.
    pub fn all() -> &'static [SettingsTab] {
        if library::AVAILABLE {
            &[
                SettingsTab::Display,
                SettingsTab::Playing,
                SettingsTab::Controls,
                SettingsTab::Library,
                SettingsTab::About,
            ]
        } else {
            &[
                SettingsTab::Display,
                SettingsTab::Playing,
                SettingsTab::Controls,
                SettingsTab::About,
            ]
        }
    }

    fn label(self) -> &'static str {
        match self {
            SettingsTab::Display => "Display",
            SettingsTab::Playing => "Playing",
            SettingsTab::Controls => "Controls",
            SettingsTab::Library => "Library",
            SettingsTab::About => "About",
        }
    }

    fn icon(self) -> Icon {
        match self {
            SettingsTab::Display => Icon::Monitor,
            SettingsTab::Playing => Icon::CirclePlay,
            SettingsTab::Controls => Icon::Gamepad2,
            SettingsTab::Library => Icon::LibraryBig,
            SettingsTab::About => Icon::Info,
        }
    }
}

/// A choice on the model row: Automatic, or one of the consoles, whose
/// revision the row under it picks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelOption {
    Automatic,
    Console(Console),
}

impl ModelOption {
    /// Every choice, in the order the row shows them.
    pub const ALL: [ModelOption; 6] = [
        ModelOption::Automatic,
        ModelOption::Console(Console::GameBoy),
        ModelOption::Console(Console::Pocket),
        ModelOption::Console(Console::SuperGameBoy),
        ModelOption::Console(Console::Color),
        ModelOption::Console(Console::Advance),
    ];

    /// The choice `preference` falls under.
    pub fn of(preference: ModelPreference) -> ModelOption {
        match preference {
            ModelPreference::Automatic => ModelOption::Automatic,
            ModelPreference::Fixed(model) => ModelOption::Console(Console::of(model)),
        }
    }

    fn label(self) -> String {
        String::from(match self {
            ModelOption::Automatic => "Automatic",
            ModelOption::Console(console) => console.short_label(),
        })
    }

    /// The preference picking this choice gives: the revision `current`
    /// uses if it is of the same console, else the console's recommended
    /// one.
    pub fn preference(self, current: ModelPreference) -> ModelPreference {
        match self {
            ModelOption::Automatic => ModelPreference::Automatic,
            ModelOption::Console(console) => match current {
                ModelPreference::Fixed(model) if Console::of(model) == console => current,
                _ => ModelPreference::Fixed(console.recommended()),
            },
        }
    }
}

/// The revisions the revision row offers for `preference`: none for
/// Automatic or a console with only one.
pub fn revisions(preference: ModelPreference) -> &'static [Model] {
    match preference {
        ModelPreference::Fixed(model) if Console::of(model).revisions().len() > 1 => {
            Console::of(model).revisions()
        }
        _ => &[],
    }
}

/// Where the D-pad is in the settings window: on the tabs (in the sidebar or
/// the row above the page, see [`SettingsLayout`]), or on a row of the page
/// (counting from 0, see [`rows`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsFocus {
    Tabs,
    Row(usize),
}

/// A row of a settings page that the D-pad can stop on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsRow {
    Appearance,
    Palette,
    OriginalScreen,
    SharpPixels,
    SgbBorder,
    Model,
    /// The revision of the console picked on the model row.
    ModelRevision,
    RestartNow,
    ResumeGames,
    PauseInBackground,
    FastForwardSpeed,
    Volume,
    /// The keyboard or controller switch on the Controls tab.
    Device,
    ResetControls,
    Binding(Action),
    /// Which controller buttons pick and go back in the menus.
    MenuButtons,
    /// A library folder, by its place in the list.
    Folder(usize),
    /// The row that adds a folder, or in the browser games.
    AddFolder,
    BoxArt,
    /// Tries to download box art again after it failed.
    RetryBoxArt,
    /// The switch that looks for newer versions.
    CheckForUpdates,
    /// The button that asks what to start over with.
    ResetSettings,
}

/// The row the D-pad's focus at `index` is on, with its index: the last row
/// when rows went away from under the focus, such as the box art's Try again
/// once it hid itself, so the outline and what A does stay on the same row.
/// `None` when there are no rows.
pub fn focused_row(rows: &[SettingsRow], index: usize) -> Option<(usize, SettingsRow)> {
    let index = index.min(rows.len().checked_sub(1)?);
    Some((index, rows[index]))
}

/// The rows of `tab` in the order the D-pad moves through them, which is
/// the order the page shows them in (a test checks that the pages draw them
/// in this order).
pub fn rows(tab: SettingsTab, settings: &Settings, state: &PageState<'_>) -> Vec<SettingsRow> {
    let mut rows = Vec::new();

    match tab {
        SettingsTab::Display => rows.extend([
            SettingsRow::Appearance,
            SettingsRow::Palette,
            SettingsRow::OriginalScreen,
            SettingsRow::SharpPixels,
            SettingsRow::SgbBorder,
        ]),
        SettingsTab::Playing => {
            rows.push(SettingsRow::Model);
            if !revisions(settings.shared.model).is_empty() {
                rows.push(SettingsRow::ModelRevision);
            }
            if state.restart_needed {
                rows.push(SettingsRow::RestartNow);
            }
            if save_states::AVAILABLE {
                rows.push(SettingsRow::ResumeGames);
            }
            rows.push(SettingsRow::PauseInBackground);
            rows.extend([SettingsRow::FastForwardSpeed, SettingsRow::Volume]);
        }
        SettingsTab::Controls => {
            rows.extend([SettingsRow::Device, SettingsRow::ResetControls]);
            if state.device == Device::Controller {
                rows.push(SettingsRow::MenuButtons);
            }
            rows.extend(Action::GAME_BOY.map(SettingsRow::Binding));
            rows.extend(
                shortcuts(state.device)
                    .iter()
                    .copied()
                    .map(SettingsRow::Binding),
            );
        }
        SettingsTab::Library => {
            if !library::KEEPS_GAMES {
                rows.extend((0..settings.shared.library_folders.len()).map(SettingsRow::Folder));
            }
            rows.push(SettingsRow::AddFolder);
            if box_art::AVAILABLE {
                rows.push(SettingsRow::BoxArt);
                if state.box_art_problem.is_some() {
                    rows.push(SettingsRow::RetryBoxArt);
                }
            }
        }
        SettingsTab::About => {
            if updates::AVAILABLE {
                rows.push(SettingsRow::CheckForUpdates);
            }
            rows.push(SettingsRow::ResetSettings);
        }
    }

    rows
}

/// The shortcuts listed for `device`.
fn shortcuts(device: Device) -> &'static [Action] {
    match device {
        Device::Keyboard => &Action::KEYBOARD_SHORTCUTS,
        Device::Controller => &Action::CONTROLLER_SHORTCUTS,
    }
}

/// The scrolling page of the settings window, so the app can scroll it to
/// the row the D-pad is on.
pub fn scroll_id() -> widget::Id {
    widget::Id::new("settings-page")
}

/// What the pages show besides the settings themselves.
pub struct PageState<'a> {
    /// Whether the running game uses another model than the one picked, so
    /// the Playing tab offers to restart it.
    pub restart_needed: bool,
    /// Whether the Controls tab shows the keyboard or the controller.
    pub device: Device,
    /// The action waiting for a key or button, if any.
    pub capture: Option<(Device, Action)>,
    /// The names of the connected controllers.
    pub controllers: &'a [String],
    /// Why box art couldn't be downloaded this run, if it couldn't.
    pub box_art_problem: Option<&'a str>,
    /// A newer version than this one, if the app found one is out.
    pub newer_version: Option<&'a str>,
    /// How the window arranges itself for the app window's width.
    pub layout: SettingsLayout,
}

/// How the settings window arranges itself in an app window of a given
/// width. In a wide window the tabs are in a sidebar next to the page. In a
/// narrower one they go in a row above it, so the page keeps the room its
/// settings need; in the narrowest only the open tab shows its name there,
/// and controls too wide to share a row with their name go under it. The
/// view and the app's D-pad handling use the same layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SettingsLayout {
    /// The width of the app's window, or 0 while it isn't known yet.
    window_width: f32,
}

impl SettingsLayout {
    pub fn new(window_width: f32) -> Self {
        SettingsLayout { window_width }
    }

    /// Whether the tabs are in a sidebar next to the page. They are while
    /// the window's width isn't known yet, as in the window the app opens.
    pub fn has_sidebar(self) -> bool {
        self.window_width <= 0.0 || self.page_width(true) >= SIDEBAR_MIN_PAGE_WIDTH
    }

    /// How many tabs `action` moves along while the D-pad is on the tabs:
    /// up and down in the sidebar, left and right in the row above the page.
    pub fn tab_step(self, action: Action) -> Option<isize> {
        match (self.has_sidebar(), action) {
            (true, Action::Up) | (false, Action::Left) => Some(-1),
            (true, Action::Down) | (false, Action::Right) => Some(1),
            _ => None,
        }
    }

    /// Whether `action` goes from the tabs into the page: right from the
    /// sidebar, down from the row above the page, and A or Start from both.
    pub fn enters_page(self, action: Action) -> bool {
        match action {
            Action::A | Action::Start => true,
            Action::Right => self.has_sidebar(),
            Action::Down => !self.has_sidebar(),
            _ => false,
        }
    }

    /// Whether up on a page's first row goes back to the tabs, which it does
    /// when they are in the row above the page.
    pub fn up_leaves_page(self) -> bool {
        !self.has_sidebar()
    }

    /// The width of what the settings window shows inside its padding.
    fn inner_width(self) -> f32 {
        WIDTH.min(self.window_width - 2.0 * super::MODAL_MARGIN) - 2.0 * PANEL_PADDING
    }

    /// The width a page gets, with the sidebar next to it or without.
    fn page_width(self, sidebar: bool) -> f32 {
        let beside = if sidebar {
            SIDEBAR_WIDTH + 2.0 * RING_PADDING + BODY_SPACING
        } else {
            0.0
        };
        self.inner_width() - beside - PAGE_GUTTER
    }

    /// Whether every tab shows its name, not only the open one.
    fn names_every_tab(self) -> bool {
        self.has_sidebar() || self.inner_width() >= TAB_ROW_NAMED_WIDTH
    }

    /// Whether controls too wide to share a row with their setting's name
    /// go under it.
    fn stacks_wide_controls(self) -> bool {
        !self.has_sidebar() && self.page_width(false) < STACK_BELOW_PAGE_WIDTH
    }
}

/// The settings window showing `tab`.
pub fn view<'a>(
    tab: SettingsTab,
    settings: &'a Settings,
    library: &'a Library,
    state: &PageState<'a>,
    focus: Option<SettingsFocus>,
) -> Element<'a, Message> {
    let header = row![
        text("Settings").size(20).font(theme::HEADING_FONT),
        space::horizontal(),
        icon_button(
            Icon::X,
            String::from("Close (Esc)"),
            Some(Message::Settings(SettingsMessage::Close))
        ),
    ]
    .align_y(Alignment::Center);

    let focused = match focus {
        Some(SettingsFocus::Row(index)) => {
            focused_row(&rows(tab, settings, state), index).map(|(_, row)| row)
        }
        _ => None,
    };
    let shown = page_scroller(page(tab, settings, library, state, &Rings::new(focused)))
        .id(scroll_id())
        .height(Fill);
    let measured = SettingsTab::all()
        .iter()
        .map(|&tab| page_scroller(page(tab, settings, library, state, &Rings::new(None))).into());
    let page = tallest(shown, measured);
    let tabs_focused = focus == Some(SettingsFocus::Tabs);
    let body: Element<'a, Message> = if state.layout.has_sidebar() {
        row![sidebar(tab, tabs_focused), page]
            .spacing(BODY_SPACING)
            .into()
    } else {
        column![
            tab_row(tab, tabs_focused, state.layout.names_every_tab()),
            page
        ]
        .spacing(12)
        .into()
    };

    let mut panel = column![header, body].spacing(20);
    if focus.is_some() {
        panel = panel.push(
            text("D-pad moves · ◀ ▶ changes · A selects · B goes back · Select clears or removes")
                .size(12)
                .style(theme::muted_text),
        );
    }

    container(panel)
        .padding(PANEL_PADDING)
        .width(WIDTH)
        .style(theme::panel)
        .into()
}

/// A page in the scrollable it is shown in. It shrinks to the page, which
/// is how the window measures every page.
fn page_scroller<'a>(page: Element<'a, Message>) -> widget::Scrollable<'a, Message> {
    scrollable(container(page).padding([0, 4]))
        .spacing(12)
        .height(Shrink)
}

/// The list of tabs at the left of the window, with About set apart below
/// the others, outlined while the D-pad is on it.
fn sidebar<'a>(selected: SettingsTab, focused: bool) -> Element<'a, Message> {
    let mut tabs = Column::new().spacing(4).width(SIDEBAR_WIDTH);

    for &tab in SettingsTab::all() {
        if tab == SettingsTab::About {
            tabs = tabs.push(container(divider()).padding([6, 4]));
        }

        tabs = tabs.push(
            button(
                row![icon(tab.icon()), text(tab.label())]
                    .spacing(10)
                    .align_y(Alignment::Center),
            )
            .padding([8, 12])
            .width(Fill)
            .style(theme::segment(tab == selected))
            .on_press(Message::Settings(SettingsMessage::TabSelected(tab))),
        );
    }

    focus_ring(tabs, focused)
}

/// The tabs in a row above the page, for a window too narrow for the
/// sidebar, with About at the far end and outlined while the D-pad is on
/// them. Unless `name_every_tab`, only the open tab shows its name and the
/// others show their icons, with their names as tooltips.
fn tab_row<'a>(selected: SettingsTab, focused: bool, name_every_tab: bool) -> Element<'a, Message> {
    let mut tabs = Row::new().spacing(4).align_y(Alignment::Center);

    for &tab in SettingsTab::all() {
        if tab == SettingsTab::About {
            tabs = tabs.push(space::horizontal());
        }

        let named = name_every_tab || tab == selected;
        let mut label = row![icon(tab.icon())].spacing(8).align_y(Alignment::Center);
        if named {
            label = label.push(text(tab.label()).wrapping(Wrapping::None));
        }

        let tab_button = button(label)
            .padding([8, 12])
            .style(theme::segment(tab == selected))
            .on_press(Message::Settings(SettingsMessage::TabSelected(tab)));
        tabs = tabs.push(if named {
            tab_button.into()
        } else {
            with_tooltip(
                tab_button,
                String::from(tab.label()),
                tooltip::Position::Bottom,
            )
        });
    }

    focus_ring(tabs.width(Fill), focused)
}

/// The page of settings for `tab`, which draws its rows in the order
/// [`rows`] lists them.
fn page<'a>(
    tab: SettingsTab,
    settings: &'a Settings,
    library: &'a Library,
    state: &PageState<'a>,
    rings: &Rings,
) -> Element<'a, Message> {
    match tab {
        SettingsTab::Display => display(settings, rings),
        SettingsTab::Playing => playing(
            settings,
            state.restart_needed,
            state.layout.stacks_wide_controls(),
            rings,
        ),
        SettingsTab::Controls => controls(settings, state, rings),
        SettingsTab::Library => library_page(settings, library, state.box_art_problem, rings),
        SettingsTab::About => about(settings, state.newer_version, rings),
    }
}

/// Outlines `content` in the accent color while the D-pad is on it. The
/// outline is there but invisible otherwise, so nothing moves when it shows.
fn focus_ring<'a>(content: impl Into<Element<'a, Message>>, focused: bool) -> Element<'a, Message> {
    container(content)
        .padding(4)
        .style(theme::focus_ring(focused))
        .into()
}

/// Outlines the row of a page the D-pad is on. In tests it also notes the
/// rows in the order the page draws them, to check that [`rows`] lists them
/// in that order.
struct Rings {
    focused: Option<SettingsRow>,
    #[cfg(test)]
    drawn: std::cell::RefCell<Vec<SettingsRow>>,
}

impl Rings {
    fn new(focused: Option<SettingsRow>) -> Self {
        Rings {
            focused,
            #[cfg(test)]
            drawn: std::cell::RefCell::default(),
        }
    }

    /// Whether the D-pad is on `row`, which the page draws next.
    fn is_focused(&self, row: SettingsRow) -> bool {
        #[cfg(test)]
        self.drawn.borrow_mut().push(row);
        self.focused == Some(row)
    }

    /// `content`, the next row of the page outside a group, outlined while
    /// the D-pad is on it.
    fn around<'a>(
        &self,
        content: impl Into<Element<'a, Message>>,
        row: SettingsRow,
    ) -> Element<'a, Message> {
        focus_ring(content, self.is_focused(row))
    }

    /// `content` as the next row of a group, outlined while the D-pad is on
    /// it.
    fn row<'a>(
        &self,
        content: impl Into<Element<'a, Message>>,
        row: SettingsRow,
    ) -> Element<'a, Message> {
        group_row(content, ROW_PADDING, self.is_focused(row))
    }
}

/// A row of a group with `padding` around it, outlined while the D-pad is
/// on it. Rows the D-pad skips pass false.
fn group_row<'a>(
    content: impl Into<Element<'a, Message>>,
    padding: Padding,
    focused: bool,
) -> Element<'a, Message> {
    container(content)
        .padding(padding)
        .width(Fill)
        .style(theme::focus_ring(focused))
        .into()
}

/// A titled group of rows in a rounded card, with thin lines between the
/// rows. `action` is a small button at the right of the title, and `note`
/// a longer explanation under the card.
fn group<'a>(
    title: &'a str,
    action: Option<Element<'a, Message>>,
    rows: Vec<Element<'a, Message>>,
    note: Option<&'a str>,
) -> Element<'a, Message> {
    let mut heading = row![
        container(text(title).size(13).style(theme::muted_text))
            .height(HEADING_HEIGHT)
            .align_y(Alignment::Center)
    ]
    .align_y(Alignment::Center);
    if let Some(action) = action {
        heading = heading.push(space::horizontal()).push(action);
    }

    let mut card = Column::new();
    for (index, row) in rows.into_iter().enumerate() {
        if index > 0 {
            card = card.push(divider());
        }
        card = card.push(row);
    }

    let mut group = column![
        heading,
        container(card).width(Fill).style(theme::settings_card)
    ]
    .spacing(6);
    if let Some(note) = note {
        group = group.push(container(note_text(note)).padding([2, 2]));
    }

    group.into()
}

/// A setting's name with its description under it, taking the width the
/// control next to it leaves.
fn label<'a>(name: &'a str, description: Option<String>) -> Column<'a, Message> {
    let mut label = column![text(name).size(14)].spacing(3).width(Fill);
    if let Some(description) = description {
        label = label.push(text(description).size(12).style(theme::muted_text));
    }
    label
}

/// A setting's name and description with its control at the right.
fn setting<'a>(
    name: &'a str,
    description: Option<String>,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    row![label(name, description), control.into()]
        .spacing(16)
        .align_y(Alignment::Center)
        .into()
}

/// An on and off switch for a setting.
fn switch<'a>(on: bool, message: impl Fn(bool) -> Message + 'a) -> Element<'a, Message> {
    toggler(on).size(18).on_toggle(message).into()
}

/// A small outlined button, for the actions next to a page's settings.
fn small_button<'a>(symbol: Icon, name: &'a str, message: Message) -> Element<'a, Message> {
    button(
        row![
            icon(symbol).size(15),
            text(name).size(13).wrapping(Wrapping::None)
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([5, 11])
    .style(theme::secondary_button)
    .on_press(message)
    .into()
}

/// A longer explanation in small, soft text.
fn note_text<'a>(note: impl text::IntoFragment<'a>) -> text::Text<'a> {
    text(note).size(12).style(theme::muted_text)
}

/// A thin line between the rows of a group.
fn divider<'a>() -> Element<'a, Message> {
    container(space())
        .width(Fill)
        .height(1)
        .style(theme::divider)
        .into()
}

/// The theme, the colors of Game Boy games, the original screen look, sharp
/// pixels and the Super Game Boy border.
fn display<'a>(settings: &'a Settings, rings: &Rings) -> Element<'a, Message> {
    let appearance = segmented(
        &Appearance::ALL,
        settings.appearance,
        |appearance| Some(appearance_icon(appearance)),
        |appearance| Message::Settings(SettingsMessage::AppearanceSelected(appearance)),
    );
    let appearance_description = match settings.appearance {
        Appearance::System => "Follows your system's light or dark mode.",
        Appearance::Light => "Always light, whatever your system uses.",
        Appearance::Dark => "Always dark, whatever your system uses.",
    };

    let palettes = row(DmgPalette::ALL
        .into_iter()
        .map(|palette| palette_button(palette, palette == settings.shared.dmg_palette)))
    .spacing(8);

    column![
        group(
            "Window",
            None,
            vec![
                rings.row(
                    column![
                        label("Theme", Some(String::from(appearance_description))),
                        appearance
                    ]
                    .spacing(10),
                    SettingsRow::Appearance,
                )
            ],
            None,
        ),
        group(
            "Game screen",
            None,
            vec![
                rings.row(
                    column![
                        label(
                            "Colors for Game Boy games",
                            Some(String::from("Game Boy Color games keep their own colors."))
                        ),
                        palettes
                    ]
                    .spacing(10),
                    SettingsRow::Palette,
                ),
                rings.row(
                    setting(
                        "Original screen look",
                        Some(String::from(
                            "Softer colors and a gentle blur, as each Game Boy's own screen \
                             showed them. Off, sharp and bright."
                        )),
                        switch(settings.shared.original_screen, |enabled| {
                            Message::Settings(SettingsMessage::OriginalScreenToggled(enabled))
                        }),
                    ),
                    SettingsRow::OriginalScreen,
                ),
                rings.row(
                    setting(
                        "Sharp pixels",
                        Some(String::from(
                            "Grows the screen in whole steps (2×, 3×, …) so every pixel is \
                             the same size. Off, it fills the window."
                        )),
                        switch(settings.integer_scaling, |enabled| {
                            Message::Settings(SettingsMessage::IntegerScalingToggled(enabled))
                        }),
                    ),
                    SettingsRow::SharpPixels,
                ),
            ],
            None,
        ),
        group(
            "Super Game Boy",
            None,
            vec![rings.row(
                setting(
                    "Border around the screen",
                    Some(String::from(
                        "Super Game Boy games show their picture frame around the game."
                    )),
                    switch(settings.shared.sgb_border, |shown| {
                        Message::Settings(SettingsMessage::SgbBorderToggled(shown))
                    }),
                ),
                SettingsRow::SgbBorder,
            )],
            Some(
                "The game's colors and Nubo's own border are picked while playing: pause \
                 menu › Super Game Boy."
            ),
        ),
    ]
    .spacing(20)
    .into()
}

/// The model games start on (with a restart while the running game uses
/// another one), continuing games, pausing in the background, the fast
/// forward speed and the volume.
/// With `stack_wide_controls` the fast forward speeds go under their name.
fn playing<'a>(
    settings: &'a Settings,
    restart_needed: bool,
    stack_wide_controls: bool,
    rings: &Rings,
) -> Element<'a, Message> {
    let preference = settings.shared.model;
    let model_description = match preference {
        ModelPreference::Automatic => String::from(
            "Each game starts on the Game Boy it was made for. A game's menu in the \
             library can pick another.",
        ),
        ModelPreference::Fixed(model) if revisions(preference).is_empty() => {
            format!("Every game starts on this model. {}", model::about(model))
        }
        ModelPreference::Fixed(model) => format!(
            "Every game starts on the {}, in the revision below.",
            Console::of(model).label()
        ),
    };

    let mut console = vec![
        rings.row(
            column![
                label("Game Boy model", Some(model_description)),
                chip_grid(
                    &ModelOption::ALL,
                    MODEL_COLUMNS,
                    ModelOption::of(preference),
                    ModelOption::label,
                    move |option| Message::Settings(SettingsMessage::ModelSelected(
                        option.preference(preference)
                    )),
                ),
            ]
            .spacing(10),
            SettingsRow::Model,
        ),
    ];
    if let ModelPreference::Fixed(current) = preference
        && !revisions(preference).is_empty()
    {
        console.push(
            rings.row(
                column![
                    label("Revision", Some(String::from(model::about(current)))),
                    chips(
                        revisions(preference),
                        current,
                        |model| String::from(model.name()),
                        |model| Message::Settings(SettingsMessage::ModelSelected(
                            ModelPreference::Fixed(model)
                        )),
                    ),
                ]
                .spacing(10),
                SettingsRow::ModelRevision,
            ),
        );
    }
    if restart_needed {
        console.push(restart_notice(rings.is_focused(SettingsRow::RestartNow)));
    }

    let mut games = Vec::new();
    if save_states::AVAILABLE {
        games.push(rings.row(
            setting(
                "Continue where you left off",
                Some(String::from(
                    "A game's state is saved as you play, so it starts there next time.",
                )),
                switch(settings.shared.resume_games, |resume| {
                    Message::Settings(SettingsMessage::ResumeGamesToggled(resume))
                }),
            ),
            SettingsRow::ResumeGames,
        ));
    }
    games.push(rings.row(
        setting(
            "Pause in the background",
            Some(String::from("While another window is in front.")),
            switch(settings.pause_in_background, |pause| {
                Message::Settings(SettingsMessage::PauseInBackgroundToggled(pause))
            }),
        ),
        SettingsRow::PauseInBackground,
    ));

    let fast_forward_key = settings
        .shared
        .controls
        .input_label(Device::Keyboard, Action::FastForward);
    let fast_forward_description = match fast_forward_key {
        Some(key) => format!("Hold {key}, or use the button in the top bar."),
        None => String::from("Use the button in the top bar."),
    };
    let speeds = segmented(
        &FastForwardSpeed::ALL,
        settings.shared.fast_forward_speed,
        |_| None,
        |speed| Message::Settings(SettingsMessage::FastForwardSpeedSelected(speed)),
    );
    let fast_forward: Element<'a, Message> = if stack_wide_controls {
        column![
            label("Fast forward speed", Some(fast_forward_description)),
            speeds
        ]
        .spacing(10)
        .into()
    } else {
        setting("Fast forward speed", Some(fast_forward_description), speeds)
    };
    games.push(rings.row(fast_forward, SettingsRow::FastForwardSpeed));

    column![
        group("Game Boy", None, console, None),
        group("Games", None, games, None),
        group(
            "Sound",
            None,
            vec![rings.row(volume(settings), SettingsRow::Volume)],
            None
        ),
    ]
    .spacing(20)
    .into()
}

/// The row that says the running game still uses the previous model, with
/// a button that restarts it on the new one.
fn restart_notice<'a>(focused: bool) -> Element<'a, Message> {
    let content = row![
        icon(Icon::TriangleAlert).style(theme::warning_text),
        text("The running game still uses the previous model.")
            .size(13)
            .width(Fill),
        small_button(
            Icon::RotateCcw,
            "Restart now",
            Message::Game(GameMessage::Reset)
        ),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    container(content)
        .padding(ROW_PADDING)
        .width(Fill)
        .style(theme::notice_row(focused))
        .into()
}

/// The volume slider, with a mute button and the volume in percent.
fn volume<'a>(settings: &Settings) -> Element<'a, Message> {
    let muted = settings.shared.muted;
    let volume = settings.shared.effective_volume();
    let level = if muted {
        String::from("Muted")
    } else {
        format!("{}%", (volume * 100.0).round())
    };

    row![
        text("Volume").size(14).width(64),
        icon_button(
            if muted { Icon::VolumeX } else { Icon::Volume2 },
            String::from(if muted { "Unmute" } else { "Mute" }),
            Some(Message::Settings(SettingsMessage::ToggleMute)),
        ),
        slider(0.0..=1.0_f32, volume, |volume| {
            Message::Settings(SettingsMessage::VolumeChanged(volume))
        })
        .step(0.01_f32)
        .on_release(Message::Settings(SettingsMessage::VolumeReleased)),
        text(level).size(13).width(48).align_x(Alignment::End),
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .into()
}

/// What Nubo is: the version (with the commit a packaged build was made
/// from), links to what's new in it and to its source code (and this
/// build's commit on GitHub), the switch that looks for new versions and
/// the button that starts over, how Nubo is made, and the license, where to
/// read it and the copyright, which the GPL asks every copy to show.
fn about<'a>(
    settings: &'a Settings,
    newer_version: Option<&'a str>,
    rings: &Rings,
) -> Element<'a, Message> {
    let mut links = row![
        small_button(
            Icon::ExternalLink,
            "What's new",
            Message::OpenLink(RELEASE_NOTES)
        ),
        small_button(
            Icon::ExternalLink,
            "Source code",
            Message::OpenLink(env!("CARGO_PKG_REPOSITORY")),
        ),
    ]
    .spacing(8);
    if let Some(commit) = &*version::COMMIT {
        links = links.push(with_tooltip(
            small_button(
                Icon::ExternalLink,
                "This build",
                Message::OpenLink(&commit.page),
            ),
            build_description(commit),
            tooltip::Position::Bottom,
        ));
    }

    let version = column![
        text(format!("Nubo {}", *version::VERSION))
            .size(20)
            .font(theme::HEADING_FONT),
        note_text("A Game Boy and Game Boy Color emulator").size(13),
        container(links).padding(Padding {
            top: 8.0,
            ..Padding::ZERO
        }),
    ]
    .spacing(2)
    .width(Fill);

    let mut general = Vec::new();
    if updates::AVAILABLE {
        general.push(rings.row(
            setting(
                "Tell me when a new Nubo is out",
                Some(String::from(
                    "When Nubo starts, it asks GitHub for the newest version.",
                )),
                switch(settings.shared.check_for_updates == Some(true), |check| {
                    Message::Settings(SettingsMessage::CheckForUpdatesToggled(check))
                }),
            ),
            SettingsRow::CheckForUpdates,
        ));
        if let Some(newer) = newer_version {
            general.push(group_row(
                setting(
                    "A new version is out",
                    Some(format!("Nubo {newer} is ready to download.")),
                    small_button(
                        Icon::ExternalLink,
                        "Open",
                        Message::OpenLink(updates::RELEASES_PAGE),
                    ),
                ),
                ROW_PADDING,
                false,
            ));
        }
    }
    general.push(rings.row(
        setting(
            "Reset settings and data",
            Some(String::from(
                "Reset the settings, clear Continue playing or delete saves. You pick what \
                 goes. Only the keys or buttons: Controls › Reset.",
            )),
            small_button(
                Icon::RotateCcw,
                "Start over…",
                Message::Settings(SettingsMessage::AskReset),
            ),
        ),
        SettingsRow::ResetSettings,
    ));

    column![
        row![logo(56.0), version].spacing(16),
        group("Updates and starting over", None, general, None),
        about_section(
            "Made with AI",
            vec![
                paragraph(
                    "Nearly all of Nubo's code and documentation, and even its name, were \
                     written by Claude, Anthropic's AI model, working in Claude Code. Its \
                     developer decides what Nubo should do and chooses what goes in. It \
                     started as an experiment: how good an emulator could be made using only \
                     AI, and how well would the quality of the code hold up?",
                ),
                paragraph(
                    "Nubo has very few original ideas. It rests on the clever ideas and hard \
                     work of many others, from other emulators to the people who wrote the \
                     test ROMs and documented the hardware, all credited in its README.",
                ),
                paragraph(
                    "If you'd rather not use software made this way, that's a fair choice, \
                     and it's why this is said up front.",
                ),
            ],
        ),
        about_section(
            "License",
            vec![
                paragraph(
                    "Nubo is free software: you can share and change it under the GNU \
                     General Public License, version 3 or later. It comes with no warranty. \
                     Every copy of Nubo holds the license as LICENSE.txt, and lists the code \
                     from other projects with its license in NOTICE.txt and \
                     THIRD_PARTY_LICENSES.txt.",
                ),
                column![
                    note_text("License: https://www.gnu.org/licenses/gpl-3.0.html"),
                    note_text(concat!("Source code: ", env!("CARGO_PKG_REPOSITORY"))),
                    note_text(
                        "Copyright 2026 Michel Schoenmakers. Based on RetroBoy, copyright \
                         2023-2026 Samuel Parsons.",
                    ),
                ]
                .spacing(2)
                .into(),
            ],
        ),
    ]
    .spacing(12)
    .padding([4, 2])
    .into()
}

/// A part of the About tab: paragraphs of plain text in a titled card.
fn about_section<'a>(
    title: &'a str,
    paragraphs: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    group(
        title,
        None,
        vec![group_row(
            Column::with_children(paragraphs).spacing(6),
            ROW_PADDING,
            false,
        )],
        None,
    )
}

/// A paragraph of the About tab's text.
fn paragraph<'a>(content: &'a str) -> Element<'a, Message> {
    text(content).size(13).width(Fill).into()
}

/// Which commit this build was made from, for the About tab.
fn build_description(commit: &version::Commit) -> String {
    let mut description = format!("Commit {}", commit.short_hash());
    if !commit.date.is_empty() {
        description.push_str(&format!(" from {}", commit.date));
    }
    if commit.dirty {
        description.push_str(", with changes that weren't committed");
    }
    description + ", on GitHub."
}

/// A palette choice: its four colors as swatches, with its name.
fn palette_button<'a>(palette: DmgPalette, selected: bool) -> Element<'a, Message> {
    let swatches = row(palette.colors().into_iter().map(|[red, green, blue]| {
        container(space())
            .width(18)
            .height(18)
            .style(move |_: &Theme| container::Style {
                background: Some(Color::from_rgb8(red, green, blue).into()),
                ..container::Style::default()
            })
            .into()
    }));

    button(
        column![
            container(swatches).padding(1).style(theme::swatch_frame),
            text(palette.to_string()).size(13),
        ]
        .spacing(6)
        .align_x(Alignment::Center),
    )
    .padding([8, 12])
    .style(theme::segment(selected))
    .on_press(Message::Settings(SettingsMessage::PaletteSelected(palette)))
    .into()
}

/// The keyboard or controller switch with a Reset button as big as it
/// beside it (under it in a narrow window) that resets that device's
/// bindings, then on a
/// controller which buttons pick and go back in the menus, and the bindings
/// of the Game Boy's buttons and the shortcuts.
fn controls<'a>(
    settings: &'a Settings,
    page: &PageState<'a>,
    rings: &Rings,
) -> Element<'a, Message> {
    let device = page.device;

    let switch = rings.around(
        segmented(
            &Device::ALL,
            device,
            |device| {
                Some(match device {
                    Device::Keyboard => Icon::Keyboard,
                    Device::Controller => Icon::Gamepad2,
                })
            },
            |device| Message::Settings(SettingsMessage::ControlsDeviceSelected(device)),
        ),
        SettingsRow::Device,
    );
    // As tall as the switch beside it: its padding is the switch's own
    // plus the frame around the switch's buttons.
    let reset = rings.around(
        button(
            row![
                icon(Icon::RotateCcw),
                text("Reset").wrapping(Wrapping::None)
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([9, 14])
        .style(theme::secondary_button)
        .on_press(Message::Settings(SettingsMessage::ResetControls(device))),
        SettingsRow::ResetControls,
    );
    let header: Element<'a, Message> = if page.layout.stacks_wide_controls() {
        column![switch, reset].spacing(8).into()
    } else {
        row![switch, space::horizontal(), reset]
            .align_y(Alignment::Center)
            .into()
    };

    let hint = match device {
        Device::Keyboard => String::from(
            "Click a key, then press the new one within 5 seconds. Esc cancels and \
             Backspace clears. In the menus the arrow keys, Enter or Space and Backspace \
             always work.",
        ),
        Device::Controller if page.controllers.is_empty() => String::from(
            "No controller connected. Click a button, then press the new one on your \
             controller within 5 seconds; Backspace leaves it unset. The left stick always \
             works as the D-pad. In the menus the D-pad and the buttons picked below always \
             work.",
        ),
        Device::Controller => format!(
            "Connected: {}. Click a button, then press the new one within 5 seconds; \
             Backspace clears. The left stick is always the D-pad. In the menus the D-pad \
             and the buttons picked below always work.",
            page.controllers.join(", ")
        ),
    };

    let bindings = |actions: &[Action]| -> Vec<Element<'a, Message>> {
        actions
            .iter()
            .map(|&action| {
                group_row(
                    binding_row(
                        settings,
                        device,
                        action,
                        page.capture == Some((device, action)),
                    ),
                    BINDING_ROW_PADDING,
                    rings.is_focused(SettingsRow::Binding(action)),
                )
            })
            .collect()
    };

    let menus = (device == Device::Controller).then(|| {
        group(
            "Menus",
            None,
            vec![rings.row(
                menu_buttons(settings, page.layout.stacks_wide_controls()),
                SettingsRow::MenuButtons,
            )],
            None,
        )
    });
    let game_boy = bindings(&Action::GAME_BOY);
    let mut shortcut_rows = Vec::new();
    if device == Device::Keyboard {
        shortcut_rows.push(group_row(pause_menu_key(), BINDING_ROW_PADDING, false));
    }
    shortcut_rows.extend(bindings(shortcuts(device)));

    let mut content =
        column![column![header, container(note_text(hint)).padding([0, 6])].spacing(4)].spacing(20);
    if let Some(menus) = menus {
        content = content.push(menus);
    }
    content
        .push(group("Game Boy buttons", None, game_boy, None))
        .push(group("Shortcuts", None, shortcut_rows, None))
        .into()
}

/// The choice of which controller buttons pick and go back in the menus,
/// whatever the Game Boy's A and B are bound to.
fn menu_buttons<'a>(settings: &Settings, stack_wide_controls: bool) -> Element<'a, Message> {
    let current = settings.shared.controls.menu_buttons;
    let description = String::from(match current {
        MenuButtons::Nintendo => {
            "Like a Nintendo Switch: in Nubo's menus, the right button picks and the \
             bottom button goes back. Games use the Game Boy buttons below."
        }
        MenuButtons::Xbox => {
            "Like an Xbox or PlayStation: in Nubo's menus, the bottom button picks and \
             the right button goes back. Games use the Game Boy buttons below."
        }
    });
    let choices = segmented(
        &MenuButtons::ALL,
        current,
        |_| None,
        |buttons| Message::Settings(SettingsMessage::MenuButtonsSelected(buttons)),
    );
    if stack_wide_controls {
        column![label("Menu buttons", Some(description)), choices]
            .spacing(10)
            .into()
    } else {
        setting("Menu buttons", Some(description), choices)
    }
}

/// The pause menu's key on the keyboard, Esc, which can't be changed so
/// the menu can always be reached.
fn pause_menu_key<'a>() -> Element<'a, Message> {
    row![
        label(
            Action::PauseMenu.label(),
            Some(String::from("Fixed, so the menu can always be opened"))
        ),
        button(text("Esc").size(13).align_x(Alignment::Center).width(Fill))
            .padding([5, 12])
            .width(CHIP_WIDTH)
            .style(theme::binding_chip(false, false)),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

/// An action with the key or button that does it. Clicking the key waits
/// for a new one, or for Backspace, which leaves the action unset unless it
/// must keep one (the pause menu on a controller).
fn binding_row(
    settings: &Settings,
    device: Device,
    action: Action,
    capturing: bool,
) -> Element<'_, Message> {
    let bound = settings.shared.controls.input_label(device, action);

    let chip_text = match (&bound, capturing, device) {
        (_, true, Device::Keyboard) => String::from("Press a key…"),
        (_, true, Device::Controller) => String::from("Press a button…"),
        (Some(name), false, _) => name.clone(),
        (None, false, _) => String::from("Not set"),
    };

    let chip = button(
        text(chip_text)
            .size(13)
            .align_x(Alignment::Center)
            .width(Fill),
    )
    .padding([5, 12])
    .width(CHIP_WIDTH)
    .style(theme::binding_chip(capturing, bound.is_none()))
    .on_press(Message::Settings(SettingsMessage::CaptureBinding(
        device, action,
    )));

    let description = (!Controls::can_clear(device, action))
        .then(|| String::from("Always has a button, so the menu can always be opened"));
    row![label(action.label(), description), chip]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
}

/// The library's folders, each with a menu, and a row that adds one (in the
/// browser, the games it keeps and a row that adds some), and the box art
/// switch.
fn library_page<'a>(
    settings: &'a Settings,
    library: &'a Library,
    box_art_problem: Option<&'a str>,
    rings: &Rings,
) -> Element<'a, Message> {
    let mut page = column![if library::KEEPS_GAMES {
        kept_games(library, rings)
    } else {
        game_folders(settings, library, rings)
    }]
    .spacing(20);

    if box_art::AVAILABLE {
        let mut rows = vec![rings.row(
            setting(
                "Download box art",
                Some(String::from(
                    "Games without box art show their title screen.",
                )),
                switch(settings.shared.download_box_art, |enabled| {
                    Message::Library(LibraryMessage::DownloadBoxArtToggled(enabled))
                }),
            ),
            SettingsRow::BoxArt,
        )];
        if let Some(problem) = box_art_problem {
            rows.push(box_art_problem_row(
                problem,
                rings.is_focused(SettingsRow::RetryBoxArt),
            ));
        }
        page = page.push(group(
            "Box art",
            None,
            rows,
            Some(if box_art::SERVER == "GitHub" {
                "Pictures come from the libretro thumbnail collection on GitHub. While this is \
                 on, the names of your games are sent to GitHub, where the game lists used to \
                 find them come from too."
            } else {
                "Pictures come from the libretro thumbnail collection. While this is on, the \
                 names of your games are sent to thumbnails.libretro.com, and the game lists \
                 used to find them come from GitHub."
            }),
        ));
    }

    page.into()
}

/// The library's folders, each with a menu, and a row that adds one.
fn game_folders<'a>(
    settings: &'a Settings,
    library: &'a Library,
    rings: &Rings,
) -> Element<'a, Message> {
    let mut folders: Vec<Element<'a, Message>> = settings
        .shared
        .library_folders
        .iter()
        .enumerate()
        .map(|(index, folder)| {
            rings.row(
                folder_row(index, folder, library.is_missing(&folder.path)),
                SettingsRow::Folder(index),
            )
        })
        .collect();
    if folders.is_empty() {
        folders.push(group_row(
            text("No folders yet. Add the folders that hold your games.")
                .size(14)
                .style(theme::muted_text),
            ROW_PADDING,
            false,
        ));
    }
    folders.push(
        rings.row(
            button(
                row![icon(Icon::FolderPlus), text("Add folder…").size(14)]
                    .spacing(12)
                    .align_y(Alignment::Center),
            )
            .padding(0)
            .style(theme::text_button)
            .on_press(Message::Library(LibraryMessage::AddFolder)),
            SettingsRow::AddFolder,
        ),
    );

    group(
        "Game folders",
        None,
        folders,
        Some(
            "Nubo looks for .gb, .gbc and .zip files in these folders, and sees new ones \
             by itself. A folder's ⋯ menu has more.",
        ),
    )
}

/// In the browser, how many games the library keeps and how much room they
/// take, and a row that adds games.
fn kept_games<'a>(library: &'a Library, rings: &Rings) -> Element<'a, Message> {
    #[cfg(target_arch = "wasm32")]
    let (count, bytes) = library.kept_size();
    #[cfg(not(target_arch = "wasm32"))]
    let (count, bytes) = (library.games("").count(), 0_u64);
    let summary = match count {
        0 => String::from("No games yet. Add your games to keep them here."),
        1 => format!("1 game, about {}", megabytes(bytes)),
        _ => format!("{count} games, about {}", megabytes(bytes)),
    };

    let rows = vec![
        group_row(
            text(summary).size(14).style(theme::muted_text),
            ROW_PADDING,
            false,
        ),
        rings.row(
            button(
                row![icon(Icon::FilePlus), text("Add games…").size(14)]
                    .spacing(12)
                    .align_y(Alignment::Center),
            )
            .padding(0)
            .style(theme::text_button)
            .on_press(Message::Library(LibraryMessage::AddGames)),
            SettingsRow::AddFolder,
        ),
    ];
    group(
        "Games in this browser",
        None,
        rows,
        Some(
            "Nubo keeps the games you add or open in this browser, with their saves. A game's \
             ⋯ menu in the library removes it.",
        ),
    )
}

/// A number of bytes in megabytes, with one decimal under ten.
fn megabytes(bytes: u64) -> String {
    let megabytes = bytes as f64 / 1_000_000.0;
    if megabytes < 10.0 {
        format!("{megabytes:.1} MB")
    } else {
        format!("{megabytes:.0} MB")
    }
}

/// The row that says box art couldn't be downloaded, with why in small
/// print, and a button that tries again.
fn box_art_problem_row(problem: &str, focused: bool) -> Element<'_, Message> {
    let content = row![
        icon(Icon::CloudOff).style(theme::warning_text),
        column![
            text("Couldn't reach the box art server, so cards show title screens for now.")
                .size(13),
            note_text(problem),
        ]
        .spacing(2)
        .width(Fill),
        small_button(
            Icon::RefreshCw,
            "Try again",
            Message::Library(LibraryMessage::RetryBoxArt)
        ),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    container(content)
        .padding(ROW_PADDING)
        .width(Fill)
        .style(theme::notice_row(focused))
        .into()
}

/// A library folder with its name and path, and a button that opens its
/// menu (subfolders, showing it, scanning it again and removing it).
fn folder_row(index: usize, folder: &LibraryFolder, missing: bool) -> Element<'_, Message> {
    let name = folder
        .path
        .file_name()
        .unwrap_or(folder.path.as_os_str())
        .to_string_lossy()
        .into_owned();

    let symbol = if missing {
        icon(Icon::TriangleAlert).style(theme::danger_text)
    } else {
        icon(Icon::Folder).style(theme::accent_text)
    };

    let mut detail = if missing {
        format!("Not found: {}", folder.path.display())
    } else {
        folder.path.display().to_string()
    };
    if !folder.include_subfolders {
        detail.push_str(" · without subfolders");
    }

    row![
        symbol,
        column![
            text(name).size(14),
            text(detail).size(12).style(theme::muted_text)
        ]
        .spacing(3)
        .width(Fill),
        icon_button(
            Icon::Ellipsis,
            String::from("More"),
            Some(Message::Library(LibraryMessage::OpenFolderMenu(index))),
        ),
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .into()
}

/// The icon on each choice of the Appearance setting: a monitor for
/// following the system, a sun for light and a moon for dark.
fn appearance_icon(appearance: Appearance) -> Icon {
    match appearance {
        Appearance::System => Icon::Monitor,
        Appearance::Light => Icon::Sun,
        Appearance::Dark => Icon::Moon,
    }
}

#[cfg(test)]
mod tests;
