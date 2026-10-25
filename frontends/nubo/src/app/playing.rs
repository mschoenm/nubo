//! Starting, playing and stopping games: the game's top bar, the pause menu
//! with its save states and cheats, and opening ROMs.

use super::window_events::leave_fullscreen_for_game;
use super::{App, Message, scroll_in_proportion};
use crate::dialog::{Dialog, DialogKind};
use crate::game::{Game, PauseMenu, SgbBorderChoice};
use crate::rom_picker;
use crate::settings::Settings;
use crate::views;
use iced::Task;
use nubo_core::{CartridgeHeader, Emulator, Model, SgbBorder};
use nubo_frontend::controls::Action;
use nubo_frontend::library::LibrarySnapshot;
use nubo_frontend::model::ModelPreference;
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
    /// The file picker closed: with the game read, why it couldn't be, or
    /// nothing when the player canceled.
    RomPicked(Option<Result<Rom, String>>),
    /// A ROM from the library, the command line or a dropped file was read.
    RomRead(Result<Rom, String>),
    /// Starts the library game at this path.
    PlayGame(PathBuf),
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
    /// The player answered whether to continue the game that waits in
    /// `App::pending_start` on the model its automatic save was made on.
    ResumeAnswered(ResumeAnswer),
}

/// How the player answered `DialogKind::OtherModel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeAnswer {
    /// Continue from the automatic save, on the model it was made on, and
    /// keep the game on that model from now on, once it has continued.
    Continue,
    /// Start from the title screen on the model the game is set to start
    /// on; the automatic save is replaced once the game has run a minute or
    /// stops running (see [`AutosaveTimer`](crate::game::AutosaveTimer)).
    StartOver,
    /// Start nothing.
    Cancel,
}

/// How far [`App::launch`] got with a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Launch {
    /// The game didn't start; a toast says why.
    Failed,
    /// The game started from the title screen: there was no state to
    /// continue from, or it didn't load.
    Started,
    /// The game started and continued from where the player left off.
    Continued,
}

/// A game whose automatic save was made on another model than the one it is
/// set to start on, waiting while `DialogKind::OtherModel` asks which one it
/// starts on.
pub struct PendingStart {
    pub(super) rom: Rom,
    pub(super) header: CartridgeHeader,
    pub(super) state: SaveState,
    /// The dialog the question replaced, such as the box art question, put
    /// back once the player answers (even with Cancel) or another game
    /// starts instead.
    pub(super) replaced: Option<Dialog>,
}

/// Why [`App::restore_state`] couldn't load a state. Either way the game is
/// put back as it was, unless it doesn't start again on its own model.
enum RestoreError {
    /// The game didn't start on the state's model; the state may be fine.
    Start(String),
    /// The state itself didn't load.
    State(String),
}

/// What the Undo button of the newest toast puts back.
pub enum UndoPoint {
    /// The game as it was before a state was loaded or it restarted.
    Game { rom_path: PathBuf, state: SaveState },
    /// What a slot held before a save replaced it, with its picture's PNG
    /// file as it was stored, so Undo writes both back unchanged.
    Slot {
        rom_path: PathBuf,
        slot: Slot,
        state: SaveState,
        picture: Option<Vec<u8>>,
    },
    /// The settings before a reset, with what the library knew then (on
    /// builds that keep one) and the model `--model` picked for this run,
    /// which resetting the settings forgets.
    Settings {
        settings: Box<Settings>,
        library: Option<LibrarySnapshot>,
        model_override: Option<Model>,
    },
    /// What a library game's menu had picked for the game at `path` before
    /// the player picked another console (`None` to follow the settings).
    GameModel {
        path: PathBuf,
        preference: Option<ModelPreference>,
    },
}

impl From<GameMessage> for Message {
    fn from(message: GameMessage) -> Self {
        Message::Game(message)
    }
}

impl App {
    /// Handles a message about games: opening one, and controlling the one
    /// that runs and its pause menu.
    pub(super) fn update_game(&mut self, message: GameMessage) -> Task<Message> {
        match message {
            GameMessage::OpenRom => {
                return Task::perform(rom_picker::pick(), |rom| GameMessage::RomPicked(rom).into());
            }
            GameMessage::RomPicked(Some(Ok(rom))) => {
                #[cfg(target_arch = "wasm32")]
                if !self.fresh {
                    self.keep_game(&rom);
                }
                self.start_game(rom, true);
            }
            GameMessage::RomPicked(Some(Err(error))) => self.toasts.error(error),
            GameMessage::RomPicked(None) => {}
            GameMessage::RomRead(rom) => self.open(rom),
            GameMessage::PlayGame(path) => return read_rom(path),
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
            GameMessage::Undo => return self.undo(),
            GameMessage::ResumeAnswered(answer) => self.answer_resume(answer),
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
                // The browser holds the title screens back while a game runs.
                return Task::batch([
                    leave_fullscreen_for_game(self),
                    self.capture_title_screens(),
                ]);
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
    /// left it (never with `--fresh`). If that automatic save was made on
    /// another model than the game is set to start on now, a dialog asks
    /// first whether to continue on the save's model or start over on the
    /// new one (`DialogKind::OtherModel`), and the game waits in
    /// `App::pending_start`. The question takes the place of any dialog
    /// open then, which comes back once it is answered.
    ///
    /// While a reset deletes saves, the game waits until they are gone (see
    /// `App::deletions_running`), and neither a reset nor a console picked
    /// in a game's menu can be undone once a game starts.
    ///
    /// A game that starts, however it was started (a dropped file and
    /// Ctrl+O included), closes the settings, stops waiting for a key to
    /// bind and closes the library's and the settings' dialogs
    /// (`DialogKind::closes_when_game_starts`), so none stays over it.
    ///
    /// Restarting the same ROM stops it first, so its battery save and
    /// where the player left off are written before it loads again and
    /// reads them; another ROM replaces the running game only once it has
    /// loaded.
    pub(super) fn start_game(&mut self, rom: Rom, resume: bool) {
        if self.deletions_running > 0 {
            self.start_after_deleting = Some((rom, resume));
            return;
        }
        // Undoing a reset puts the library back as it was, which would
        // forget that this game was just played, and undoing a console pick
        // would overwrite the model the game now starts on (such as the
        // one its automatic save asks for).
        if matches!(
            self.undo,
            Some(UndoPoint::Settings { .. } | UndoPoint::GameModel { .. })
        ) {
            self.undo = None;
            self.toasts.forget_undo();
        }
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

        self.start_or_ask(rom, header, resume_state);
    }

    /// Starts `rom` from `resume_state`, where the player left off, or asks
    /// first when that was on another model than the one the game is set to
    /// start on (see `App::ask_which_model`). A scene doesn't ask, and
    /// continues on the save's model. A `--scale` window, which shows only
    /// the game's picture and so can't show the question, doesn't ask
    /// either: it continues on the save's model too, for this run only, so
    /// where the player left off is never replaced without asking
    /// (`--fresh` starts over on the model asked for).
    pub(super) fn start_or_ask(
        &mut self,
        rom: Rom,
        header: CartridgeHeader,
        resume_state: Option<SaveState>,
    ) {
        let model = self.model_for(&rom.path, &header);
        match resume_state {
            Some(state)
                if state.model != model
                    && self.screen_scale.is_none()
                    && !self.held_for_scene() =>
            {
                self.ask_which_model(rom, header, state, model);
            }
            Some(state) => {
                self.launch(rom, header, state.model, Some(state));
            }
            None => {
                self.launch(rom, header, model, None);
            }
        }
    }

    /// Asks whether to continue `rom` from `state`, its automatic save, on
    /// the model the save was made on or start over on `configured`, and
    /// keeps the game waiting in `App::pending_start` meanwhile. The
    /// question takes the place of the open dialog, kept to come back after
    /// the answer; a game already waiting is replaced, handing on the
    /// dialog its own question replaced.
    pub(super) fn ask_which_model(
        &mut self,
        rom: Rom,
        header: CartridgeHeader,
        state: SaveState,
        configured: Model,
    ) {
        let replaced = match self.dialog.take() {
            Some(Dialog {
                kind: DialogKind::OtherModel { .. },
                ..
            }) => self
                .pending_start
                .take()
                .and_then(|pending| pending.replaced),
            open => open,
        };
        self.dialog = Some(Dialog::new(DialogKind::OtherModel {
            saved: state.model,
            configured,
            remembers: self.model_override.is_none(),
        }));
        self.pending_start = Some(PendingStart {
            rom,
            header,
            state,
            replaced,
        });
    }

    /// Starts the game waiting in `App::pending_start` as the player
    /// answered, or drops it, and puts back the dialog the question
    /// replaced (which the game closes if it starts and the dialog belongs
    /// to the library or the settings). Continuing also sets the game to
    /// start on the save's model from now on (see
    /// `App::preference_starting_on`), so the question doesn't come back,
    /// unless a model was picked on the command line. That happens only once
    /// the game has continued: a game that didn't start, or whose save
    /// didn't load, keeps its model, so the question comes back at its next
    /// start.
    fn answer_resume(&mut self, answer: ResumeAnswer) {
        let Some(PendingStart {
            rom,
            header,
            state,
            replaced,
        }) = self.pending_start.take()
        else {
            return;
        };
        self.dialog = replaced;
        match answer {
            ResumeAnswer::Continue => {
                let path = rom.path.clone();
                let model = state.model;
                let preference = self.preference_starting_on(&header, model);
                if self.launch(rom, header, model, Some(state)) == Launch::Continued
                    && self.model_override.is_none()
                    && let Err(error) = self.library.set_model(&path, preference)
                {
                    self.toasts.error(error);
                }
            }
            ResumeAnswer::StartOver => {
                let model = self.model_for(&rom.path, &header);
                self.launch(rom, header, model, None);
            }
            ResumeAnswer::Cancel => {}
        }
    }

    /// Starts `rom` on `model`, loading `resume_state` (where the player
    /// left off, made on `model`) if there is one, and shows the game.
    /// Returns how far it got.
    fn launch(
        &mut self,
        rom: Rom,
        header: CartridgeHeader,
        model: Model,
        resume_state: Option<SaveState>,
    ) -> Launch {
        let path = rom.path.clone();

        let running = !self.held_for_scene();
        let launch = match self.start_session(rom, header, model, running) {
            Ok(mut game) => {
                let mut launch = Launch::Started;
                if let Some(state) = resume_state {
                    match game.session.load(state.data) {
                        Ok(()) => {
                            self.toasts.info("Continuing where you left off");
                            launch = Launch::Continued;
                        }
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
                self.capture = None;
                self.settings_focus = None;
                if let Some(pending) = self.pending_start.take()
                    && self
                        .dialog
                        .as_ref()
                        .is_some_and(|dialog| matches!(dialog.kind, DialogKind::OtherModel { .. }))
                {
                    self.dialog = pending.replaced;
                }
                if self
                    .dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.kind.closes_when_game_starts())
                {
                    self.dialog = None;
                }
                self.search.clear();
                self.library_selection = None;
                launch
            }
            Err(error) => {
                self.toasts.error(format!(
                    "Couldn't start {}: {error}",
                    path.file_stem().unwrap_or_default().to_string_lossy()
                ));
                Launch::Failed
            }
        };

        self.show_notices();
        launch
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
                // A scene's game keeps no battery saves, and so runs its
                // cartridge clock on emulated time, as headless runs do,
                // so its frames come out the same every time.
                keep_saves: !self.fresh && !self.held_for_scene(),
                fast_forward: self.speed,
            },
            &self.notices,
            running,
            &self.settings.shared,
        )?;
        self.apply_settings(&mut session);
        if !self.fresh
            && let Err(error) = session.load_cheats()
        {
            self.toasts.error(error);
        }
        session.set_controller_states(self.controller_states_for_game());
        Ok(Game::new(session, !self.fresh))
    }

    /// Hands the controls, the look of the picture and the Super Game Boy
    /// choices in the settings to `session`, a game that starts or whose
    /// settings were just replaced.
    fn apply_settings(&self, session: &mut GameSession) {
        let shared = &self.settings.shared;
        session.set_controls(&shared.controls);
        session.set_screen_look(self.screen_look());
        session.set_sgb_border(shared.sgb_border);
        if session.model().is_sgb() {
            // The Super Game Boy menu's choices are the player's, which save
            // states don't keep, so they come from the settings.
            session.set_sgb_player_palette(self.sgb_player_palette());
            session.choose_sgb_border(if shared.sgb_nubo_border {
                SgbBorder::Nubo
            } else {
                SgbBorder::Game
            });
        }
    }

    /// Hands the settings to the running game, after they were replaced
    /// (see [`App::apply_settings`]).
    pub(super) fn apply_settings_to_game(&mut self) {
        if let Some(mut game) = self.game.take() {
            self.apply_settings(&mut game.session);
            self.game = Some(game);
        }
    }

    /// Stops the running game. It saves where the player left off (if the
    /// settings say so) and keeps the last frame as the game's thumbnail,
    /// unless it keeps nothing (`--fresh`).
    pub(super) fn stop_game(&mut self) {
        self.save_where_left_off();
        if self.game.take().is_none() {
            return;
        }
        self.undo = None;
        self.toasts.forget_undo();
    }

    /// Saves where the player left off in the running game, unless the
    /// settings don't continue games, and its picture for the library's
    /// card, unless it keeps no data (`--fresh`). A pause menu page listing
    /// the slots is listed again, so it shows the new save.
    pub(super) fn save_where_left_off(&mut self) {
        let Some(game) = self.game.as_mut().filter(|game| game.keeps_data) else {
            return;
        };

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

        if let page @ Some(PauseMenu::Save | PauseMenu::Load) = game.menu() {
            game.show_menu(page);
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
        let picture = previous
            .as_ref()
            .and_then(|_| save_states::picture_png(&rom_path, slot));

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
    /// was before a load or a restart, a slot as it was before a save, the
    /// settings as they were before a reset, or the console a library game
    /// was set to start on before the player picked another.
    fn undo(&mut self) -> Task<Message> {
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
                        Err(RestoreError::Start(error) | RestoreError::State(error)) => {
                            self.toasts.error(format!("Couldn't undo: {error}"));
                        }
                    }
                }
            }
            Some(UndoPoint::Slot {
                rom_path,
                slot,
                state,
                picture,
            }) => match save_states::save_with_png(&rom_path, slot, &state, picture.as_deref()) {
                Ok(()) => self.toasts.info(format!("Put the earlier {slot} back")),
                Err(error) => self.toasts.error(format!("Couldn't undo: {error}")),
            },
            Some(UndoPoint::Settings {
                settings,
                library,
                model_override,
            }) => {
                return self.restore_settings(*settings, library, model_override);
            }
            Some(UndoPoint::GameModel { path, preference }) => {
                match self.library.set_model(&path, preference) {
                    Ok(()) => self.toasts.info("Undone"),
                    Err(error) => self.toasts.error(format!("Couldn't undo: {error}")),
                }
            }
            None => {}
        }
        Task::none()
    }

    /// Loads `state` into the running game, restarting it first on the
    /// state's model if that differs, stopping the running one before the
    /// new one starts so its battery save is written before the new one
    /// reads it. The game resumes, unless the settings are open.
    ///
    /// If the game doesn't start on the state's model, or the state doesn't
    /// load into it, the game starts again on its own model from a snapshot
    /// taken before it stopped, on the pause menu page it was on, so the
    /// player keeps it as it was.
    fn restore_state(&mut self, state: SaveState) -> Result<(), RestoreError> {
        let Some(game) = self.game.as_mut() else {
            return Ok(());
        };

        if state.model == game.session.model() {
            game.session.load(state.data).map_err(RestoreError::State)?;
            game.show_menu(None);
            return Ok(());
        }

        let rom = game.session.rom().clone();
        let header = game.session.header().clone();
        let old_model = game.session.model();
        let menu = game.menu();
        let snapshot = game.session.snapshot();
        let running = self.settings_tab.is_none();
        self.game = None;

        let error = match self.start_session(rom.clone(), header.clone(), state.model, running) {
            Ok(mut game) => match game.session.load(state.data) {
                Ok(()) => {
                    game.show_menu(None);
                    self.game = Some(game);
                    return Ok(());
                }
                Err(error) => RestoreError::State(error),
            },
            Err(error) => RestoreError::Start(error),
        };

        match self.start_session(rom, header, old_model, running) {
            Ok(mut game) => {
                if let Err(error) = snapshot.and_then(|snapshot| game.session.load(snapshot.data)) {
                    self.toasts
                        .error(format!("Couldn't put the game back as it was: {error}"));
                }
                game.show_menu(menu);
                self.game = Some(game);
            }
            Err(start_error) => self.toasts.error(format!(
                "Couldn't start the game again on {old_model}: {start_error}"
            )),
        }
        Err(error)
    }

    /// Loads the state in `slot` and resumes the game (see
    /// [`App::restore_state`]), offering to undo it. Only a state that
    /// can't be read or loaded is moved aside; when the game doesn't start
    /// on the state's model the slot stays as it is.
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
            Err(RestoreError::Start(error)) => {
                self.toasts.error(format!("Couldn't load {slot}: {error}"));
            }
            Err(RestoreError::State(error)) => {
                self.keep_unloadable_state(
                    &rom_path,
                    slot,
                    format!("Couldn't load {slot}: {error}"),
                );
            }
        }
    }

    /// Reports `message` about the state in `slot` that couldn't be loaded,
    /// and moves the state aside so the next save to the slot (as the game
    /// runs and when it stops, for where the player left off) can't
    /// overwrite it.
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
                "Screenshot {} as {}",
                if screenshots::DOWNLOADED {
                    "downloaded"
                } else {
                    "saved"
                },
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
    /// or Start picks one, and B steps back. Up and down wrap around the
    /// ends, unless the key is `held` (see `crate::selection`).
    pub(super) fn navigate_pause_menu(&mut self, action: Action, held: bool) -> Task<Message> {
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
                let step = if action == Action::Up { -1 } else { 1 };
                game.move_menu_selection(step, rows, !held);
                return scroll_in_proportion(
                    views::game::pause_scroll_id(),
                    game.menu_selection(),
                    rows,
                );
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

/// Reads the ROM at `path` in the background: its file on the desktop, or
/// the file the library keeps in the browser.
pub(super) fn read_rom(path: PathBuf) -> Task<Message> {
    Task::perform(rom::load(path), |rom| GameMessage::RomRead(rom).into())
}
