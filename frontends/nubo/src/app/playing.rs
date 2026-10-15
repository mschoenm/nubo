//! Starting, playing and stopping games: the game's top bar, the pause menu
//! with its save states and cheats, and opening ROMs.

use super::window_events::leave_fullscreen_for_game;
use super::{App, Message};
use crate::game::{Game, PauseMenu, SgbBorderChoice};
use crate::rom_picker;
use crate::views;
use iced::Task;
use iced::widget::{self, scrollable::RelativeOffset};
use nubo_core::{CartridgeHeader, Emulator, Model, SgbBorder};
use nubo_frontend::cheats;
use nubo_frontend::controls::Action;
use nubo_frontend::library::LibraryEntry;
use nubo_frontend::model::Console;
use nubo_frontend::picture::Picture;
use nubo_frontend::rom::{self, Rom};
use nubo_frontend::save_states::{self, SaveState, Slot};
use nubo_frontend::screenshots;
use nubo_frontend::session::{GameSession, StartOptions};
use nubo_frontend::sgb_menu::{ColorChannel, SgbColors};
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
    /// Starts the last game played again, in the browser (see
    /// `nubo_frontend::rom::remember`).
    PlayRemembered,
    Reset,
    TogglePause,
    /// Opens the pause menu on its main page, from a right click on the
    /// game. Does nothing while the menu or the settings are open.
    OpenPauseMenu,
    /// Closes the pause menu and resumes the game, from a click outside it.
    ClosePauseMenu,
    /// A pause menu row that leaves the menu behind (going fullscreen, the
    /// settings) was picked: closes the menu, then handles the row's message.
    PickedFromMenu(Box<Message>),
    /// Stops the game and goes back to the library, in a window again if
    /// the game went fullscreen.
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
    /// Colors a Super Game Boy game with these colors.
    SgbColorsPicked(SgbColors),
    /// A channel of one of the player's own Super Game Boy colors (shade 0
    /// to 3, lightest first) was dragged to this value.
    SgbOwnColorChanged(usize, ColorChannel, u8),
    /// The player let go of a channel's slider.
    SgbOwnColorReleased,
    /// A channel of one of the player's own colors went down or up by this
    /// much with the D-pad.
    SgbOwnColorStepped(usize, ColorChannel, i8),
    /// Shows or hides the editor for the player's own Super Game Boy
    /// colors.
    ToggleSgbEditor,
    /// Shows this border around a Super Game Boy game.
    SgbBorderPicked(SgbBorderChoice),
    /// The pictures of the save state slots a pause menu page lists were
    /// read, for this opening of the page (see
    /// `GameSession::thumbnails_to_load`).
    ThumbnailsLoaded(u64, Vec<(Slot, Option<Picture>)>),
    /// Undoes the last load, restart or save that offered it (see
    /// `UndoPoint`).
    Undo,
}

/// What the Undo button of the newest toast puts back.
pub enum UndoPoint {
    /// The game as it was before a state was loaded or it restarted.
    Game { rom_path: PathBuf, state: SaveState },
    /// What a slot held before a save replaced it, with its picture.
    Slot {
        rom_path: PathBuf,
        slot: Slot,
        state: SaveState,
        picture: Option<Vec<u8>>,
    },
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
            GameMessage::PlayRemembered => {
                if let Some(rom) = self.remembered.clone() {
                    self.start_game(rom, true);
                }
            }
            GameMessage::Reset => {
                if let Some(game) = &mut self.game {
                    let rom = game.session.rom().clone();
                    let before = game.keeps_data.then(|| game.session.snapshot());
                    self.start_game(rom.clone(), false);
                    let restarted = self
                        .game
                        .as_ref()
                        .is_some_and(|game| game.session.rom().path == rom.path);
                    if let Some(Ok(state)) = before
                        && restarted
                    {
                        self.undo = Some(UndoPoint::Game {
                            rom_path: rom.path,
                            state,
                        });
                        self.toasts.info_with_undo("Restarted");
                    }
                }
            }
            GameMessage::Undo => self.undo(),
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
            GameMessage::OpenPauseMenu => {
                if let Some(game) = self.game.as_mut().filter(|game| game.menu().is_none())
                    && self.settings_tab.is_none()
                {
                    game.show_menu(Some(PauseMenu::Main));
                }
            }
            GameMessage::ClosePauseMenu => {
                if let Some(game) = &mut self.game {
                    game.show_menu(None);
                }
            }
            GameMessage::PickedFromMenu(message) => {
                if let Some(game) = &mut self.game {
                    game.show_menu(None);
                }
                return self.update(*message);
            }
            GameMessage::CloseGame => {
                self.stop_game();
                return leave_fullscreen_for_game(self);
            }
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
            GameMessage::SgbColorsPicked(colors) => {
                self.change_settings(|settings| settings.shared.sgb_colors = colors);
                let palette = self.sgb_player_palette();
                if let Some(game) = &mut self.game {
                    game.session.set_sgb_player_palette(palette);
                    if colors == SgbColors::Own {
                        game.sgb_editor_open = true;
                    }
                }
            }
            GameMessage::ToggleSgbEditor => {
                if let Some(game) = &mut self.game {
                    game.sgb_editor_open = !game.sgb_editor_open;
                }
            }
            GameMessage::SgbOwnColorChanged(shade, channel, value) => {
                self.set_sgb_own_color(shade, channel, value);
            }
            GameMessage::SgbOwnColorReleased => self.change_settings(|_| {}),
            GameMessage::SgbOwnColorStepped(shade, channel, step) => {
                if let Some(&color) = self.settings.shared.sgb_own_palette.get(shade) {
                    let value = channel
                        .of(color)
                        .saturating_add_signed(step)
                        .min(ColorChannel::MAX);
                    if value != channel.of(color) {
                        self.set_sgb_own_color(shade, channel, value);
                        self.sgb_colors_unsaved = true;
                    }
                }
            }
            GameMessage::SgbBorderPicked(choice) => {
                let shown = choice != SgbBorderChoice::None;
                self.change_settings(|settings| {
                    settings.shared.sgb_border = shown;
                    if shown {
                        settings.shared.sgb_nubo_border = choice == SgbBorderChoice::Nubo;
                    }
                });
                if let Some(game) = &mut self.game {
                    game.session.set_sgb_border(shown);
                    match choice {
                        SgbBorderChoice::Game => game.session.choose_sgb_border(SgbBorder::Game),
                        SgbBorderChoice::Nubo => game.session.choose_sgb_border(SgbBorder::Nubo),
                        SgbBorderChoice::None => {}
                    }
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
    /// left it, on the model it ran on then (never with `--fresh`), unless
    /// the player has since picked another model for the game: then it
    /// starts on that one, and Undo goes back to where they left off.
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

        let picked = self.library.entry(&rom.path).and_then(LibraryEntry::model);
        let (resume_state, set_aside) = match resume_state {
            Some(state) if picked.is_some_and(|model| model != state.model) => (None, Some(state)),
            state => (state, None),
        };

        let model = resume_state
            .as_ref()
            .map_or_else(|| self.model_for(&rom.path, &header), |state| state.model);
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

                if rom::REMEMBERS_LAST_GAME && !self.fresh {
                    let rom = game.session.rom().clone();
                    if let Err(error) = rom::remember(&rom) {
                        self.toasts.error(error);
                    }
                    self.remembered = Some(rom);
                }
                self.game = Some(game);
                self.settings_tab = None;
                self.search.clear();
                self.library_selection = None;

                if let Some(state) = set_aside {
                    self.undo = Some(UndoPoint::Game {
                        rom_path: path,
                        state,
                    });
                    self.toasts.info_with_undo(format!(
                        "Started on the {}, not where you left off",
                        Console::of(model).label()
                    ));
                }
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
        session.set_screen_look(self.screen_look());
        session.set_sgb_border(self.settings.shared.sgb_border);
        if model.is_sgb() {
            // The Super Game Boy menu's choices are the player's, which save
            // states don't keep, so they come from the settings.
            let shared = &self.settings.shared;
            session.set_sgb_player_palette(self.sgb_player_palette());
            session.choose_sgb_border(if shared.sgb_nubo_border {
                SgbBorder::Nubo
            } else {
                SgbBorder::Game
            });
        }
        if !self.fresh {
            match cheats::load(&session.rom().path) {
                Ok(cheats) => session.set_cheats(cheats),
                Err(error) => self.toasts.error(error),
            }
        }
        session.set_controller_states(self.controller_states_for_game());
        Ok(Game::new(session, !self.fresh))
    }

    /// Stops the running game. It saves where the player left off (if the
    /// settings say so) and keeps the last frame as the game's thumbnail,
    /// unless it keeps nothing (`--fresh`).
    pub(super) fn stop_game(&mut self) {
        let Some(mut game) = self.game.take() else {
            return;
        };
        self.undo = None;
        self.toasts.forget_undo();
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
            && let Err(error) = self.library.set_thumbnail(&game.session.rom().path, pixels)
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

        // What the slot held, so the save can be undone.
        let rom_path = game.session.rom().path.clone();
        let previous = save_states::load(&rom_path, slot).ok().flatten();
        let picture = previous.as_ref().and_then(|_| {
            save_states::thumbnails(&rom_path, &[slot])
                .into_iter()
                .find_map(|(_, picture)| picture)
                .map(|picture| picture.pixels().to_vec())
        });

        match game.session.save(slot) {
            Ok(()) => {
                game.show_menu(None);
                match previous {
                    Some(state) => {
                        self.undo = Some(UndoPoint::Slot {
                            rom_path,
                            slot,
                            state,
                            picture,
                        });
                        self.toasts.info_with_undo(format!("Saved to {slot}"));
                    }
                    None => self.toasts.info(format!("Saved to {slot}")),
                }
            }
            Err(error) => self.toasts.error(error),
        }
    }

    /// Puts back what the newest toast's Undo button offers: the game as it
    /// was before a load or a restart, or a slot as it was before a save.
    fn undo(&mut self) {
        self.toasts.forget_undo();
        match self.undo.take() {
            Some(UndoPoint::Game { rom_path, state }) => {
                if self
                    .game
                    .as_ref()
                    .is_some_and(|game| game.session.rom().path == rom_path)
                {
                    match self.restore_state(state) {
                        Ok(()) => self.toasts.info("Undone"),
                        Err(error) => self.toasts.error(format!("Couldn't undo: {error}")),
                    }
                }
            }
            Some(UndoPoint::Slot {
                rom_path,
                slot,
                state,
                picture,
            }) => match save_states::save(&rom_path, slot, &state, picture.as_deref()) {
                Ok(()) => self.toasts.info(format!("Put the earlier {slot} back")),
                Err(error) => self.toasts.error(format!("Couldn't undo: {error}")),
            },
            None => {}
        }
    }

    /// Loads `state` into the running game, restarting it first on the
    /// state's model if that differs, stopping the running one before the
    /// new one starts so its battery save is written before the new one
    /// reads it. The game resumes, unless the settings are open.
    fn restore_state(&mut self, state: SaveState) -> Result<(), String> {
        let Some(game) = self.game.as_ref() else {
            return Ok(());
        };

        if state.model != game.session.model() {
            let (rom, header) = (game.session.rom().clone(), game.session.header().clone());
            self.game = None;
            let running = self.settings_tab.is_none();
            self.game = Some(self.start_session(rom, header, state.model, running)?);
        }

        let Some(game) = &mut self.game else {
            return Ok(());
        };
        game.session.load(state.data)?;
        game.show_menu(None);
        Ok(())
    }

    /// Loads the state in `slot` and resumes the game (see
    /// [`App::restore_state`]), offering to undo it.
    pub(super) fn load_state(&mut self, slot: Slot) {
        let Some(game) = self.game.as_mut().filter(|game| game.keeps_data) else {
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
        let before = game.session.snapshot();

        match self.restore_state(state) {
            Ok(()) => {
                if let Ok(state) = before {
                    self.undo = Some(UndoPoint::Game { rom_path, state });
                    self.toasts.info_with_undo(format!("Loaded {slot}"));
                } else {
                    self.toasts.info(format!("Loaded {slot}"));
                }
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
        let Some(picture) = game.session.screen() else {
            return;
        };

        match screenshots::save(&game.session.name(), picture) {
            Ok(path) => self.toasts.info(format!(
                "Screenshot saved as {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            )),
            Err(error) => self.toasts.error(error),
        }
    }

    /// Sets a channel of one of the player's own Super Game Boy colors and
    /// picks those colors (neither saved yet), and colors the game with them.
    fn set_sgb_own_color(&mut self, shade: usize, channel: ColorChannel, value: u8) {
        let Some(color) = self.settings.shared.sgb_own_palette.get_mut(shade) else {
            return;
        };
        *color = channel.with(*color, value);
        self.settings.shared.sgb_colors = SgbColors::Own;
        let palette = self.sgb_player_palette();
        if let Some(game) = &mut self.game {
            game.session.set_sgb_player_palette(palette);
        }
    }

    /// The colors the player picked in the Super Game Boy menu, or none for
    /// the game's own: always the game's with `--raw-screen`, which shows
    /// the Game Boy screen as the emulator draws it.
    fn sgb_player_palette(&self) -> Option<[u16; 4]> {
        let shared = &self.settings.shared;
        if self.raw_screen {
            None
        } else {
            shared.sgb_colors.palette(shared.sgb_own_palette)
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
            PauseMenu::SuperGameBoy => views::game::sgb::rows(game).len(),
        };

        match action {
            Action::Up | Action::Down => {
                game.move_menu_selection(if action == Action::Up { -1 } else { 1 }, rows);
                if page == PauseMenu::SuperGameBoy {
                    return scroll_pause_menu(game.menu_selection(), rows);
                }
            }
            Action::Left | Action::Right if page == PauseMenu::SuperGameBoy => {
                let step = if action == Action::Left { -1 } else { 1 };
                let message = views::game::sgb::rows(game)
                    .get(game.menu_selection())
                    .and_then(|&row| views::game::sgb::adjust(row, step));
                if let Some(message) = message {
                    return self.update(message);
                }
            }
            Action::B => return self.escape(),
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
                    PauseMenu::SuperGameBoy => views::game::sgb::rows(game)
                        .get(selection)
                        .and_then(|&row| views::game::sgb::pick(row)),
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

/// Scrolls the pause menu so row `index` of `count` is roughly in view, as
/// the settings window does (rows are spread fairly evenly over the page).
fn scroll_pause_menu(index: usize, count: usize) -> Task<Message> {
    let fraction = if count > 1 {
        index as f32 / (count - 1) as f32
    } else {
        0.0
    };
    widget::operation::snap_to(
        views::game::pause_scroll_id(),
        RelativeOffset {
            x: None,
            y: Some(fraction),
        },
    )
}

/// Reads the ROM at `path` in the background.
pub(super) fn read_rom(path: PathBuf) -> Task<Message> {
    Task::perform(async move { Rom::read(&path) }, |rom| {
        GameMessage::RomRead(rom).into()
    })
}
