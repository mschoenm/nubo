//! The settings window: opening and closing it, changing the settings on
//! its pages, and moving through them with the D-pad.

use super::{App, GameMessage, LibraryMessage, Message, cycle};
use crate::settings::Appearance;
use crate::views;
use crate::views::settings::{SettingsFocus, SettingsRow, SettingsTab};
use iced::Task;
use iced::widget;
use iced::widget::scrollable::RelativeOffset;
use nubo_frontend::controls::{Action, Controls, Device};
use nubo_frontend::model::ModelPreference;
use nubo_frontend::palettes::DmgPalette;
use nubo_frontend::settings::FastForwardSpeed;

/// How much the D-pad changes the volume per press.
const VOLUME_STEP: f32 = 0.05;

/// What the player does in the settings window, and the sound settings the
/// game's top bar changes too.
#[derive(Debug, Clone, Copy)]
pub enum SettingsMessage {
    Open,
    Close,
    TabSelected(SettingsTab),
    AppearanceSelected(Appearance),
    PaletteSelected(DmgPalette),
    IntegerScalingToggled(bool),
    SgbBorderToggled(bool),
    ModelSelected(ModelPreference),
    ResumeGamesToggled(bool),
    FastForwardSpeedSelected(FastForwardSpeed),
    /// The volume slider moved; the setting is saved once it's released.
    VolumeChanged(f32),
    VolumeReleased,
    /// The volume went up or down by this much with the D-pad.
    VolumeStepped(f32),
    ToggleMute,
    ControlsDeviceSelected(Device),
    /// Waits for the player to press the key or button for this action.
    CaptureBinding(Device, Action),
    ClearBinding(Device, Action),
    ResetControls(Device),
}

impl From<SettingsMessage> for Message {
    fn from(message: SettingsMessage) -> Self {
        Message::Settings(message)
    }
}

impl App {
    pub(super) fn update_settings(&mut self, message: SettingsMessage) -> Task<Message> {
        match message {
            SettingsMessage::Open => {
                if self.settings_tab.is_none() {
                    self.settings_tab = Some(SettingsTab::General);
                    self.settings_focus = None;
                }
            }
            SettingsMessage::Close => {
                self.settings_tab = None;
                self.capture = None;
            }
            SettingsMessage::TabSelected(tab) => {
                if self.settings_tab.is_none() {
                    self.settings_focus = None;
                } else if self.settings_focus.is_some() {
                    self.settings_focus = Some(SettingsFocus::Sidebar);
                }
                self.settings_tab = Some(tab);
                self.capture = None;
            }
            SettingsMessage::AppearanceSelected(appearance) => {
                self.change_settings(|settings| settings.appearance = appearance);
            }
            SettingsMessage::PaletteSelected(palette) => {
                self.change_settings(|settings| settings.shared.dmg_palette = palette);
                if let Some(game) = &mut self.game {
                    game.session.set_palette(palette);
                }
                return self.load_pictures(false);
            }
            SettingsMessage::IntegerScalingToggled(enabled) => {
                self.change_settings(|settings| settings.integer_scaling = enabled);
            }
            SettingsMessage::SgbBorderToggled(shown) => {
                self.change_settings(|settings| settings.shared.sgb_border = shown);
                if let Some(game) = &mut self.game {
                    game.session.set_sgb_border(shown);
                }
            }
            SettingsMessage::ModelSelected(model) => {
                self.model_override = None;
                self.change_settings(|settings| settings.shared.model = model);
            }
            SettingsMessage::ResumeGamesToggled(resume) => {
                self.change_settings(|settings| settings.shared.resume_games = resume);
            }
            SettingsMessage::FastForwardSpeedSelected(speed) => {
                self.change_settings(|settings| settings.shared.fast_forward_speed = speed);
            }
            SettingsMessage::VolumeChanged(volume) => {
                self.settings.shared.volume = volume;
                self.settings.shared.muted = false;
            }
            SettingsMessage::VolumeReleased => self.change_settings(|_| {}),
            SettingsMessage::VolumeStepped(step) => {
                let volume = (self.settings.shared.effective_volume() + step).clamp(0.0, 1.0);
                self.change_settings(|settings| {
                    settings.shared.volume = volume;
                    settings.shared.muted = false;
                });
            }
            SettingsMessage::ToggleMute => {
                let muted = !self.settings.shared.muted;
                self.change_settings(|settings| settings.shared.muted = muted);
            }
            SettingsMessage::ControlsDeviceSelected(device) => {
                self.controls_device = device;
                self.capture = None;
            }
            SettingsMessage::CaptureBinding(device, action) => {
                self.capture = Some((device, action));
            }
            SettingsMessage::ClearBinding(device, action) => {
                self.capture = None;
                self.change_controls(|controls| controls.clear(device, action));
            }
            SettingsMessage::ResetControls(device) => {
                self.capture = None;
                self.change_controls(|controls| controls.reset(device));
            }
        }

        Task::none()
    }

    /// Changes the controls, saves them and hands them to the running game.
    pub(super) fn change_controls(&mut self, change: impl FnOnce(&mut Controls)) {
        self.change_settings(|settings| change(&mut settings.shared.controls));
        if let Some(game) = &mut self.game {
            game.session.set_controls(&self.settings.shared.controls);
        }
    }

    /// Moves through the settings window. On the tabs, up and down switch
    /// tabs, and right or A goes into the page. On the page, up and down
    /// move between rows, left and right change the row's value, A picks it,
    /// Select clears a key or removes a folder, and B goes back to the tabs
    /// (and from there closes the window).
    pub(super) fn navigate_settings(&mut self, action: Action) -> Task<Message> {
        let Some(tab) = self.settings_tab else {
            return Task::none();
        };
        let Some(focus) = self.settings_focus else {
            // The first press only shows where the D-pad is.
            self.settings_focus = Some(SettingsFocus::Sidebar);
            return Task::none();
        };
        let rows = views::settings::rows(
            tab,
            &self.settings,
            self.restart_needed(),
            self.controls_device,
        );

        match focus {
            SettingsFocus::Sidebar => match action {
                Action::Up | Action::Down => {
                    let tabs = SettingsTab::all();
                    let current = tabs.iter().position(|&known| known == tab).unwrap_or(0);
                    let step = if action == Action::Up { -1 } else { 1 };
                    let next = current.saturating_add_signed(step).min(tabs.len() - 1);
                    self.settings_tab = Some(tabs[next]);
                    self.capture = None;
                }
                Action::Right | Action::A | Action::Start if !rows.is_empty() => {
                    self.settings_focus = Some(SettingsFocus::Row(0));
                    return scroll_settings(0, rows.len());
                }
                Action::B => {
                    self.settings_tab = None;
                    self.settings_focus = None;
                    self.capture = None;
                }
                _ => {}
            },
            SettingsFocus::Row(index) => {
                let Some(&row) = rows.get(index.min(rows.len().saturating_sub(1))) else {
                    self.settings_focus = Some(SettingsFocus::Sidebar);
                    return Task::none();
                };
                let index = index.min(rows.len() - 1);

                match action {
                    Action::Up | Action::Down => {
                        let step = if action == Action::Up { -1 } else { 1 };
                        let next = index.saturating_add_signed(step).min(rows.len() - 1);
                        self.settings_focus = Some(SettingsFocus::Row(next));
                        return scroll_settings(next, rows.len());
                    }
                    Action::Left | Action::Right => {
                        let step = if action == Action::Left { -1 } else { 1 };
                        match self.adjust_setting(row, step, false) {
                            Some(message) => return self.update(message),
                            None if action == Action::Left => {
                                self.settings_focus = Some(SettingsFocus::Sidebar);
                            }
                            None => {}
                        }
                    }
                    Action::A | Action::Start => {
                        if let Some(message) = self.activate_setting(row) {
                            return self.update(message);
                        }
                    }
                    Action::Select => {
                        if let Some(message) = self.clear_setting(row) {
                            return self.update(message);
                        }
                    }
                    Action::B => self.settings_focus = Some(SettingsFocus::Sidebar),
                    _ => {}
                }
            }
        }

        Task::none()
    }

    /// What moving `step` options along does to a setting with options, or
    /// `None` for a row that has none. With `wrap`, going past the last
    /// option starts at the first again.
    fn adjust_setting(&self, row: SettingsRow, step: isize, wrap: bool) -> Option<Message> {
        let settings = &self.settings;
        let message = match row {
            SettingsRow::Appearance => SettingsMessage::AppearanceSelected(cycle(
                &Appearance::ALL,
                settings.appearance,
                step,
                wrap,
            )),
            SettingsRow::Palette => SettingsMessage::PaletteSelected(cycle(
                &DmgPalette::ALL,
                settings.shared.dmg_palette,
                step,
                wrap,
            )),
            SettingsRow::Model => SettingsMessage::ModelSelected(cycle(
                &views::settings::model_choices(),
                settings.shared.model,
                step,
                wrap,
            )),
            SettingsRow::FastForwardSpeed => SettingsMessage::FastForwardSpeedSelected(cycle(
                &FastForwardSpeed::ALL,
                settings.shared.fast_forward_speed,
                step,
                wrap,
            )),
            SettingsRow::Volume => SettingsMessage::VolumeStepped(step as f32 * VOLUME_STEP),
            SettingsRow::Device => SettingsMessage::ControlsDeviceSelected(cycle(
                &Device::ALL,
                self.controls_device,
                step,
                wrap,
            )),
            _ => return None,
        };
        Some(message.into())
    }

    /// What pressing A on a settings row does.
    fn activate_setting(&self, row: SettingsRow) -> Option<Message> {
        let settings = &self.settings;
        let device = self.controls_device;
        Some(match row {
            SettingsRow::SharpPixels => {
                SettingsMessage::IntegerScalingToggled(!settings.integer_scaling).into()
            }
            SettingsRow::SgbBorder => {
                SettingsMessage::SgbBorderToggled(!settings.shared.sgb_border).into()
            }
            SettingsRow::RestartNow => GameMessage::Reset.into(),
            SettingsRow::ResumeGames => {
                SettingsMessage::ResumeGamesToggled(!settings.shared.resume_games).into()
            }
            SettingsRow::Volume => SettingsMessage::ToggleMute.into(),
            SettingsRow::ResetControls => SettingsMessage::ResetControls(device).into(),
            SettingsRow::Binding(action) => SettingsMessage::CaptureBinding(device, action).into(),
            SettingsRow::Folder(index) => LibraryMessage::IncludeSubfolders(
                index,
                !settings
                    .shared
                    .library_folders
                    .get(index)?
                    .include_subfolders,
            )
            .into(),
            SettingsRow::AddFolder => LibraryMessage::AddFolder.into(),
            SettingsRow::Rescan => LibraryMessage::Rescan.into(),
            SettingsRow::BoxArt => {
                LibraryMessage::DownloadBoxArtToggled(!settings.shared.download_box_art).into()
            }
            _ => return self.adjust_setting(row, 1, true),
        })
    }

    /// What pressing Select on a settings row does: clear a key or button,
    /// or remove a folder.
    fn clear_setting(&self, row: SettingsRow) -> Option<Message> {
        match row {
            SettingsRow::Binding(action) => {
                Some(SettingsMessage::ClearBinding(self.controls_device, action).into())
            }
            SettingsRow::Folder(index) => Some(LibraryMessage::RemoveFolder(index).into()),
            _ => None,
        }
    }
}

/// Scrolls the settings page so row `index` of `count` is roughly in view.
/// Rows are spread fairly evenly over the page, so scrolling in proportion
/// is close enough and needs no knowledge of their sizes.
fn scroll_settings(index: usize, count: usize) -> Task<Message> {
    let fraction = if count > 1 {
        index as f32 / (count - 1) as f32
    } else {
        0.0
    };

    widget::operation::snap_to(
        views::settings::scroll_id(),
        RelativeOffset {
            x: None,
            y: Some(fraction),
        },
    )
}
