//! The application window. It shows the game library until a game starts
//! and the game screen while one runs; the settings, the dialogs, the drop
//! target and notifications are drawn on top.
//!
//! The messages come in groups, each handled in a file of its own: the
//! running game and its pause menu (`app/playing.rs`), the library
//! (`app/library_screen.rs`), the settings window (`app/settings_window.rs`),
//! the dialogs (`app/dialogs.rs`), the window itself
//! (`app/window_events.rs`), keys and controller buttons
//! (`app/player_input.rs`), and scenes for the documentation's screenshots
//! (`app/scenes.rs`, desktop only).

use crate::command_line::Options;
use crate::dialog::Dialog;
use crate::game::{Game, PauseMenu};
use crate::input::{self, KeyInput};
use crate::menu_presses::MenuPresses;
use crate::selection;
use crate::settings::Settings;
use crate::subscriptions;
use crate::ui::theme;
use crate::ui::toasts::Toasts;
use crate::views;
use crate::views::settings::{SettingsFocus, SettingsTab};
use iced::theme::Mode;
use iced::widget::Stack;
use iced::{Element, Subscription, Task, Theme, event, keyboard, system, window};
use nubo_core::{CartridgeHeader, ControllerState, Model};
use nubo_frontend::background;
use nubo_frontend::controls::{self, Action, Device};
use nubo_frontend::gamepads::{ControllerEvent, Controllers};
use nubo_frontend::library::box_art::GameList;
use nubo_frontend::library::{Library, LibraryEntry};
use nubo_frontend::model::ModelPreference;
use nubo_frontend::notices::Notices;
use nubo_frontend::rom::Rom;
use nubo_frontend::save_states;
use nubo_frontend::settings::{FastForwardSpeed, ScreenLook};
use std::path::Path;
use std::sync::Arc;

pub use dialogs::DialogMessage;
pub use library_screen::LibraryMessage;
pub use nubo_frontend::picture::{SCREEN_HEIGHT, SCREEN_WIDTH};
pub use playing::GameMessage;
#[cfg(not(target_arch = "wasm32"))]
pub use scenes::{SceneMessage, prepare as prepare_capture};
pub use settings_window::SettingsMessage;
pub use window_events::{WindowMessage, screen_only_size};

/// How often the desktop build checks for toasts to take down and for the
/// fullscreen bar to hide, while there are any.
#[cfg(not(target_arch = "wasm32"))]
const QUICK_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);
/// How long picking a key or button waits for one before it gives up, so
/// a controller can't get stuck waiting: every one of its buttons can be
/// picked, so none can cancel.
const CAPTURE_TIMEOUT: iced::time::Duration = iced::time::Duration::from_secs(5);
/// How often the desktop build otherwise checks for problems the running
/// game reported, which would wait for the next new frame on a still screen.
#[cfg(not(target_arch = "wasm32"))]
const NOTICE_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

/// The Iced application state.
pub struct App {
    settings: Settings,
    /// The operating system's light or dark mode, used by the "System"
    /// appearance.
    system_mode: Mode,
    /// A model picked on the command line (`--model`), which every game of
    /// this run starts on, before the game's own pick and the settings'
    /// (see `App::model_for`), until the player picks one in the settings or
    /// resets them.
    model_override: Option<Model>,
    /// With `--scale`: the window shows only the game screen, this many
    /// times its size, and nothing else can be brought up (see
    /// `command_line`).
    screen_scale: Option<u32>,
    /// With `--fresh`: the app reads and writes nothing kept between runs,
    /// like a new install (see `command_line`).
    fresh: bool,
    /// With `--raw-screen`: games show the Game Boy screen as the emulator
    /// draws it, whatever palette and screen effects the settings pick (see
    /// `command_line`).
    raw_screen: bool,
    /// Whether saving the settings can't lose the player's: the settings
    /// file was read, there was none, or the one that couldn't be read was
    /// kept aside. Otherwise the app saves nothing on its own, such as the
    /// window's size, so it doesn't write the defaults over the player's
    /// settings.
    settings_read: bool,
    /// Whether the D-pad changed the player's own Super Game Boy colors
    /// since the settings were last saved: they are saved once the player
    /// leaves the editor, not on every step of a held key.
    sgb_colors_unsaved: bool,
    /// With `--speed`: games start fast forwarding at this speed.
    speed: Option<FastForwardSpeed>,
    library: Library,
    /// The text in the library's search field.
    search: String,
    game: Option<Game>,
    /// The settings tab on show, or `None` while the settings are closed.
    settings_tab: Option<SettingsTab>,
    /// Whether a file is being dragged over the window.
    file_hovered: bool,
    /// Whether the window fills the screen. The game's top bar then hides
    /// until the mouse moves.
    fullscreen: bool,
    /// Whether the window went fullscreen while a game was open, so it goes
    /// back to a window when the game closes.
    fullscreen_for_game: bool,
    /// Whether the window is the one in front, which the game only runs in
    /// if the settings say to pause in the background.
    focused: bool,
    /// The window's size, as it last had one.
    window_width: f32,
    window_height: f32,
    /// Whether the window has no size, which is how Windows reports a
    /// minimized window. New frames aren't shown then (the game keeps
    /// running and playing); Iced 0.14 doesn't tell when a window is
    /// minimized or hidden on other systems (see `App::subscription`).
    minimized: bool,
    /// The library card the D-pad is on, once the player has used it.
    library_selection: Option<usize>,
    /// How far the library page is scrolled, and how high its visible part
    /// is, to keep the selected card in view.
    library_scroll: (f32, f32),
    toasts: Toasts,
    /// Problems reported by the running game, waiting to become toasts.
    notices: Notices,
    /// The names of the connected controllers.
    controllers: Vec<String>,
    /// What the controllers did last, all combined, to tell which buttons
    /// were just pressed or released.
    controller_state: ControllerState,
    /// What each controller did last, which the game gets.
    controller_states: Controllers,
    /// The controller buttons pressed in the menus, which the game doesn't
    /// see until they are released.
    menu_presses: MenuPresses,
    /// The device shown on the Controls tab of the settings.
    controls_device: Device,
    /// The action waiting for the player to press a key or button for it.
    capture: Option<(Device, Action)>,
    /// When the wait for `capture` started; it ends after
    /// [`CAPTURE_TIMEOUT`].
    capture_started: iced::time::Instant,
    /// Where the D-pad is in the settings window, once the player has used
    /// it there.
    settings_focus: Option<SettingsFocus>,
    /// The No-Intro game lists that box art is looked up with, once read.
    game_list: Option<Arc<GameList>>,
    /// Whether the game lists are being read, so they are read only once.
    loading_game_list: bool,
    /// Why box art couldn't be downloaded this run, shown in the settings'
    /// Library tab. Nubo stops trying until the player asks it to or the
    /// app starts again, and never shows a toast for it.
    box_art_problem: Option<String>,
    /// The dialog over the screen, if one is open.
    dialog: Option<Dialog>,
    /// Whether the game whose menu is open has save states, so the menu
    /// offers to delete them. Worked out when the menu opens and when save
    /// states are deleted (`App::refresh_game_menu_save_states`) rather than
    /// on every redraw, since it looks for each slot's file.
    game_menu_save_states: bool,
    /// What the newest toast's Undo button puts back, if it has one.
    undo: Option<playing::UndoPoint>,
    /// How many resets are deleting saves in the background. No game starts
    /// until they are all done, since it could read a save about to go and
    /// write it back.
    deletions_running: u32,
    /// The game to start once the saves are deleted, and whether it
    /// continues where the player left off, if one was asked for meanwhile.
    start_after_deleting: Option<(Rom, bool)>,
    /// The game waiting for the player to say which model it continues on,
    /// while `DialogKind::OtherModel` asks.
    pending_start: Option<playing::PendingStart>,
    /// Whether the browser's storage was opened (see
    /// `nubo_frontend::storage::indexed_db`), which the library is read from;
    /// always on the desktop.
    storage_open: bool,
    /// A newer version than this one, once the app found one is out.
    newer_version: Option<String>,
    /// With `--record-scenes`: the folder Ctrl+F12 saves scenes in (see
    /// `scenes`).
    #[cfg(not(target_arch = "wasm32"))]
    scene_recorder: Option<std::path::PathBuf>,
    /// The scene Ctrl+F12 is saving, until the picture of the window is
    /// taken.
    #[cfg(not(target_arch = "wasm32"))]
    recording: Option<scenes::PendingRecording>,
    /// The scene the `capture` command shows.
    #[cfg(not(target_arch = "wasm32"))]
    scene_run: Option<scenes::SceneRun>,
}

/// Everything the user or the window can ask the application to do.
#[derive(Debug, Clone)]
pub enum Message {
    Game(GameMessage),
    Library(LibraryMessage),
    Settings(SettingsMessage),
    Window(WindowMessage),
    Dialog(DialogMessage),
    Key(KeyInput),
    Controller(ControllerEvent),
    /// Steps back: dismisses a dialog, closes the settings, goes up a page
    /// in the pause menu, or pauses and resumes the game.
    Escape,
    /// Time to show the newest frame and check what changes with time alone,
    /// such as toasts expiring: the emulator finished a new frame, a timer
    /// check is due (desktop, see `App::check_interval`), or the window is
    /// about to draw (browser, which also emulates the frames due by then,
    /// see `nubo_frontend::session`).
    Tick,
    /// Takes down the toast at this place in the list.
    DismissToast(usize),
    /// Opens this web page in the player's browser.
    OpenLink(&'static str),
    /// The newest version was looked up: its number, or why not.
    LatestVersion(Result<String, String>),
    /// What a scene being shown or recorded hears back (see `scenes`).
    #[cfg(not(target_arch = "wasm32"))]
    Scene(SceneMessage),
    /// The browser's storage was opened, or why it couldn't be.
    #[cfg(target_arch = "wasm32")]
    StorageOpened(Result<(), String>),
    /// The last game played that older versions kept in localStorage is in
    /// the browser's storage for good now, or couldn't be kept there.
    #[cfg(target_arch = "wasm32")]
    LastGameMoved(Result<(), String>),
}

impl App {
    /// Creates the application with the saved settings and library (or the
    /// defaults and an empty library with `--fresh`), starting the ROM given
    /// on the command line if there is one, as the other `options` say.
    pub fn new(rom: Option<Result<Rom, String>>, options: &Options) -> (Self, Task<Message>) {
        let (settings, settings_error, settings_read) = if options.fresh {
            (Settings::default(), None, true)
        } else {
            let loaded = Settings::load();
            (loaded.settings, loaded.problem, loaded.saving_is_safe)
        };
        // The browser reads its library once its storage is open (see
        // `Message::StorageOpened`).
        let storage_open = !cfg!(target_arch = "wasm32");
        let (library, library_error) = if options.fresh || !storage_open {
            (Library::default(), None)
        } else {
            Library::load()
        };
        let toasts = if options.fresh || options.scale.is_some() {
            Toasts::printed()
        } else {
            Toasts::default()
        };

        let mut app = App {
            settings,
            system_mode: Mode::None,
            model_override: options.model,
            screen_scale: options.scale,
            fresh: options.fresh,
            raw_screen: options.raw_screen,
            settings_read,
            sgb_colors_unsaved: false,
            speed: options.speed,
            library,
            search: String::new(),
            game: None,
            settings_tab: None,
            file_hovered: false,
            fullscreen: false,
            fullscreen_for_game: false,
            focused: true,
            window_width: 0.0,
            window_height: 0.0,
            minimized: false,
            library_selection: None,
            library_scroll: (0.0, 0.0),
            toasts,
            notices: Notices::default(),
            controllers: Vec::new(),
            controller_state: ControllerState::new(),
            controller_states: Controllers::default(),
            menu_presses: MenuPresses::default(),
            controls_device: Device::Keyboard,
            capture: None,
            capture_started: iced::time::Instant::now(),
            settings_focus: None,
            game_list: None,
            loading_game_list: false,
            box_art_problem: None,
            dialog: None,
            game_menu_save_states: false,
            undo: None,
            deletions_running: 0,
            start_after_deleting: None,
            pending_start: None,
            newer_version: None,
            #[cfg(not(target_arch = "wasm32"))]
            scene_recorder: options.record_scenes.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            recording: None,
            #[cfg(not(target_arch = "wasm32"))]
            scene_run: None,
            storage_open,
        };

        for error in [settings_error, library_error].into_iter().flatten() {
            app.toasts.error(error);
        }
        if app.settings_read && app.settings.shared.controls_revision < controls::REVISION {
            app.change_settings(|settings| {
                controls::add_new_actions(&mut settings.shared.controls);
                settings.shared.controls_revision = controls::REVISION;
            });
        }
        if app.settings_read
            && app
                .settings
                .shared
                .controls
                .controller
                .input(Action::PauseMenu)
                .is_none()
        {
            app.change_settings(|settings| {
                settings.shared.controls.give_pause_menu_a_button();
            });
        }

        // Decided before the command line's game starts, which counts as
        // played, so the very first start with a game doesn't ask over it.
        let played_before = app.played_before();
        if let Some(rom) = rom {
            app.open(rom);
        }
        app.ask_about_box_art();
        app.ask_about_updates(played_before);

        let updates = app.look_for_updates();
        #[cfg(not(target_arch = "wasm32"))]
        let library = app.library_read();
        #[cfg(target_arch = "wasm32")]
        let library = Task::perform(
            nubo_frontend::storage::indexed_db::open(),
            Message::StorageOpened,
        );
        (
            app,
            Task::batch([
                system::theme().map(|mode| WindowMessage::SystemThemeChanged(mode).into()),
                library,
                updates,
            ]),
        )
    }

    /// Brings the library just read up to date: looks whether its games'
    /// files are still there, scans its folders and reads its pictures.
    fn library_read(&mut self) -> Task<Message> {
        Task::batch([self.check_files(), self.rescan(), self.load_pictures(false)])
    }

    /// Reads the library from the browser's storage, now open, and moves
    /// the last game played that older versions kept into it. Storage that
    /// couldn't be opened (as in some private windows) leaves the library
    /// empty; games still play, and what they can't keep says so.
    #[cfg(target_arch = "wasm32")]
    fn storage_opened(&mut self, result: Result<(), String>) -> Task<Message> {
        self.storage_open = true;
        if let Err(error) = result {
            self.toasts.error(error);
            return Task::none();
        }
        let (library, error) = Library::load();
        self.library = library;
        if let Some(error) = error {
            self.toasts.error(error);
        }

        let moved = nubo_frontend::rom::remembered()
            .map_or_else(Task::none, |rom| self.move_last_game(&rom));
        self.ask_about_box_art();
        Task::batch([self.library_read(), moved])
    }

    /// Moves `rom`, the last game played that older versions kept in
    /// localStorage, into the library. The old entry is only forgotten once
    /// the library's copy has committed (`Message::LastGameMoved`), so it is
    /// never lost, or at once when there is nothing to move: the library
    /// already keeps that game's file (moved at an earlier start, or a newer
    /// file of the same name the player added since, which must not be
    /// replaced), or the entry isn't a game, which is said once.
    #[cfg(target_arch = "wasm32")]
    fn move_last_game(&mut self, rom: &nubo_frontend::rom::Rom) -> Task<Message> {
        let header = match nubo_core::Emulator::read_rom_header(&rom.bytes) {
            Ok(header) => header,
            Err(error) => {
                self.toasts.error(format!(
                    "The last game played, {}, isn't a game Nubo can play: {error}",
                    rom.name()
                ));
                return Task::done(Message::LastGameMoved(Ok(())));
            }
        };
        if self.library.keeps_file_of(&rom.path) {
            return Task::done(Message::LastGameMoved(Ok(())));
        }

        let kept = self
            .library
            .add_game(rom)
            .and_then(|()| self.library.record_played(&rom.path, &header));
        match kept {
            Ok(()) => Task::perform(
                nubo_frontend::storage::indexed_db::settled(),
                Message::LastGameMoved,
            ),
            Err(error) => {
                self.toasts.error(error);
                Task::none()
            }
        }
    }

    /// The window title, including the game's name while one is running.
    pub fn title(&self) -> String {
        match &self.game {
            Some(game) => format!("{} — Nubo", game.session.name()),
            None => String::from("Nubo"),
        }
    }

    /// The colors for the appearance the settings pick, following the
    /// system's light or dark mode for "System".
    pub fn theme(&self) -> Theme {
        theme::for_appearance(self.settings.appearance, self.system_mode)
    }

    /// Handles a message and returns any follow-up work for Iced to run.
    ///
    /// Any message can change whether the game runs, how fast or how loud,
    /// so the game is told afterwards; it only passes on what changed.
    /// Problems the game reported meanwhile, such as a battery save that
    /// couldn't be written when it stopped, become toasts right away.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = match message {
            Message::Game(message) => self.update_game(message),
            Message::Library(message) => self.update_library(message),
            Message::Settings(message) => self.update_settings(message),
            Message::Window(message) => self.update_window(message),
            Message::Dialog(message) => self.update_dialog(message),
            Message::Key(input) => self.handle_key(&input),
            Message::Controller(event) => self.handle_controller(event),
            Message::Escape => self.escape(),
            Message::Tick => {
                if let Some(game) = self.game.as_mut().filter(|_| !self.minimized) {
                    #[cfg(target_arch = "wasm32")]
                    game.session.run_due_frames();
                    game.session.refresh_screen();
                }
                let now = iced::time::Instant::now();
                self.toasts.remove_expired(now);
                if now.saturating_duration_since(self.capture_started) >= CAPTURE_TIMEOUT {
                    self.capture = None;
                }
                Task::none()
            }
            Message::LatestVersion(Ok(version)) => {
                if nubo_frontend::updates::is_newer(&version, env!("CARGO_PKG_VERSION")) {
                    self.newer_version = Some(version);
                }
                Task::none()
            }
            // Nothing the player can do about it; Nubo looks again next time.
            Message::LatestVersion(Err(_)) => Task::none(),
            Message::OpenLink(url) => {
                if let Err(error) = nubo_frontend::links::open(url) {
                    self.toasts.error(error);
                }
                Task::none()
            }
            Message::DismissToast(index) => {
                self.toasts.dismiss(index);
                Task::none()
            }
            #[cfg(not(target_arch = "wasm32"))]
            Message::Scene(message) => self.update_scene(message),
            #[cfg(target_arch = "wasm32")]
            Message::StorageOpened(result) => self.storage_opened(result),
            // Why it couldn't be kept was shown already, and the old entry
            // stays for the next start to try again.
            #[cfg(target_arch = "wasm32")]
            Message::LastGameMoved(result) => {
                if result.is_ok()
                    && let Err(error) = nubo_frontend::rom::forget()
                {
                    self.toasts.error(error);
                }
                Task::none()
            }
        };

        self.close_emptied_dialog();
        self.apply_to_game();
        // The browser's title screen captures share the game's one thread.
        #[cfg(target_arch = "wasm32")]
        nubo_frontend::library::title_screens::hold(self.game.is_some());
        self.save_stepped_sgb_colors();
        self.show_notices();
        Task::batch([task, self.load_slot_thumbnails()])
    }

    /// Reads the pictures of the save state slots a pause menu page that
    /// just opened lists, on a background thread.
    fn load_slot_thumbnails(&mut self) -> Task<Message> {
        let Some((slot_listing, rom_path, slots)) = self
            .game
            .as_mut()
            .and_then(|game| game.session.thumbnails_to_load())
        else {
            return Task::none();
        };
        Task::perform(
            background::run(move || save_states::thumbnails(&rom_path, &slots)),
            move |thumbnails| {
                GameMessage::ThumbnailsLoaded(slot_listing, thumbnails.unwrap_or_default()).into()
            },
        )
    }

    /// The current screen with the settings, drop target and notifications
    /// layered on top, or the game screen alone with `--scale`.
    pub fn view(&self) -> Element<'_, Message> {
        if self.screen_scale.is_some() {
            return views::game::screen_only(self.game.as_ref());
        }

        let mut layers = vec![match &self.game {
            Some(game) => views::game::view(game, &self.settings, self.fullscreen),
            None => views::library::view(
                &self.library,
                &self.settings,
                &self.search,
                self.window_width,
                self.library_selection,
                &self.controllers,
                self.storage_open,
                self.keeps_library(),
                views::library::TopBar {
                    newer_version: self.newer_version.as_deref(),
                    fullscreen: self.fullscreen,
                    fullscreen_key: self
                        .settings
                        .shared
                        .controls
                        .input_label(Device::Keyboard, Action::Fullscreen),
                },
            ),
        }];

        if let Some(tab) = self.settings_tab {
            layers.push(views::modal(
                views::settings::view(
                    tab,
                    &self.settings,
                    &self.library,
                    &self.settings_page_state(),
                    self.settings_focus,
                ),
                Some(SettingsMessage::Close.into()),
            ));
        }

        if let Some((dialog, content)) = self.dialog.as_ref().zip(self.dialog_content()) {
            let dismiss = content
                .click_outside_dismisses
                .then(|| content.dismiss.clone().into_message());
            layers.push(views::modal(views::dialog::view(dialog, content), dismiss));
        }

        if self.file_hovered {
            layers.push(views::drop_target());
        }

        if !self.toasts.is_empty() && !self.hides_toasts() {
            let over_game = self.game.is_some() && self.settings_tab.is_none();
            layers.push(self.toasts.view(
                Message::DismissToast,
                GameMessage::Undo.into(),
                over_game,
            ));
        }

        Stack::with_children(layers).into()
    }

    /// Listens for keys, the mouse, dropped files, theme changes, close
    /// requests and, on macOS and Linux, signals asking the app to stop, and
    /// for frame ticks while a game is running or toasts need to expire. On
    /// the desktop the window only redraws for new frames, not on every
    /// refresh of the screen.
    ///
    /// New frames aren't shown while the window is minimized on Windows,
    /// which reports that as a window without a size. Iced 0.14 passes on no
    /// other sign of a minimized or hidden window (winit's `Occluded` event
    /// goes no further), so on macOS and Linux a hidden window keeps taking
    /// in about 60 frames a second, each copied and laid out, though the
    /// system may skip drawing them. Before the window redrew only for new
    /// frames, `window::frames()` stopped there instead.
    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            keyboard::listen().filter_map(|event| input::to_message(&event)),
            subscriptions::controllers().map(Message::Controller),
            event::listen_with(window_events::message_for),
            system::theme_changes().map(|mode| WindowMessage::SystemThemeChanged(mode).into()),
            subscriptions::library_changes(
                &self.settings.shared.library_folders,
                self.recent_games_elsewhere(),
            )
            .map(|event| LibraryMessage::Watch(event).into()),
            window::close_requests().map(|_| WindowMessage::CloseRequested.into()),
            subscriptions::stop_requests().map(|()| WindowMessage::CloseRequested.into()),
        ];

        #[cfg(not(target_arch = "wasm32"))]
        {
            // The Super Game Boy page recolors the paused game, which the
            // emulator shows as a new frame.
            if let Some(game) = self.game.as_ref().filter(|game| {
                (self.is_running() || game.menu() == Some(PauseMenu::SuperGameBoy))
                    && !self.minimized
            }) {
                subscriptions.push(subscriptions::new_frames(game).map(|()| Message::Tick));
            }
            if let Some(interval) = self.check_interval() {
                subscriptions.push(crate::ticks::every(interval).map(|()| Message::Tick));
            }
        }
        // The browser build emulates on these ticks (see
        // `nubo_frontend::session`).
        #[cfg(target_arch = "wasm32")]
        if self.is_running() || !self.toasts.is_empty() || self.capture.is_some() {
            subscriptions.push(window::frames().map(|_| Message::Tick));
        }

        Subscription::batch(subscriptions)
    }

    /// The model to start the game at `path` with this header on: the one
    /// picked on the command line, else the one picked for the game in its
    /// menu (Automatic or a model), else the one the settings pick.
    fn model_for(&self, path: &Path, header: &CartridgeHeader) -> Model {
        self.model_override.unwrap_or_else(|| {
            self.library
                .entry(path)
                .and_then(LibraryEntry::preference)
                .unwrap_or(self.settings.shared.model)
                .model_for(header)
        })
    }

    /// What to pick for a game with this header in its menu so it starts on
    /// `model` from now on: nothing when the settings pick `model` for it,
    /// else `model` itself.
    fn preference_starting_on(
        &self,
        header: &CartridgeHeader,
        model: Model,
    ) -> Option<ModelPreference> {
        (self.settings.shared.model.model_for(header) != model)
            .then_some(ModelPreference::Fixed(model))
    }

    /// Whether the running game uses another model than the settings pick,
    /// so the settings offer to restart it.
    fn restart_needed(&self) -> bool {
        self.game.as_ref().is_some_and(|game| {
            game.session.model() != self.model_for(&game.session.rom().path, game.session.header())
        })
    }

    /// What the settings pages show besides the settings themselves.
    fn settings_page_state(&self) -> views::settings::PageState<'_> {
        views::settings::PageState {
            restart_needed: self.restart_needed(),
            device: self.controls_device,
            capture: self.capture,
            controllers: &self.controllers,
            box_art_problem: self.box_art_problem.as_deref(),
            newer_version: self.newer_version.as_deref(),
            layout: views::settings::SettingsLayout::new(self.window_width),
            keeps_library: self.keeps_library(),
        }
    }

    /// Whether the D-pad, A, B and Start move through menus instead of
    /// playing: in the library, the pause menu and the settings.
    fn in_menus(&self) -> bool {
        self.dialog.is_some()
            || self.settings_tab.is_some()
            || self.game.as_ref().is_none_or(Game::is_paused)
    }

    /// Steps back from whatever is on top.
    fn escape(&mut self) -> Task<Message> {
        if self.dialog.is_some() {
            return self.dismiss_dialog();
        }
        if self.settings_tab.is_some() {
            self.settings_tab = None;
        } else if let Some(game) = &mut self.game {
            let page = match game.menu() {
                None => Some(PauseMenu::Main),
                Some(PauseMenu::Main) => None,
                Some(
                    PauseMenu::Save | PauseMenu::Load | PauseMenu::Cheats | PauseMenu::SuperGameBoy,
                ) => Some(PauseMenu::Main),
            };
            game.show_menu(page);
        }
        Task::none()
    }

    /// How often the desktop build needs a timer check (see `Message::Tick`),
    /// or `None` when nothing changes with time alone.
    #[cfg(not(target_arch = "wasm32"))]
    fn check_interval(&self) -> Option<std::time::Duration> {
        let bar_hides_later =
            self.fullscreen && self.game.as_ref().is_some_and(Game::controls_hide_later);
        if bar_hides_later || !self.toasts.is_empty() || self.capture.is_some() {
            Some(QUICK_CHECK_INTERVAL)
        } else if self.is_running() {
            Some(NOTICE_CHECK_INTERVAL)
        } else {
            None
        }
    }

    /// Whether a game is running: not paused, not behind the settings or a
    /// dialog, not in a window in the background if the settings pause it
    /// there, and not held still for a scene (see `scenes`).
    fn is_running(&self) -> bool {
        let in_background = !self.focused && self.settings.pause_in_background;
        self.game.as_ref().is_some_and(|game| {
            !game.is_paused()
                && self.settings_tab.is_none()
                && self.dialog.is_none()
                && !in_background
                && !self.held_for_scene()
        })
    }

    /// Whether the toasts are left out of the window, while a scene's
    /// picture is taken: scenes don't keep them (see `scenes`).
    #[cfg(not(target_arch = "wasm32"))]
    fn hides_toasts(&self) -> bool {
        self.recording.is_some()
    }

    /// The browser build has no scenes, so its toasts always show.
    #[cfg(target_arch = "wasm32")]
    #[expect(
        clippy::unused_self,
        reason = "the desktop build's version reads the app's scenes"
    )]
    fn hides_toasts(&self) -> bool {
        false
    }

    /// The browser build has no scenes, so nothing holds its game still.
    #[cfg(target_arch = "wasm32")]
    #[expect(
        clippy::unused_self,
        reason = "the desktop build's version reads the app's scenes"
    )]
    fn held_for_scene(&self) -> bool {
        false
    }

    /// Tells the emulator whether to run, how fast and how loud, and saves
    /// where the player left off when it's time (see
    /// `game::AutosaveTimer`).
    fn apply_to_game(&mut self) {
        let running = self.is_running();
        let Some(game) = &mut self.game else {
            return;
        };
        game.session.apply(running, &self.settings.shared);
        if game.keeps_data && game.autosave.due(running, iced::time::Instant::now()) {
            self.save_where_left_off();
        }
    }

    /// How games' pictures look: as the settings say, or the emulator's own
    /// picture with `--raw-screen`.
    fn screen_look(&self) -> ScreenLook {
        if self.raw_screen {
            ScreenLook::RAW
        } else {
            self.settings.shared.screen_look()
        }
    }

    /// Changes the settings and saves them, unless the app keeps nothing
    /// (`--fresh`).
    fn change_settings(&mut self, change: impl FnOnce(&mut Settings)) {
        change(&mut self.settings);
        self.sgb_colors_unsaved = false;

        if !self.fresh
            && let Err(error) = self.settings.save()
        {
            self.toasts.error(error);
        }
    }

    /// Saves the Super Game Boy colors the D-pad changed once the player has
    /// left their editor (or the game), rather than on every step.
    fn save_stepped_sgb_colors(&mut self) {
        let editing = self.game.as_ref().is_some_and(|game| {
            game.sgb_editor_open && game.menu() == Some(PauseMenu::SuperGameBoy)
        });
        if self.sgb_colors_unsaved && !editing {
            self.change_settings(|_| {});
        }
    }

    /// Turns problems reported by the running game, and in the browser
    /// what its storage couldn't keep, into toasts.
    fn show_notices(&mut self) {
        for message in self.notices.take() {
            self.toasts.error(message);
        }
        #[cfg(target_arch = "wasm32")]
        for message in nubo_frontend::storage::indexed_db::take_errors() {
            self.toasts.error(message);
        }
    }
}

/// Moves `step` places from `current` in `options`, stopping at the ends,
/// or starting over at the other end with `wrap`. With no options, stays on
/// `current`.
fn cycle<T: Copy + PartialEq>(options: &[T], current: T, step: isize, wrap: bool) -> T {
    let index = options
        .iter()
        .position(|&option| option == current)
        .unwrap_or(0);
    options
        .get(selection::moved(index, step, options.len(), wrap))
        .copied()
        .unwrap_or(current)
}

/// Scrolls the scrollable with `id` so item `index` of `count` is roughly in
/// view. The items of a settings page or a question are spread fairly evenly
/// over it, from its start to its end, so scrolling in proportion is close
/// enough and needs no knowledge of their sizes. A lone item counts as the
/// last, at the end: the only row of the browser's About tab is.
fn scroll_in_proportion(id: iced::widget::Id, index: usize, count: usize) -> Task<Message> {
    let fraction = if count > 1 {
        index as f32 / (count - 1) as f32
    } else {
        1.0
    };

    iced::widget::operation::snap_to(
        id,
        iced::widget::scrollable::RelativeOffset {
            x: None,
            y: Some(fraction),
        },
    )
}

mod dialogs;
mod library_screen;
mod player_input;
mod playing;
#[cfg(not(target_arch = "wasm32"))]
mod scenes;
mod settings_window;
#[cfg(test)]
mod tests;
mod window_events;
