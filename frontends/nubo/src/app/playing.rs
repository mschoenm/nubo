//! Starting, playing and stopping games: the game's top bar, the pause menu
//! with its save states and cheats, and opening ROMs.

use super::{App, Message};
use crate::game::{Game, PauseMenu};
use crate::rom_picker;
use crate::views;
use iced::Task;
use nubo_core::{CartridgeHeader, Emulator, Model};
use nubo_frontend::cheats;
use nubo_frontend::controls::Action;
use nubo_frontend::picture::Picture;
use nubo_frontend::rom::Rom;
use nubo_frontend::save_states::{self, Slot};
use nubo_frontend::screenshots;
use nubo_frontend::session::{GameSession, StartOptions};
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
    /// The pictures of the save state slots a pause menu page lists were
    /// read, for this opening of the page (see
    /// `GameSession::thumbnails_to_load`).
    ThumbnailsLoaded(u64, Vec<(Slot, Option<Picture>)>),
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
                return Task::perform(rom_picker::pick(), |rom| GameMessage::RomPicked(rom).into());
            }
            GameMessage::RomPicked(Some(rom)) => self.start_game(rom, true),
            GameMessage::RomPicked(None) => {}
            GameMessage::RomRead(rom) => self.open(rom),
            GameMessage::PlayGame(path) => return read_rom(path),
            GameMessage::Reset => {
                if let Some(game) = &self.game {
                    let rom = game.session.rom().clone();
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
            GameMessage::ThumbnailsLoaded(slot_listing, thumbnails) => {
                if let Some(game) = &mut self.game {
                    game.session.set_thumbnails(slot_listing, thumbnails);
                }
            }
            GameMessage::SaveState(slot) => self.save_state(slot),
            GameMessage::LoadState(slot) => self.load_state(slot),
            GameMessage::ToggleFastForward => {
                if let Some(game) = &mut self.game {
                    game.session.toggle_fast_forward();
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
                    && let Err(error) = game.session.set_cheat_enabled(index, enabled)
                {
                    self.toasts.error(error);
                }
            }
            GameMessage::RemoveCheat(index) => {
                if let Some(game) = &mut self.game
                    && let Err(error) = game.session.remove_cheat(index)
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
    /// left it, on the model it ran on then (never with `--fresh`).
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
            .is_some_and(|game| game.session.rom().path == rom.path)
        {
            self.stop_game();
        }

        let resume_state = if resume && self.settings.shared.resume_games && !self.fresh {
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
                    match game.session.load(state.data) {
                        Ok(()) => self.toasts.info("Continuing where you left off"),
                        Err(error) => self.keep_unloadable_state(
                            &path,
                            Slot::Resume,
                            format!("Couldn't continue where you left off: {error}"),
                        ),
                    }
                }

                self.stop_game();

                if !self.fresh
                    && let Err(error) = self.library.record_played(&path, game.session.header())
                {
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
    /// paused, without `running`) as loud as the settings say, and fast
    /// forwarding at the speed asked for on the command line, if any. With
    /// `--fresh` it keeps no battery saves, cheats, save states or
    /// screenshots.
    fn start_session(
        &mut self,
        rom: Rom,
        header: CartridgeHeader,
        model: Model,
        running: bool,
    ) -> Result<Game, String> {
        let mut session = GameSession::start(
            rom,
            header,
            model,
            StartOptions {
                keep_saves: !self.fresh,
                fast_forward: self.speed,
            },
            &self.notices,
            running,
            &self.settings.shared,
        )?;
        session.set_controls(&self.settings.shared.controls);
        session.set_palette(self.settings.shared.dmg_palette);
        if !self.fresh {
            match cheats::load(&session.rom().path) {
                Ok(cheats) => session.set_cheats(cheats),
                Err(error) => self.toasts.error(error),
            }
        }
        session.set_controller_state(self.menu_presses.hide_from(&self.controller_state));
        Ok(Game::new(session, !self.fresh))
    }

    /// Stops the running game. It saves where the player left off (if the
    /// settings say so) and keeps the last frame as the game's thumbnail,
    /// unless it keeps nothing (`--fresh`).
    pub(super) fn stop_game(&mut self) {
        let Some(mut game) = self.game.take() else {
            return;
        };
        if !game.keeps_data {
            return;
        }

        if self.settings.shared.resume_games
            && save_states::AVAILABLE
            && let Err(error) = game.session.save(Slot::Resume)
        {
            self.toasts.error(error);
        }

        if let Some(pixels) = game.session.last_frame()
            && let Err(error) = self
                .library
                .set_thumbnail(&game.session.rom().path, pixels.to_vec())
        {
            self.toasts.error(error);
        }
    }

    /// Saves the running game in `slot` and resumes it, unless it keeps no
    /// save states (`--fresh`).
    pub(super) fn save_state(&mut self, slot: Slot) {
        let Some(game) = self.game.as_mut().filter(|game| game.keeps_data) else {
            return;
        };

        match game.session.save(slot) {
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
        let Some(game) = self.game.as_ref().filter(|game| game.keeps_data) else {
            return;
        };

        let rom_path = game.session.rom().path.clone();
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

        if state.model != game.session.model() {
            let (rom, header) = (game.session.rom().clone(), game.session.header().clone());
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

        match game.session.load(state.data) {
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

    /// Saves the current screen to the Pictures folder, unless the game
    /// keeps no screenshots (`--fresh`).
    pub(super) fn take_screenshot(&mut self) {
        let Some(game) = self.game.as_mut().filter(|game| game.keeps_data) else {
            return;
        };
        // A controller can ask for this while the window is minimized and
        // doesn't show new frames.
        game.session.refresh_screen();
        let Some(pixels) = game.session.last_frame() else {
            return;
        };

        match screenshots::save(&game.session.name(), pixels) {
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
            PauseMenu::Main => {
                views::game::main_entries(game, &self.settings, self.fullscreen).len()
            }
            PauseMenu::Save | PauseMenu::Load => views::game::listed_slots(game, saving).len(),
            PauseMenu::Cheats => game.session.cheats().len(),
        };

        match action {
            Action::Up => game.move_menu_selection(-1, rows),
            Action::Down => game.move_menu_selection(1, rows),
            Action::B => self.escape(),
            Action::A | Action::Start => {
                let selection = game.menu_selection();
                let message = match page {
                    PauseMenu::Main => {
                        views::game::main_entries(game, &self.settings, self.fullscreen)
                            .into_iter()
                            .nth(selection)
                            .map(|entry| entry.message)
                    }
                    PauseMenu::Save | PauseMenu::Load => views::game::listed_slots(game, saving)
                        .get(selection)
                        .and_then(|info| views::game::slot_message(info, saving)),
                    PauseMenu::Cheats => {
                        game.session.cheats().get(selection).map(|cheat| {
                            GameMessage::CheatToggled(selection, !cheat.enabled).into()
                        })
                    }
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
