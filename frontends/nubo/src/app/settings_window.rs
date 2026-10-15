//! The settings window: opening and closing it, changing the settings on
//! its pages, and moving through them with the D-pad.

use super::playing::UndoPoint;
use super::{App, GameMessage, LibraryMessage, Message, cycle};
use crate::dialog::ResetChoices;
use crate::settings::{Appearance, Settings};
use crate::views;
use crate::views::settings::ModelOption;
use crate::views::settings::{SettingsFocus, SettingsRow, SettingsTab};
use iced::Task;
use iced::widget;
use iced::widget::scrollable::RelativeOffset;
use nubo_frontend::background;
use nubo_frontend::battery;
use nubo_frontend::controls::{Action, Controls, Device};
use nubo_frontend::library::LibrarySnapshot;
use nubo_frontend::model::ModelPreference;
use nubo_frontend::palettes::DmgPalette;
use nubo_frontend::rom::{self, Rom};
use nubo_frontend::save_states;
use nubo_frontend::settings::FastForwardSpeed;
use nubo_frontend::updates;

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
    ColorCorrectionToggled(bool),
    /// The player turned looking for newer versions on or off.
    CheckForUpdatesToggled(bool),
    FrameBlendingToggled(bool),
    SgbBorderToggled(bool),
    ModelSelected(ModelPreference),
    ResumeGamesToggled(bool),
    PauseInBackgroundToggled(bool),
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
    /// Asks whether to reset every setting, and what else.
    AskReset,
    /// Resets every setting and what else the player ticked, as they
    /// confirmed.
    Reset(ResetChoices),
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
                    self.settings_tab = Some(SettingsTab::Display);
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
                    self.settings_focus = Some(SettingsFocus::Tabs);
                }
                self.settings_tab = Some(tab);
                self.capture = None;
            }
            SettingsMessage::AppearanceSelected(appearance) => {
                self.change_settings(|settings| settings.appearance = appearance);
            }
            SettingsMessage::PaletteSelected(palette) => {
                self.change_settings(|settings| settings.shared.dmg_palette = palette);
                self.apply_screen_look();
                return self.load_pictures(false);
            }
            SettingsMessage::CheckForUpdatesToggled(check) => {
                self.change_settings(|settings| settings.shared.check_for_updates = Some(check));
                return self.look_for_updates();
            }
            SettingsMessage::ColorCorrectionToggled(enabled) => {
                self.change_settings(|settings| settings.shared.color_correction = enabled);
                self.apply_screen_look();
            }
            SettingsMessage::FrameBlendingToggled(enabled) => {
                self.change_settings(|settings| settings.shared.frame_blending = enabled);
                self.apply_screen_look();
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
            SettingsMessage::PauseInBackgroundToggled(pause) => {
                self.change_settings(|settings| settings.pause_in_background = pause);
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
            SettingsMessage::AskReset => {
                self.capture = None;
                self.ask_reset();
            }
            SettingsMessage::Reset(choices) => return self.reset_settings(choices),
        }

        Task::none()
    }

    /// Puts every setting back to how it started (see `Settings::reset`),
    /// forgets the model picked for each game, and does what else `choices`
    /// ticks, as far as this build and run offer it (see
    /// `App::reset_offers`). Unless something was deleted, the toast that
    /// says so offers to undo it all.
    fn reset_settings(&mut self, choices: ResetChoices) -> Task<Message> {
        let offers = self.reset_offers();
        let before = UndoPoint::Settings {
            settings: Box::new(self.settings.clone()),
            library: self.keeps_library().then(|| self.library.snapshot()),
            remembered: self.remembered.clone(),
        };

        let mut deleted = Vec::new();
        if choices.delete_battery_saves && offers.delete_battery_saves {
            match battery::delete_all() {
                Ok(()) => deleted.push("in-game saves"),
                Err(error) => self.toasts.error(error),
            }
        }
        if choices.delete_save_states && offers.delete_save_states {
            match save_states::delete_all() {
                Ok(()) => deleted.push("save states"),
                Err(error) => self.toasts.error(error),
            }
        }
        let clear_recent = choices.clear_recently_played && offers.clear_recently_played;
        if self.keeps_library()
            && let Err(error) = self.library.reset_games(clear_recent)
        {
            self.toasts.error(error);
        }
        if clear_recent && rom::REMEMBERS_LAST_GAME {
            match rom::forget() {
                Ok(()) => self.remembered = None,
                Err(error) => self.toasts.error(error),
            }
        }

        let remove_folders = choices.remove_folders && offers.remove_folders;
        let reset = self.settings.reset(remove_folders);
        self.model_override = None;
        self.change_settings(|settings| *settings = reset);

        if choices.deletes() {
            // An earlier reset's Undo would put back settings and games from
            // before this reset, next to saves it can't put back.
            self.undo = None;
            self.toasts.forget_undo();
            if !deleted.is_empty() {
                self.toasts.info(format!(
                    "Settings reset, and the {} deleted",
                    deleted.join(" and ")
                ));
            }
        } else {
            self.undo = Some(before);
            self.toasts
                .info_with_undo("Settings are back to how they started");
        }
        self.settings_replaced()
    }

    /// Puts back the settings, the library's games and the last game played
    /// as they were before a reset, for its Undo button.
    pub(super) fn restore_settings(
        &mut self,
        settings: Settings,
        library: Option<LibrarySnapshot>,
        remembered: Option<Rom>,
    ) -> Task<Message> {
        self.change_settings(|current| *current = settings);
        if let Some(snapshot) = library
            && let Err(error) = self.library.restore(snapshot)
        {
            self.toasts.error(error);
        }
        if let Some(rom) = remembered.filter(|_| self.remembered.is_none()) {
            if let Err(error) = rom::remember(&rom) {
                self.toasts.error(error);
            }
            self.remembered = Some(rom);
        }
        self.toasts.info("Settings put back");
        self.settings_replaced()
    }

    /// Catches up with settings replaced all at once, by a reset or its
    /// undo: the running game gets them, the library is scanned again for
    /// its folders and redrawn in the palette, and newer versions are
    /// looked for if that is on again.
    fn settings_replaced(&mut self) -> Task<Message> {
        self.capture = None;
        self.library_selection = None;
        self.apply_settings_to_game();
        Task::batch([
            self.rescan(),
            self.load_pictures(false),
            self.look_for_updates(),
        ])
    }

    /// Changes the controls, saves them and hands them to the running game.
    /// Looks for a newer version in the background, if the player wants
    /// that and this run hasn't yet.
    pub(super) fn look_for_updates(&mut self) -> Task<Message> {
        if !updates::AVAILABLE
            || self.fresh
            || self.settings.shared.check_for_updates != Some(true)
            || self.newer_version.is_some()
        {
            return Task::none();
        }
        Task::perform(background::run(updates::latest_version), |version| {
            Message::LatestVersion(version.flatten())
        })
    }

    /// Shows the running game with the palette and screen effects the
    /// settings pick (see `App::screen_look`).
    fn apply_screen_look(&mut self) {
        let look = self.screen_look();
        if let Some(game) = &mut self.game {
            game.session.set_screen_look(look);
        }
    }

    pub(super) fn change_controls(&mut self, change: impl FnOnce(&mut Controls)) {
        self.change_settings(|settings| change(&mut settings.shared.controls));
        if let Some(game) = &mut self.game {
            game.session.set_controls(&self.settings.shared.controls);
        }
    }

    /// Moves through the settings window. On the tabs, up and down switch
    /// tabs in the sidebar and left and right in the row above the page, and
    /// right or down (toward the page) or A goes into it (see
    /// `SettingsLayout`). On the page, up and down move between rows (up
    /// from the first goes back to a row of tabs above it), left and right
    /// change the row's value, A picks it, Select clears a key or removes a
    /// folder, and B goes back to the tabs (and from there closes the
    /// window).
    pub(super) fn navigate_settings(&mut self, action: Action) -> Task<Message> {
        let Some(tab) = self.settings_tab else {
            return Task::none();
        };
        let Some(focus) = self.settings_focus else {
            // The first press only shows where the D-pad is.
            self.settings_focus = Some(SettingsFocus::Tabs);
            return Task::none();
        };
        let state = self.settings_page_state();
        let layout = state.layout;
        let rows = views::settings::rows(tab, &self.settings, &state);

        match focus {
            SettingsFocus::Tabs => {
                if let Some(step) = layout.tab_step(action) {
                    let tabs = SettingsTab::all();
                    let current = tabs.iter().position(|&known| known == tab).unwrap_or(0);
                    let next = current.saturating_add_signed(step).min(tabs.len() - 1);
                    self.settings_tab = Some(tabs[next]);
                    self.capture = None;
                } else if layout.enters_page(action) && !rows.is_empty() {
                    self.settings_focus = Some(SettingsFocus::Row(0));
                    return scroll_settings(0, rows.len());
                } else if action == Action::B {
                    self.settings_tab = None;
                    self.settings_focus = None;
                    self.capture = None;
                }
            }
            SettingsFocus::Row(index) => {
                let Some(&row) = rows.get(index.min(rows.len().saturating_sub(1))) else {
                    self.settings_focus = Some(SettingsFocus::Tabs);
                    return Task::none();
                };
                let index = index.min(rows.len() - 1);

                match action {
                    Action::Up if index == 0 && layout.up_leaves_page() => {
                        self.settings_focus = Some(SettingsFocus::Tabs);
                    }
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
                                self.settings_focus = Some(SettingsFocus::Tabs);
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
                    Action::B => self.settings_focus = Some(SettingsFocus::Tabs),
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
            SettingsRow::Model => SettingsMessage::ModelSelected(
                cycle(
                    &ModelOption::ALL,
                    ModelOption::of(settings.shared.model),
                    step,
                    wrap,
                )
                .preference(settings.shared.model),
            ),
            SettingsRow::ModelRevision => {
                let ModelPreference::Fixed(current) = settings.shared.model else {
                    return None;
                };
                SettingsMessage::ModelSelected(ModelPreference::Fixed(cycle(
                    views::settings::revisions(settings.shared.model),
                    current,
                    step,
                    wrap,
                )))
            }
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
            SettingsRow::ColorCorrection => {
                SettingsMessage::ColorCorrectionToggled(!settings.shared.color_correction).into()
            }
            SettingsRow::FrameBlending => {
                SettingsMessage::FrameBlendingToggled(!settings.shared.frame_blending).into()
            }
            SettingsRow::SgbBorder => {
                SettingsMessage::SgbBorderToggled(!settings.shared.sgb_border).into()
            }
            SettingsRow::RestartNow => GameMessage::Reset.into(),
            SettingsRow::ResumeGames => {
                SettingsMessage::ResumeGamesToggled(!settings.shared.resume_games).into()
            }
            SettingsRow::PauseInBackground => {
                SettingsMessage::PauseInBackgroundToggled(!settings.pause_in_background).into()
            }
            SettingsRow::Volume => SettingsMessage::ToggleMute.into(),
            SettingsRow::ResetControls => SettingsMessage::ResetControls(device).into(),
            SettingsRow::Binding(action) => SettingsMessage::CaptureBinding(device, action).into(),
            SettingsRow::Folder(index) => LibraryMessage::OpenFolderMenu(index).into(),
            SettingsRow::AddFolder => LibraryMessage::AddFolder.into(),
            SettingsRow::BoxArt => {
                LibraryMessage::DownloadBoxArtToggled(!settings.shared.download_box_art).into()
            }
            SettingsRow::RetryBoxArt => LibraryMessage::RetryBoxArt.into(),
            SettingsRow::CheckForUpdates => SettingsMessage::CheckForUpdatesToggled(
                settings.shared.check_for_updates != Some(true),
            )
            .into(),
            SettingsRow::ResetSettings => SettingsMessage::AskReset.into(),
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
