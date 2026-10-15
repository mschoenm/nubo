//! The settings window, drawn over the current screen, with a tab per
//! group of settings. Every page is built the same way: titled groups of
//! rows in a rounded card, each row with its name and a short description
//! at the left and its control at the right (or under them, for a control
//! too wide to share the row), and any longer note under its group. The
//! window grows with the app's window, up to [`MAX_HEIGHT`].

use crate::app::{GameMessage, LibraryMessage, Message, SettingsMessage};
use crate::settings::{Appearance, Settings};
use crate::ui::icons::{Icon, icon};
use crate::ui::logo::logo;
use crate::ui::theme;
use crate::ui::widgets::{icon_button, segmented};
use iced::widget::text::Wrapping;
use iced::widget::{
    Column, button, column, container, pick_list, row, scrollable, slider, space, text, toggler,
};
use iced::{Alignment, Color, Element, Fill, Padding, Theme, widget};
use nubo_core::Model;
use nubo_frontend::controls::{Action, Device};
use nubo_frontend::library::{self, Library, box_art};
use nubo_frontend::model::ModelPreference;
use nubo_frontend::palettes::DmgPalette;
use nubo_frontend::save_states;
use nubo_frontend::settings::{FastForwardSpeed, LibraryFolder};
use std::fmt;

const WIDTH: f32 = 700.0;
/// The tallest the window gets. Below that it fills the app's window, less
/// the margin `views::modal` leaves around it.
const MAX_HEIGHT: f32 = 640.0;
const SIDEBAR_WIDTH: f32 = 170.0;
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
/// The width of the chips that show the key or button of an action.
const CHIP_WIDTH: f32 = 140.0;
/// The width of the clear button next to a chip, kept free when there is
/// nothing to clear so the chips line up.
const CLEAR_WIDTH: f32 = 34.0;

/// A page of the settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    /// How the window and the game screen look.
    Display,
    /// The hardware games run on, continuing games, fast forward and sound.
    Playing,
    Controls,
    Library,
    /// The version, the license and where to get the source code.
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

/// A model preference as the model picker lists it. Automatic is explained
/// by the row's description, so the picker keeps it short.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ModelChoice(ModelPreference);

impl fmt::Display for ModelChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            ModelPreference::Automatic => formatter.write_str("Automatic"),
            ModelPreference::Fixed(model) => formatter.write_str(model.description()),
        }
    }
}

/// Where the D-pad is in the settings window: on the tabs at the left, or on
/// a row of the page (counting from 0, see [`rows`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsFocus {
    Sidebar,
    Row(usize),
}

/// A row of a settings page that the D-pad can stop on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsRow {
    Appearance,
    Palette,
    SharpPixels,
    SgbBorder,
    Model,
    RestartNow,
    ResumeGames,
    FastForwardSpeed,
    Volume,
    /// The keyboard or controller switch on the Controls tab.
    Device,
    ResetControls,
    Binding(Action),
    Rescan,
    /// A library folder, by its place in the list.
    Folder(usize),
    AddFolder,
    BoxArt,
    /// Tries to download box art again after it failed.
    RetryBoxArt,
}

/// The rows of `tab` in the order the D-pad moves through them, which is
/// the order the page shows them in (a test checks that the pages draw them
/// in this order). The About tab has none.
pub fn rows(tab: SettingsTab, settings: &Settings, state: &PageState<'_>) -> Vec<SettingsRow> {
    let mut rows = Vec::new();

    match tab {
        SettingsTab::Display => rows.extend([
            SettingsRow::Appearance,
            SettingsRow::Palette,
            SettingsRow::SharpPixels,
            SettingsRow::SgbBorder,
        ]),
        SettingsTab::Playing => {
            rows.push(SettingsRow::Model);
            if state.restart_needed {
                rows.push(SettingsRow::RestartNow);
            }
            if save_states::AVAILABLE {
                rows.push(SettingsRow::ResumeGames);
            }
            rows.extend([SettingsRow::FastForwardSpeed, SettingsRow::Volume]);
        }
        SettingsTab::Controls => {
            rows.push(SettingsRow::Device);
            rows.extend(Action::GAME_BOY.map(SettingsRow::Binding));
            rows.extend(
                shortcuts(state.device)
                    .iter()
                    .copied()
                    .map(SettingsRow::Binding),
            );
            rows.push(SettingsRow::ResetControls);
        }
        SettingsTab::Library => {
            rows.push(SettingsRow::Rescan);
            rows.extend((0..settings.shared.library_folders.len()).map(SettingsRow::Folder));
            rows.push(SettingsRow::AddFolder);
            if box_art::AVAILABLE {
                rows.push(SettingsRow::BoxArt);
                if state.box_art_problem.is_some() {
                    rows.push(SettingsRow::RetryBoxArt);
                }
            }
        }
        SettingsTab::About => {}
    }

    rows
}

/// The model choices in the order the picker lists them.
pub fn model_choices() -> Vec<ModelPreference> {
    std::iter::once(ModelPreference::Automatic)
        .chain(Model::ALL.map(ModelPreference::Fixed))
        .collect()
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
        Some(SettingsFocus::Row(index)) => rows(tab, settings, state).get(index).copied(),
        _ => None,
    };
    let page = page(tab, settings, library, state, &Rings::new(focused));

    let body = row![
        sidebar(tab, focus == Some(SettingsFocus::Sidebar)),
        scrollable(container(page).padding([0, 4]))
            .id(scroll_id())
            .spacing(12)
            .height(Fill)
    ]
    .spacing(20);

    let mut panel = column![header, body].spacing(20);
    if focus.is_some() {
        panel = panel.push(
            text("D-pad moves · ◀ ▶ changes · A selects · B goes back · Select clears or removes")
                .size(12)
                .style(theme::muted_text),
        );
    }

    container(panel)
        .padding(24)
        .width(WIDTH)
        .height(Fill)
        .max_height(MAX_HEIGHT)
        .style(theme::panel)
        .into()
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
        SettingsTab::Playing => playing(settings, state.restart_needed, rings),
        SettingsTab::Controls => controls(settings, state, rings),
        SettingsTab::Library => library_page(settings, library, state.box_art_problem, rings),
        SettingsTab::About => about(),
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

/// The theme, the colors of Game Boy games, sharp pixels and the Super
/// Game Boy border.
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
                        "Sharp pixels",
                        Some(String::from(
                            "Grow the screen only in whole steps (2×, 3×, …), so every pixel \
                             is the same size."
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
/// another one), continuing games, the fast forward speed and the volume.
fn playing<'a>(
    settings: &'a Settings,
    restart_needed: bool,
    rings: &Rings,
) -> Element<'a, Message> {
    let choices: Vec<ModelChoice> = model_choices().into_iter().map(ModelChoice).collect();
    let model_description = match settings.shared.model {
        ModelPreference::Automatic => "Each game starts on the Game Boy it was made for.",
        ModelPreference::Fixed(_) => "Every game starts on this model.",
    };

    let mut console = vec![
        rings.row(
            column![
                label("Game Boy model", Some(String::from(model_description))),
                pick_list(
                    choices,
                    Some(ModelChoice(settings.shared.model)),
                    |choice| Message::Settings(SettingsMessage::ModelSelected(choice.0)),
                )
                .text_size(13)
                .width(Fill),
            ]
            .spacing(10),
            SettingsRow::Model,
        ),
    ];
    if restart_needed {
        console.push(restart_notice(rings.is_focused(SettingsRow::RestartNow)));
    }

    let mut games = Vec::new();
    if save_states::AVAILABLE {
        games.push(rings.row(
            setting(
                "Continue where you left off",
                Some(String::from(
                    "Leaving a game saves its state, so it starts there next time.",
                )),
                switch(settings.shared.resume_games, |resume| {
                    Message::Settings(SettingsMessage::ResumeGamesToggled(resume))
                }),
            ),
            SettingsRow::ResumeGames,
        ));
    }

    let fast_forward_key = settings
        .shared
        .controls
        .input_label(Device::Keyboard, Action::FastForward);
    let fast_forward_description = match fast_forward_key {
        Some(key) => format!("Hold {key}, or use the button in the top bar."),
        None => String::from("Use the button in the top bar."),
    };
    games.push(rings.row(
        setting(
            "Fast forward speed",
            Some(fast_forward_description),
            segmented(
                &FastForwardSpeed::ALL,
                settings.shared.fast_forward_speed,
                |_| None,
                |speed| Message::Settings(SettingsMessage::FastForwardSpeedSelected(speed)),
            ),
        ),
        SettingsRow::FastForwardSpeed,
    ));

    column![
        group("Console", None, console, None),
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

/// The version, the copyright, the license, where to read it and where to get
/// the source code, which the GPL asks every copy to show.
fn about<'a>() -> Element<'a, Message> {
    let title = row![
        logo(48.0),
        column![
            text(concat!("Nubo ", env!("CARGO_PKG_VERSION")))
                .size(20)
                .font(theme::HEADING_FONT),
            note_text("A Game Boy and Game Boy Color emulator").size(13),
        ]
        .spacing(2),
    ]
    .spacing(16)
    .align_y(Alignment::Center);

    let rows = vec![
        group_row(
            setting(
                "Source code",
                None,
                text(env!("CARGO_PKG_REPOSITORY")).size(13),
            ),
            ROW_PADDING,
            false,
        ),
        group_row(
            label(
                "License: GNU General Public License, version 3 or later",
                Some(String::from(
                    "At https://www.gnu.org/licenses/gpl-3.0.html, and in every copy of \
                     Nubo as LICENSE.txt.",
                )),
            ),
            ROW_PADDING,
            false,
        ),
        group_row(
            label(
                "Code from other projects",
                Some(String::from(
                    "Listed with their licenses in NOTICE.txt and THIRD_PARTY_LICENSES.txt.",
                )),
            ),
            ROW_PADDING,
            false,
        ),
    ];

    column![
        container(title).padding([4, 2]),
        group(
            "Source and license",
            None,
            rows,
            Some(
                "Copyright 2026 Michel Schoenmakers. Based on RetroBoy, copyright 2023-2026 \
                 Samuel Parsons. Nubo is free software: you can share and change it under \
                 the GNU General Public License, version 3 or later. It comes with no \
                 warranty."
            ),
        ),
    ]
    .spacing(20)
    .into()
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
            container(swatches).style(theme::swatch_frame),
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

/// The keyboard or controller switch, the bindings of the Game Boy's
/// buttons and the shortcuts, and a button that resets them.
fn controls<'a>(
    settings: &'a Settings,
    page: &PageState<'a>,
    rings: &Rings,
) -> Element<'a, Message> {
    let device = page.device;

    let header = rings.around(
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

    let hint = match device {
        Device::Keyboard => {
            String::from("Click a key, then press the new one. Esc cancels and Backspace clears.")
        }
        Device::Controller if page.controllers.is_empty() => String::from(
            "No controller connected. Click a button, then press the new one on your \
             controller. The left stick always works as the D-pad.",
        ),
        Device::Controller => format!(
            "Connected: {}. Click a button, then press the new one. The left stick always \
             works as the D-pad.",
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

    let game_boy = bindings(&Action::GAME_BOY);
    let mut shortcut_rows = Vec::new();
    if device == Device::Keyboard {
        shortcut_rows.push(group_row(pause_menu_key(), BINDING_ROW_PADDING, false));
    }
    shortcut_rows.extend(bindings(shortcuts(device)));

    let reset = rings.around(
        small_button(
            Icon::RotateCcw,
            "Reset to defaults",
            Message::Settings(SettingsMessage::ResetControls(device)),
        ),
        SettingsRow::ResetControls,
    );

    column![
        column![header, container(note_text(hint)).padding([0, 6])].spacing(4),
        group("Game Boy buttons", None, game_boy, None),
        group("Shortcuts", None, shortcut_rows, None),
        row![space::horizontal(), reset],
    ]
    .spacing(20)
    .into()
}

/// The pause menu's key on the keyboard, Esc, which can't be changed so
/// the menu can always be reached.
fn pause_menu_key<'a>() -> Element<'a, Message> {
    row![
        label(
            Action::PauseMenu.label(),
            Some(String::from("Always Esc on the keyboard"))
        ),
        button(text("Esc").size(13).align_x(Alignment::Center).width(Fill))
            .padding([5, 12])
            .width(CHIP_WIDTH)
            .style(theme::binding_chip(false, false)),
        space().width(CLEAR_WIDTH),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

/// An action with the key or button that does it. Clicking the key waits
/// for a new one.
fn binding_row(
    settings: &Settings,
    device: Device,
    action: Action,
    capturing: bool,
) -> Element<'_, Message> {
    let bound = settings.shared.controls.input_label(device, action);

    let label = match (&bound, capturing, device) {
        (_, true, Device::Keyboard) => String::from("Press a key…"),
        (_, true, Device::Controller) => String::from("Press a button…"),
        (Some(name), false, _) => name.clone(),
        (None, false, _) => String::from("Not set"),
    };

    let chip = button(text(label).size(13).align_x(Alignment::Center).width(Fill))
        .padding([5, 12])
        .width(CHIP_WIDTH)
        .style(theme::binding_chip(capturing, bound.is_none()))
        .on_press(Message::Settings(SettingsMessage::CaptureBinding(
            device, action,
        )));

    let clear: Element<'_, Message> = if bound.is_some() && !capturing {
        icon_button(
            Icon::X,
            String::from("Clear"),
            Some(Message::Settings(SettingsMessage::ClearBinding(
                device, action,
            ))),
        )
    } else {
        space().width(CLEAR_WIDTH).into()
    };

    row![text(action.label()).size(14).width(Fill), chip, clear]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
}

/// The library's folders with a rescan button and a row that adds one, and
/// the box art switch.
fn library_page<'a>(
    settings: &'a Settings,
    library: &'a Library,
    box_art_problem: Option<&'a str>,
    rings: &Rings,
) -> Element<'a, Message> {
    let rescan = rings.around(
        small_button(
            Icon::RefreshCw,
            "Rescan",
            Message::Library(LibraryMessage::Rescan),
        ),
        SettingsRow::Rescan,
    );

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
            text("No folders yet. Add the folders that hold your ROMs.")
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

    let mut page = column![group(
        "ROM folders",
        Some(rescan),
        folders,
        Some("Nubo looks for .gb, .gbc and .zip files in these folders."),
    )]
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
            Some(
                "Pictures come from the libretro thumbnail collection. While this is on, the \
                 names of your games are sent to thumbnails.libretro.com, and the game lists \
                 used to find them come from GitHub.",
            ),
        ));
    }

    page.into()
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

/// A library folder with its name, path, subfolder switch and remove button.
fn folder_row(index: usize, folder: &LibraryFolder, missing: bool) -> Element<'_, Message> {
    let name = folder
        .path
        .file_name()
        .unwrap_or(folder.path.as_os_str())
        .to_string_lossy()
        .into_owned();

    let symbol = if missing {
        icon(Icon::TriangleAlert).style(|theme: &Theme| text::Style {
            color: Some(theme.palette().danger),
        })
    } else {
        icon(Icon::Folder).style(theme::accent_text)
    };

    let detail = if missing {
        format!("Not found: {}", folder.path.display())
    } else {
        folder.path.display().to_string()
    };

    row![
        symbol,
        column![
            text(name).size(14),
            text(detail).size(12).style(theme::muted_text)
        ]
        .spacing(3)
        .width(Fill),
        text("Subfolders").size(12).style(theme::muted_text),
        switch(folder.include_subfolders, move |include| {
            Message::Library(LibraryMessage::IncludeSubfolders(index, include))
        }),
        icon_button(
            Icon::Trash2,
            String::from("Remove folder"),
            Some(Message::Library(LibraryMessage::RemoveFolder(index))),
        ),
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .into()
}

fn appearance_icon(appearance: Appearance) -> Icon {
    match appearance {
        Appearance::System => Icon::Monitor,
        Appearance::Light => Icon::Sun,
        Appearance::Dark => Icon::Moon,
    }
}

#[cfg(test)]
mod tests;
