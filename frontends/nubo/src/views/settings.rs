//! The settings window, drawn over the current screen, with a tab per
//! group of settings.

use crate::app::{GameMessage, LibraryMessage, Message, SettingsMessage};
use crate::settings::{Appearance, Settings};
use crate::ui::icons::{Icon, icon};
use crate::ui::theme;
use crate::ui::widgets::{icon_button, secondary_button, section, segmented};
use iced::widget::{
    Column, button, column, container, pick_list, row, scrollable, slider, space, text, toggler,
};
use iced::{Alignment, Color, Element, Fill, Theme, widget};
use nubo_core::Model;
use nubo_frontend::controls::{Action, Device};
use nubo_frontend::library::{self, Library, box_art};
use nubo_frontend::model::ModelPreference;
use nubo_frontend::palettes::DmgPalette;
use nubo_frontend::save_states;
use nubo_frontend::settings::{FastForwardSpeed, LibraryFolder};
use std::fmt;

const WIDTH: f32 = 700.0;
const HEIGHT: f32 = 480.0;
const SIDEBAR_WIDTH: f32 = 170.0;

/// A page of the settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Controls,
    Library,
}

impl SettingsTab {
    /// The tabs to show. The browser build has no library to set up.
    pub fn all() -> &'static [SettingsTab] {
        if library::AVAILABLE {
            &[
                SettingsTab::General,
                SettingsTab::Controls,
                SettingsTab::Library,
            ]
        } else {
            &[SettingsTab::General, SettingsTab::Controls]
        }
    }

    fn label(self) -> &'static str {
        match self {
            SettingsTab::General => "General",
            SettingsTab::Controls => "Controls",
            SettingsTab::Library => "Library",
        }
    }

    fn icon(self) -> Icon {
        match self {
            SettingsTab::General => Icon::Settings2,
            SettingsTab::Controls => Icon::Gamepad2,
            SettingsTab::Library => Icon::LibraryBig,
        }
    }
}

/// A model preference as the model picker lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ModelChoice(ModelPreference);

impl fmt::Display for ModelChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            ModelPreference::Automatic => {
                formatter.write_str("Automatic (each game on the Game Boy it was made for)")
            }
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
    /// A library folder, by its place in the list.
    Folder(usize),
    AddFolder,
    Rescan,
    BoxArt,
}

/// The rows of `tab` in the order the D-pad moves through them, which is
/// the order the page shows them in (a test checks that the pages draw them
/// in this order). `restart_needed` and `device` are as in [`view`] and
/// [`ControlsPage`].
pub fn rows(
    tab: SettingsTab,
    settings: &Settings,
    restart_needed: bool,
    device: Device,
) -> Vec<SettingsRow> {
    let mut rows = Vec::new();

    match tab {
        SettingsTab::General => {
            rows.extend([
                SettingsRow::Appearance,
                SettingsRow::Palette,
                SettingsRow::SharpPixels,
                SettingsRow::SgbBorder,
                SettingsRow::Model,
            ]);
            if restart_needed {
                rows.push(SettingsRow::RestartNow);
            }
            if save_states::AVAILABLE {
                rows.push(SettingsRow::ResumeGames);
            }
            rows.extend([SettingsRow::FastForwardSpeed, SettingsRow::Volume]);
        }
        SettingsTab::Controls => {
            rows.extend([SettingsRow::Device, SettingsRow::ResetControls]);
            rows.extend(Action::GAME_BOY.map(SettingsRow::Binding));
            rows.extend(shortcuts(device).iter().copied().map(SettingsRow::Binding));
        }
        SettingsTab::Library => {
            rows.extend((0..settings.shared.library_folders.len()).map(SettingsRow::Folder));
            rows.extend([SettingsRow::AddFolder, SettingsRow::Rescan]);
            if box_art::AVAILABLE {
                rows.push(SettingsRow::BoxArt);
            }
        }
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

/// What the Controls tab shows besides the settings.
pub struct ControlsPage<'a> {
    /// Whether it shows the keyboard or the controller.
    pub device: Device,
    /// The action waiting for a key or button, if any.
    pub capture: Option<(Device, Action)>,
    /// The names of the connected controllers.
    pub controllers: &'a [String],
}

/// The settings window showing `tab`. `restart_needed` says whether the
/// running game uses another model than the selected one, so it should offer
/// a restart.
pub fn view<'a>(
    tab: SettingsTab,
    settings: &'a Settings,
    library: &'a Library,
    restart_needed: bool,
    controls_page: &ControlsPage<'a>,
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
            rows(tab, settings, restart_needed, controls_page.device)
                .get(index)
                .copied()
        }
        _ => None,
    };
    let page = page(
        tab,
        settings,
        library,
        restart_needed,
        controls_page,
        &Rings::new(focused),
    );

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
        .height(HEIGHT)
        .style(theme::panel)
        .into()
}

/// The list of tabs at the left of the window, outlined while the D-pad is
/// on it.
fn sidebar<'a>(selected: SettingsTab, focused: bool) -> Element<'a, Message> {
    let tabs = SettingsTab::all().iter().map(|&tab| {
        button(
            row![icon(tab.icon()), text(tab.label())]
                .spacing(10)
                .align_y(Alignment::Center),
        )
        .padding([8, 12])
        .width(Fill)
        .style(theme::segment(tab == selected))
        .on_press(Message::Settings(SettingsMessage::TabSelected(tab)))
        .into()
    });

    focus_ring(
        Column::with_children(tabs).spacing(4).width(SIDEBAR_WIDTH),
        focused,
    )
}

/// The page of settings for `tab`, which draws its rows in the order
/// [`rows`] lists them.
fn page<'a>(
    tab: SettingsTab,
    settings: &'a Settings,
    library: &'a Library,
    restart_needed: bool,
    controls_page: &ControlsPage<'a>,
    rings: &Rings,
) -> Element<'a, Message> {
    match tab {
        SettingsTab::General => general(settings, restart_needed, rings),
        SettingsTab::Controls => controls(settings, controls_page, rings),
        SettingsTab::Library => library_folders(settings, library, rings),
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

    /// `content`, the next row of the page, outlined while the D-pad is on
    /// it.
    fn around<'a>(
        &self,
        content: impl Into<Element<'a, Message>>,
        row: SettingsRow,
    ) -> Element<'a, Message> {
        focus_ring(content, self.is_focused(row))
    }
}

fn general<'a>(
    settings: &'a Settings,
    restart_needed: bool,
    rings: &Rings,
) -> Element<'a, Message> {
    let appearance = rings.around(
        segmented(
            &Appearance::ALL,
            settings.appearance,
            |appearance| Some(appearance_icon(appearance)),
            |appearance| Message::Settings(SettingsMessage::AppearanceSelected(appearance)),
        ),
        SettingsRow::Appearance,
    );

    column![
        section("Appearance", appearance),
        section("Screen", screen_settings(settings, rings)),
        section(
            "Hardware",
            hardware_settings(settings, restart_needed, rings)
        ),
        section("Playing", playing_settings(settings, rings)),
        section("Sound", sound_settings(settings, rings)),
        section("About", about()),
    ]
    .spacing(24)
    .into()
}

/// The palette for original Game Boy games, the sharp pixels switch and the
/// Super Game Boy border switch.
fn screen_settings<'a>(settings: &Settings, rings: &Rings) -> Element<'a, Message> {
    let palettes = row(DmgPalette::ALL
        .into_iter()
        .map(|palette| palette_button(palette, palette == settings.shared.dmg_palette)))
    .spacing(8);

    column![
        column![
            text("Colors for Game Boy games").size(14),
            rings.around(palettes, SettingsRow::Palette),
            text("Game Boy Color games keep their own colors.")
                .size(12)
                .style(theme::muted_text),
        ]
        .spacing(8),
        rings.around(
            toggler(settings.integer_scaling)
                .label("Sharp pixels (grow the screen in whole steps)")
                .text_size(14)
                .size(18)
                .on_toggle(
                    |enabled| Message::Settings(SettingsMessage::IntegerScalingToggled(enabled))
                ),
            SettingsRow::SharpPixels,
        ),
        rings.around(
            toggler(settings.shared.sgb_border)
                .label("Super Game Boy border around the screen")
                .text_size(14)
                .size(18)
                .on_toggle(|shown| Message::Settings(SettingsMessage::SgbBorderToggled(shown))),
            SettingsRow::SgbBorder,
        ),
    ]
    .spacing(14)
    .into()
}

/// The model picker, and a restart button while the running game uses
/// another model.
fn hardware_settings<'a>(
    settings: &Settings,
    restart_needed: bool,
    rings: &Rings,
) -> Element<'a, Message> {
    let choices: Vec<ModelChoice> = model_choices().into_iter().map(ModelChoice).collect();

    let mut hardware = column![
        rings.around(
            pick_list(
                choices,
                Some(ModelChoice(settings.shared.model)),
                |choice| { Message::Settings(SettingsMessage::ModelSelected(choice.0)) }
            )
            .width(Fill),
            SettingsRow::Model,
        ),
        text("Games start on this model. It also applies when a game is reset.")
            .size(12)
            .style(theme::muted_text),
    ]
    .spacing(8);

    if restart_needed {
        hardware = hardware.push(
            row![
                text("The running game still uses the previous model.")
                    .size(13)
                    .width(Fill),
                rings.around(
                    secondary_button(
                        Icon::RotateCcw,
                        "Restart now",
                        Message::Game(GameMessage::Reset)
                    ),
                    SettingsRow::RestartNow,
                ),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        );
    }

    hardware.into()
}

/// Continuing games where the player left off, and the fast forward speed.
fn playing_settings<'a>(settings: &Settings, rings: &Rings) -> Element<'a, Message> {
    let mut playing = column![].spacing(12);

    if save_states::AVAILABLE {
        playing = playing.push(
            rings.around(
                toggler(settings.shared.resume_games)
                    .label("Continue games where you left off")
                    .text_size(14)
                    .size(18)
                    .on_toggle(|resume| {
                        Message::Settings(SettingsMessage::ResumeGamesToggled(resume))
                    }),
                SettingsRow::ResumeGames,
            ),
        );
    }

    playing
        .push(
            column![
                text("Fast forward speed").size(14),
                rings.around(
                    segmented(
                        &FastForwardSpeed::ALL,
                        settings.shared.fast_forward_speed,
                        |_| None,
                        |speed| Message::Settings(SettingsMessage::FastForwardSpeedSelected(speed)),
                    ),
                    SettingsRow::FastForwardSpeed,
                ),
                text("Hold Tab to fast forward, or use the button in the top bar.")
                    .size(12)
                    .style(theme::muted_text),
            ]
            .spacing(8),
        )
        .into()
}

/// The volume slider, with the mute state as its icon.
fn sound_settings<'a>(settings: &Settings, rings: &Rings) -> Element<'a, Message> {
    let volume_percent = (settings.shared.effective_volume() * 100.0).round();
    let sound = row![
        icon(if settings.shared.muted {
            Icon::VolumeX
        } else {
            Icon::Volume2
        }),
        slider(
            0.0..=1.0_f32,
            settings.shared.effective_volume(),
            |volume| { Message::Settings(SettingsMessage::VolumeChanged(volume)) }
        )
        .step(0.01_f32)
        .on_release(Message::Settings(SettingsMessage::VolumeReleased)),
        text(format!("{volume_percent}%"))
            .size(13)
            .width(44)
            .align_x(Alignment::End),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    rings.around(sound, SettingsRow::Volume)
}

/// The version, the copyright, the license, where to read it and where to get
/// the source code, which the GPL asks every copy to show.
fn about<'a>() -> Element<'a, Message> {
    column![
        text(concat!("Nubo ", env!("CARGO_PKG_VERSION"))).size(14),
        text(
            "Copyright 2026 Michel Schoenmakers. Based on RetroBoy, copyright 2023-2026 \
             Samuel Parsons. Nubo is free software: you can share and change it under \
             the GNU General Public License, version 3 or later. It comes with no warranty.",
        )
        .size(12)
        .style(theme::muted_text),
        text(
            "The license is at https://www.gnu.org/licenses/gpl-3.0.html. Copies of Nubo \
             include it as LICENSE.txt, with NOTICE.txt and THIRD_PARTY_LICENSES.txt, which \
             list the code from other projects in Nubo and their licenses.",
        )
        .size(12)
        .style(theme::muted_text),
        text(concat!("Source code: ", env!("CARGO_PKG_REPOSITORY")))
            .size(12)
            .style(theme::muted_text),
    ]
    .spacing(6)
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

fn controls<'a>(
    settings: &'a Settings,
    page: &ControlsPage<'a>,
    rings: &Rings,
) -> Element<'a, Message> {
    let device = page.device;
    let ring = |content, row| rings.around(content, row);

    let header = row![
        ring(
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
        ),
        space::horizontal(),
        ring(
            secondary_button(
                Icon::RotateCcw,
                "Reset to defaults",
                Message::Settings(SettingsMessage::ResetControls(device))
            ),
            SettingsRow::ResetControls,
        ),
    ]
    .align_y(Alignment::Center);

    let hint = match device {
        Device::Keyboard => String::from(
            "Pick a key, then press the new one. Esc cancels and Backspace clears. \
             Esc always opens the pause menu.",
        ),
        Device::Controller if page.controllers.is_empty() => String::from(
            "No controller connected. Pick a button, then press the new one on your \
             controller. The left stick always works as the D-pad.",
        ),
        Device::Controller => format!(
            "Connected: {}. Pick a button, then press the new one. The left stick \
             always works as the D-pad.",
            page.controllers.join(", ")
        ),
    };

    let rows = |actions: &[Action]| {
        Column::with_children(actions.iter().map(|&action| {
            ring(
                binding_row(
                    settings,
                    device,
                    action,
                    page.capture == Some((device, action)),
                ),
                SettingsRow::Binding(action),
            )
        }))
        .spacing(0)
    };

    column![
        header,
        text(hint).size(12).style(theme::muted_text),
        section("Game Boy", rows(&Action::GAME_BOY)),
        section("Shortcuts", rows(shortcuts(device))),
    ]
    .spacing(20)
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
        .width(150)
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
        space().width(34).into()
    };

    row![text(action.label()).size(14).width(Fill), chip, clear]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
}

fn library_folders<'a>(
    settings: &'a Settings,
    library: &'a Library,
    rings: &Rings,
) -> Element<'a, Message> {
    let ring = |content, row| rings.around(content, row);
    let folders: Element<'a, Message> = if settings.shared.library_folders.is_empty() {
        text("No folders yet. Add the folders that hold your ROMs.")
            .size(14)
            .style(theme::muted_text)
            .into()
    } else {
        Column::with_children(settings.shared.library_folders.iter().enumerate().map(
            |(index, folder)| {
                folder_row(
                    index,
                    folder,
                    library.is_missing(&folder.path),
                    rings.is_focused(SettingsRow::Folder(index)),
                )
            },
        ))
        .spacing(8)
        .into()
    };

    let actions = row![
        ring(
            secondary_button(
                Icon::FolderPlus,
                "Add folder…",
                Message::Library(LibraryMessage::AddFolder)
            ),
            SettingsRow::AddFolder,
        ),
        ring(
            secondary_button(
                Icon::RefreshCw,
                "Rescan",
                Message::Library(LibraryMessage::Rescan)
            ),
            SettingsRow::Rescan,
        ),
    ]
    .spacing(4);

    let mut page = column![
        section("ROM folders", column![folders, actions].spacing(12)),
        text("Nubo looks for .gb, .gbc and .zip files in these folders.")
            .size(12)
            .style(theme::muted_text),
    ]
    .spacing(16);

    if box_art::AVAILABLE {
        page = page.push(section(
            "Box art",
            column![
                ring(
                    toggler(settings.shared.download_box_art)
                        .label("Download box art for your games")
                        .text_size(14)
                        .size(18)
                        .on_toggle(|enabled| Message::Library(
                            LibraryMessage::DownloadBoxArtToggled(enabled)
                        ))
                        .into(),
                    SettingsRow::BoxArt,
                ),
                text(
                    "Pictures come from the libretro thumbnail collection. While this is on, \
                     the names of your games are sent to thumbnails.libretro.com, and the game \
                     lists used to find them come from GitHub. Games without box art show \
                     their title screen.",
                )
                .size(12)
                .style(theme::muted_text),
            ]
            .spacing(8),
        ));
    }

    page.into()
}

/// A library folder with its name, path, subfolder switch and remove button,
/// outlined while the D-pad is on it.
fn folder_row(
    index: usize,
    folder: &LibraryFolder,
    missing: bool,
    focused: bool,
) -> Element<'_, Message> {
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

    let row = row![
        symbol,
        column![
            text(name).size(14),
            text(detail).size(12).style(theme::muted_text)
        ]
        .spacing(2)
        .width(Fill),
        toggler(folder.include_subfolders)
            .label("Subfolders")
            .text_size(13)
            .size(18)
            .on_toggle(
                move |include| Message::Library(LibraryMessage::IncludeSubfolders(index, include))
            ),
        icon_button(
            Icon::Trash2,
            String::from("Remove folder"),
            Some(Message::Library(LibraryMessage::RemoveFolder(index))),
        ),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    container(row)
        .padding([8, 12])
        .style(theme::selectable_row(focused))
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
