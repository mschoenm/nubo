//! The settings window: opening and closing it, changing the settings on
//! its pages, and moving through them with the D-pad.

use super::playing::UndoPoint;
use super::{App, GameMessage, LibraryMessage, Message, cycle, scroll_in_proportion};
use crate::dialog::ResetChoices;
use crate::selection;
use crate::settings::{Appearance, Settings};
use crate::views;
use crate::views::settings::ModelOption;
use crate::views::settings::{SettingsFocus, SettingsRow, SettingsTab};
use iced::Task;
use iced::widget::operation;
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
#[derive(Debug, Clone)]
pub enum SettingsMessage {
    Open,
    Close,
    TabSelected(SettingsTab),
    AppearanceSelected(Appearance),
    PaletteSelected(DmgPalette),
    IntegerScalingToggled(bool),
    /// The player turned the original screen look on or off.
    OriginalScreenToggled(bool),
    /// The player turned looking for newer versions on or off.
    CheckForUpdatesToggled(bool),
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
    /// The saves a reset deletes in the background are gone: what else the
    /// reset did, what it deleted, and why anything wasn't (see
    /// `reset_summary`).
    SavesDeleted {
        done: Vec<&'static str>,
        deleted: Vec<&'static str>,
        errors: Vec<String>,
    },
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
                return self.open_settings_tab(tab);
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
            SettingsMessage::OriginalScreenToggled(enabled) => {
                self.change_settings(|settings| settings.shared.original_screen = enabled);
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
                self.capture_started = iced::time::Instant::now();
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
                return self.ask_reset();
            }
            SettingsMessage::Reset(choices) => return self.reset_settings(choices),
            SettingsMessage::SavesDeleted {
                mut done,
                deleted,
                errors,
            } => {
                self.deletions_running = self.deletions_running.saturating_sub(1);
                for error in errors {
                    self.toasts.error(error);
                }
                done.extend(deleted);
                if !done.is_empty() {
                    self.toasts.info(reset_summary(&done));
                }
                if self.deletions_running == 0
                    && let Some((rom, resume)) = self.start_after_deleting.take()
                {
                    self.start_game(rom, resume);
                }
            }
        }

        Task::none()
    }

    /// Starts over with what `choices` ticks, as far as this build and run
    /// offer it (see `App::reset_offers`): puts every setting back to how
    /// it started (see `Settings::reset`) and forgets the model picked for
    /// each game, removes the library folders, clears "Continue playing" and
    /// deletes the saves. Saves are deleted in the background, which can
    /// take a while with many games (see `SettingsMessage::SavesDeleted`).
    /// Unless something was deleted, the toast that says what was done
    /// offers to undo it all.
    fn reset_settings(&mut self, choices: ResetChoices) -> Task<Message> {
        let choices = choices.within(self.reset_offers());
        let before = UndoPoint::Settings {
            settings: Box::new(self.settings.clone()),
            library: self.keeps_library().then(|| self.library.snapshot()),
            remembered: self.remembered.clone(),
        };

        let mut done = Vec::new();
        if choices.reset_settings {
            done.push("settings reset");
        }
        if self.keeps_library()
            && (choices.reset_settings || choices.clear_recently_played)
            && let Err(error) = self
                .library
                .reset_games(choices.reset_settings, choices.clear_recently_played)
        {
            self.toasts.error(error);
        }
        if choices.clear_recently_played {
            if rom::REMEMBERS_LAST_GAME {
                match rom::forget() {
                    Ok(()) => {
                        self.remembered = None;
                        done.push("last game forgotten");
                    }
                    Err(error) => self.toasts.error(error),
                }
            } else {
                done.push("Continue playing cleared");
            }
        }
        if choices.remove_folders {
            done.push("game folders removed");
        }

        if choices.reset_settings || choices.remove_folders {
            let mut settings = if choices.reset_settings {
                self.model_override = None;
                self.settings.reset(false)
            } else {
                self.settings.clone()
            };
            if choices.remove_folders {
                settings.shared.library_folders.clear();
            }
            self.change_settings(|current| *current = settings);
        }

        let (delete_battery_saves, delete_save_states) =
            (choices.delete_battery_saves, choices.delete_save_states);
        let deletion = if choices.deletes() {
            // An earlier reset's Undo would put back settings and games from
            // before this reset, next to saves it can't put back. The toast
            // comes once the saves are deleted.
            self.undo = None;
            self.toasts.forget_undo();
            self.deletions_running += 1;
            let done = done.clone();
            Task::perform(
                background::run(move || delete_saves(delete_battery_saves, delete_save_states)),
                move |result| {
                    let (deleted, errors) = result.unwrap_or_else(|error| {
                        (
                            Vec::new(),
                            vec![format!("Couldn't delete the saves: {error}")],
                        )
                    });
                    SettingsMessage::SavesDeleted {
                        done,
                        deleted,
                        errors,
                    }
                    .into()
                },
            )
        } else {
            if !done.is_empty() {
                self.undo = Some(before);
                self.toasts.info_with_undo(reset_summary(&done));
            }
            Task::none()
        };
        Task::batch([deletion, self.settings_replaced()])
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
        self.toasts.info("Put back as it was");
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

    /// Changes the controls, saves them and hands them to the running game.
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
    /// window). Moving through the tabs or the rows wraps around their ends
    /// unless the key is `held` (see `crate::selection`).
    pub(super) fn navigate_settings(&mut self, action: Action, held: bool) -> Task<Message> {
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
                    let next = selection::moved(current, step, tabs.len(), !held);
                    return self.open_settings_tab(tabs[next]);
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
                        let next = selection::moved(index, step, rows.len(), !held);
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
            SettingsRow::OriginalScreen => {
                SettingsMessage::OriginalScreenToggled(!settings.shared.original_screen).into()
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

    /// Shows `tab` in the settings window, from the top of its page when it
    /// wasn't open already: every page scrolls in the same place, which
    /// would otherwise keep the last page's position.
    fn open_settings_tab(&mut self, tab: SettingsTab) -> Task<Message> {
        let changed = self.settings_tab != Some(tab);
        self.settings_tab = Some(tab);
        self.capture = None;
        if changed {
            operation::snap_to(views::settings::scroll_id(), RelativeOffset::START)
        } else {
            Task::none()
        }
    }
}

/// Scrolls the settings page so row `index` of `count` is roughly in view
/// (see `scroll_in_proportion`).
fn scroll_settings(index: usize, count: usize) -> Task<Message> {
    scroll_in_proportion(views::settings::scroll_id(), index, count)
}

/// What a reset did, as a sentence for its toast: the `done` parts, such as
/// "settings reset" and "save states deleted", joined with commas and a
/// final "and", starting with a capital.
pub(super) fn reset_summary(done: &[&str]) -> String {
    let sentence = match done {
        [] => String::new(),
        [only] => String::from(*only),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    let mut letters = sentence.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => sentence,
    }
}

/// Deletes every game's battery saves and save states, as asked, returning
/// what went, such as "save states deleted", and why anything didn't. It
/// runs in the background.
fn delete_saves(
    delete_battery_saves: bool,
    delete_save_states: bool,
) -> (Vec<&'static str>, Vec<String>) {
    let mut deleted = Vec::new();
    let mut errors = Vec::new();
    if delete_battery_saves {
        match battery::delete_all() {
            Ok(()) => deleted.push("in-game saves deleted"),
            Err(error) => errors.push(error),
        }
    }
    if delete_save_states {
        match save_states::delete_all() {
            Ok(()) => deleted.push("save states deleted"),
            Err(error) => errors.push(error),
        }
    }
    (deleted, errors)
}
