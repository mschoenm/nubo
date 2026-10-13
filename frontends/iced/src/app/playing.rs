//! Starting, playing and stopping games: the game's top bar, the pause menu
//! with its save states and cheats, and opening ROMs.

use super::{App, Message};
use crate::cheats;
use crate::controls::Action;
use crate::game::{Game, PauseMenu};
use crate::rom::{self, Rom};
use crate::save_states::{self, Slot};
use crate::screenshots;
use crate::views;
use iced::Task;
use nubo_core::{CartridgeHeader, Emulator, Model};
use std::path::{Path, PathBuf};

/// What the player can do with games: open one, and control the one that
/// runs.
#[derive(Debug, Clone)]
pub enum GameMessage {
    OpenRom,
    RomPicked(Option<Rom>),
    /// A ROM from the library, the command line or a dropped file was read.
    RomRead(Result<Rom, String>),
    /// Starts the library game at this path.
    PlayGame(PathBuf),
    Reset,
    TogglePause,
    /// Stops the game and goes back to the library.
    CloseGame,
    ShowPauseMenu(PauseMenu),
    SaveState(Slot),
    LoadState(Slot),
    ToggleFastForward,
    Screenshot,
    CheatNameChanged(String),
    CheatCodeChanged(String),
    AddCheat,
    CheatToggled(usize, bool),
    RemoveCheat(usize),
}

impl From<GameMessage> for Message {
    fn from(message: GameMessage) -> Self {
        Message::Game(message)
    }
}

impl App {
    pub(super) fn update_game(&mut self, message: GameMessage) -> Task<Message> {
        match message {
            GameMessage::OpenRom => {
                return Task::perform(rom::pick(), |rom| GameMessage::RomPicked(rom).into());
            }
            GameMessage::RomPicked(Some(rom)) => self.start_game(rom, true),
            GameMessage::RomPicked(None) => {}
            GameMessage::RomRead(rom) => self.open(rom),
            GameMessage::PlayGame(path) => return read_rom(path),
            GameMessage::Reset => {
                if let Some(game) = &self.game {
                    let rom = game.rom().clone();
                    self.start_game(rom, false);
                }
            }
            GameMessage::TogglePause => {
                if let Some(game) = &mut self.game {
                    let page = if game.is_paused() {
                        None
                    } else {
                        Some(PauseMenu::Main)
                    };
                    game.show_menu(page);
                }
            }
            GameMessage::CloseGame => self.stop_game(),
            GameMessage::ShowPauseMenu(page) => {
                if let Some(game) = &mut self.game {
                    game.show_menu(Some(page));
                }
            }
            GameMessage::SaveState(slot) => self.save_state(slot),
            GameMessage::LoadState(slot) => self.load_state(slot),
            GameMessage::ToggleFastForward => {
                if let Some(game) = &mut self.game {
                    game.toggle_fast_forward();
                }
            }
            GameMessage::Screenshot => self.take_screenshot(),
            GameMessage::CheatNameChanged(name) => {
                if let Some(game) = &mut self.game {
                    game.cheat_form.name = name;
                }
            }
            GameMessage::CheatCodeChanged(code) => {
                if let Some(game) = &mut self.game {
                    game.cheat_form.code = code;
                    game.cheat_form.error = None;
                }
            }
            GameMessage::AddCheat => {
                if let Some(game) = &mut self.game
                    && let Err(error) = game.add_cheat()
                {
                    self.toasts.error(error);
                }
            }
            GameMessage::CheatToggled(index, enabled) => {
                if let Some(game) = &mut self.game
                    && let Err(error) = game.set_cheat_enabled(index, enabled)
                {
                    self.toasts.error(error);
                }
            }
            GameMessage::RemoveCheat(index) => {
                if let Some(game) = &mut self.game
                    && let Err(error) = game.remove_cheat(index)
                {
                    self.toasts.error(error);
                }
            }
        }

        Task::none()
    }

    /// Starts a ROM that was read, continuing where the player left off, or
    /// shows why it couldn't be read.
    pub(super) fn open(&mut self, rom: Result<Rom, String>) {
        match rom {
            Ok(rom) => self.start_game(rom, true),
            Err(error) => self.toasts.error(error),
        }
    }

    /// Starts `rom` on the model the settings pick for it. With `resume` and
    /// the "resume games" setting on, it continues from where the player
    /// left it, on the model it ran on then.
    ///
    /// Restarting the same ROM stops it first, so its battery save and
    /// where the player left off are written before it loads again and
    /// reads them; another ROM replaces the running game only once it has
    /// loaded.
    fn start_game(&mut self, rom: Rom, resume: bool) {
        let header = match Emulator::read_rom_header(&rom.bytes) {
            Ok(header) => header,
            Err(error) => {
                self.toasts
                    .error(format!("Couldn't start {}: {error}", rom.name()));
                return;
            }
        };

        if self
            .game
            .as_ref()
            .is_some_and(|game| game.rom().path == rom.path)
        {
            self.stop_game();
        }

        let resume_state = if resume && self.settings.resume_games {
            save_states::load(&rom.path, Slot::Resume).unwrap_or_else(|error| {
                self.keep_unloadable_state(&rom.path, Slot::Resume, error);
                None
            })
        } else {
            None
        };

        let model = resume_state
            .as_ref()
            .map_or_else(|| self.model_for(&header), |state| state.model);
        let path = rom.path.clone();

        match self.start_session(rom, header, model, true) {
            Ok(mut game) => {
                if let Some(state) = resume_state {
                    match game.load(state.data) {
                        Ok(()) => self.toasts.info("Continuing where you left off"),
                        Err(error) => self.keep_unloadable_state(
                            &path,
                            Slot::Resume,
                            format!("Couldn't continue where you left off: {error}"),
                        ),
                    }
                }

                self.stop_game();

                if let Err(error) = self.library.record_played(&path, game.header()) {
                    self.toasts.error(error);
                }

                self.game = Some(game);
                self.settings_tab = None;
            }
            Err(error) => self.toasts.error(format!(
                "Couldn't start {}: {error}",
                path.file_stem().unwrap_or_default().to_string_lossy()
            )),
        }

        self.show_notices();
    }

    /// Starts an emulator for `rom` with the player's controls, running (or
    /// paused, without `running`) as loud as the settings say.
    fn start_session(
        &mut self,
        rom: Rom,
        header: CartridgeHeader,
        model: Model,
        running: bool,
    ) -> Result<Game, String> {
        let mut game = Game::start(rom, header, model, &self.notices, running, &self.settings)?;
        game.set_controls(&self.settings.controls);
        game.set_palette(self.settings.dmg_palette);
        match cheats::load(&game.rom().path) {
            Ok(cheats) => game.set_cheats(cheats),
            Err(error) => self.toasts.error(error),
        }
        game.set_controller_state(self.menu_presses.hide_from(&self.controller_state));
        Ok(game)
    }

    /// Stops the running game. It saves where the player left off (if the
    /// settings say so) and keeps the last frame as the game's thumbnail.
    pub(super) fn stop_game(&mut self) {
        let Some(mut game) = self.game.take() else {
            return;
        };

        if self.settings.resume_games
            && save_states::AVAILABLE
            && let Err(error) = game.save(Slot::Resume)
        {
            self.toasts.error(error);
        }

        if let Some(pixels) = game.last_frame()
            && let Err(error) = self
                .library
                .set_thumbnail(&game.rom().path, pixels.to_vec())
        {
            self.toasts.error(error);
        }
    }

    /// Saves the running game in `slot` and resumes it.
    pub(super) fn save_state(&mut self, slot: Slot) {
        let Some(game) = &mut self.game else {
            return;
        };

        match game.save(slot) {
            Ok(()) => {
                game.show_menu(None);
                self.toasts.info(format!("Saved to {slot}"));
            }
            Err(error) => self.toasts.error(error),
        }
    }

    /// Loads the state in `slot` and resumes the game. A state made on
    /// another model restarts the game on that model first, stopping the
    /// running one before the new one starts so its battery save is written
    /// before the new one reads it.
    pub(super) fn load_state(&mut self, slot: Slot) {
        let Some(game) = &self.game else {
            return;
        };

        let rom_path = game.rom().path.clone();
        let state = match save_states::load(&rom_path, slot) {
            Ok(Some(state)) => state,
            Ok(None) => {
                self.toasts.info(format!("{slot} is empty"));
                return;
            }
            Err(error) => {
                self.keep_unloadable_state(&rom_path, slot, error);
                return;
            }
        };

        if state.model != game.model() {
            let (rom, header) = (game.rom().clone(), game.header().clone());
            self.game = None;
            // The game resumes once the state has loaded, unless the
            // settings are open.
            let running = self.settings_tab.is_none();
            match self.start_session(rom, header, state.model, running) {
                Ok(restarted) => self.game = Some(restarted),
                Err(error) => {
                    self.toasts.error(error);
                    return;
                }
            }
        }

        let Some(game) = &mut self.game else {
            return;
        };

        match game.load(state.data) {
            Ok(()) => {
                game.show_menu(None);
                self.toasts.info(format!("Loaded {slot}"));
            }
            Err(error) => {
                self.keep_unloadable_state(
                    &rom_path,
                    slot,
                    format!("Couldn't load {slot}: {error}"),
                );
            }
        }
    }

    /// Reports `message` about the state in `slot` that couldn't be loaded,
    /// and moves the state aside so the next save to the slot (when the game
    /// stops, for where the player left off) can't overwrite it.
    fn keep_unloadable_state(&mut self, rom_path: &Path, slot: Slot, message: String) {
        match save_states::keep_aside(rom_path, slot) {
            Ok(kept) => self
                .toasts
                .error(format!("{message}. The state was kept as {kept}")),
            Err(error) => {
                self.toasts.error(message);
                self.toasts.error(error);
            }
        }
    }

    /// Saves the current screen to the Pictures folder.
    pub(super) fn take_screenshot(&mut self) {
        let Some(game) = &mut self.game else {
            return;
        };
        // A controller can ask for this while the window is minimized and
        // doesn't show new frames.
        game.refresh_screen();
        let Some(pixels) = game.last_frame() else {
            return;
        };

        match screenshots::save(&game.name(), pixels) {
            Ok(path) => self.toasts.info(format!(
                "Screenshot saved as {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            )),
            Err(error) => self.toasts.error(error),
        }
    }

    /// Moves through the pause menu: up and down move between its rows, A
    /// or Start picks one, and B steps back.
    pub(super) fn navigate_pause_menu(&mut self, action: Action) -> Task<Message> {
        let Some(game) = &mut self.game else {
            return Task::none();
        };
        let Some(page) = game.menu() else {
            return Task::none();
        };

        let saving = page == PauseMenu::Save;
        let rows = match page {
            PauseMenu::Main => views::game::main_entries(&self.settings, self.fullscreen).len(),
            PauseMenu::Save | PauseMenu::Load => views::game::listed_slots(game, saving).len(),
            PauseMenu::Cheats => game.cheats().len(),
        };

        match action {
            Action::Up => game.move_menu_selection(-1, rows),
            Action::Down => game.move_menu_selection(1, rows),
            Action::B => self.escape(),
            Action::A | Action::Start => {
                let selection = game.menu_selection();
                let message = match page {
                    PauseMenu::Main => views::game::main_entries(&self.settings, self.fullscreen)
                        .into_iter()
                        .nth(selection)
                        .map(|entry| entry.message),
                    PauseMenu::Save | PauseMenu::Load => views::game::listed_slots(game, saving)
                        .get(selection)
                        .and_then(|info| views::game::slot_message(info, saving)),
                    PauseMenu::Cheats => game
                        .cheats()
                        .get(selection)
                        .map(|cheat| GameMessage::CheatToggled(selection, !cheat.enabled).into()),
                };

                if let Some(message) = message {
                    return self.update(message);
                }
            }
            _ => {}
        }

        Task::none()
    }
}

/// Reads the ROM at `path` in the background.
pub(super) fn read_rom(path: PathBuf) -> Task<Message> {
    Task::perform(async move { Rom::read(&path) }, |rom| {
        GameMessage::RomRead(rom).into()
    })
}
