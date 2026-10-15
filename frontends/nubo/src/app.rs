//! The application window. It shows the game library until a game starts
//! and the game screen while one runs; the settings, the drop target and
//! notifications are drawn on top.
//!
//! The messages come in groups, each handled in a file of its own: the
//! running game and its pause menu (`app/playing.rs`), the library
//! (`app/library_screen.rs`), the settings window (`app/settings_window.rs`),
//! the window itself (`app/window_events.rs`), and keys and controller
//! buttons (`app/player_input.rs`).

use crate::command_line::Options;
use crate::dialog::Dialog;
use crate::game::{Game, PauseMenu};
use crate::input::{self, KeyInput};
use crate::menu_presses::MenuPresses;
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
use nubo_frontend::notices::Notices;
use nubo_frontend::rom::Rom;
use nubo_frontend::save_states;
use nubo_frontend::settings::FastForwardSpeed;
use std::path::Path;
use std::sync::Arc;

pub use dialogs::DialogMessage;
pub use library_screen::LibraryMessage;
pub use nubo_frontend::picture::{SCREEN_HEIGHT, SCREEN_WIDTH};
pub use playing::GameMessage;
pub use settings_window::SettingsMessage;
pub use window_events::{WindowMessage, screen_only_size};

/// How often the desktop build checks for toasts to take down and for the
/// fullscreen bar to hide, while there are any.
#[cfg(not(target_arch = "wasm32"))]
const QUICK_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);
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
    /// A model picked on the command line, used instead of the settings
    /// until the user picks one in the settings.
    model_override: Option<Model>,
    /// With `--scale`: the window shows only the game screen, this many
    /// times its size, and nothing else can be brought up (see
    /// `command_line`).
    screen_scale: Option<u32>,
    /// With `--fresh`: the app reads and writes nothing kept between runs,
    /// like a new install (see `command_line`).
    fresh: bool,
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
    /// Where the D-pad is in the settings window, once the player has used
    /// it there.
    settings_focus: Option<SettingsFocus>,
    /// The No-Intro game lists that box art is looked up with, once read.
    game_list: Option<Arc<GameList>>,
    loading_game_list: bool,
    /// Why box art couldn't be downloaded this run, shown in the settings'
    /// Library tab. Nubo stops trying until the player asks it to or the
    /// app starts again, and never shows a toast for it.
    box_art_problem: Option<String>,
    /// The dialog over the screen, if one is open.
    dialog: Option<Dialog>,
    /// What the newest toast's Undo button puts back, if it has one.
    undo: Option<playing::UndoPoint>,
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
    DismissToast(usize),
    /// Opens this web page in the player's browser.
    OpenLink(&'static str),
}

impl App {
    /// Creates the application with the saved settings and library (or the
    /// defaults and an empty library with `--fresh`), starting the ROM given
    /// on the command line if there is one, as the other `options` say.
    pub fn new(rom: Option<Result<Rom, String>>, options: &Options) -> (Self, Task<Message>) {
        let (settings, settings_error) = if options.fresh {
            (Settings::default(), None)
        } else {
            Settings::load()
        };
        let (library, library_error) = if options.fresh {
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
            speed: options.speed,
            library,
            search: String::new(),
            game: None,
            settings_tab: None,
            file_hovered: false,
            fullscreen: false,
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
            settings_focus: None,
            game_list: None,
            loading_game_list: false,
            box_art_problem: None,
            dialog: None,
            undo: None,
        };

        // Saving over settings that couldn't be read would lose them.
        let settings_read = settings_error.is_none();
        for error in [settings_error, library_error].into_iter().flatten() {
            app.toasts.error(error);
        }
        if settings_read && app.settings.shared.controls_revision < controls::REVISION {
            app.change_settings(|settings| {
                controls::add_new_actions(&mut settings.shared.controls);
                settings.shared.controls_revision = controls::REVISION;
            });
        }

        if let Some(rom) = rom {
            app.open(rom);
        }
        app.ask_about_box_art();

        let check = app.check_files();
        let scan = app.rescan();
        let pictures = app.load_pictures(false);
        (
            app,
            Task::batch([
                system::theme().map(|mode| WindowMessage::SystemThemeChanged(mode).into()),
                check,
                scan,
                pictures,
            ]),
        )
    }

    /// The window title, including the game's name while one is running.
    pub fn title(&self) -> String {
        match &self.game {
            Some(game) => format!("{} — Nubo", game.session.name()),
            None => String::from("Nubo"),
        }
    }

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
                self.toasts.remove_expired(iced::time::Instant::now());
                Task::none()
            }
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
        };

        self.apply_to_game();
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
                self.library_selection,
                &self.controllers,
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

        if !self.toasts.is_empty() {
            let over_game = self.game.is_some() && self.settings_tab.is_none();
            layers.push(self.toasts.view(
                Message::DismissToast,
                GameMessage::Undo.into(),
                over_game,
            ));
        }

        Stack::with_children(layers).into()
    }

    /// Listens for keys, the mouse, dropped files, theme changes and close
    /// requests, and for frame ticks while a game is running or toasts need
    /// to expire. On the desktop the window only redraws for new frames, not
    /// on every refresh of the screen.
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
        if self.is_running() || !self.toasts.is_empty() {
            subscriptions.push(window::frames().map(|_| Message::Tick));
        }

        Subscription::batch(subscriptions)
    }

    /// The model to start the game at `path` with this header on: the one
    /// picked on the command line, else the one picked for the game in its
    /// menu, else the one the settings pick.
    fn model_for(&self, path: &Path, header: &CartridgeHeader) -> Model {
        self.model_override
            .or_else(|| self.library.entry(path).and_then(LibraryEntry::model))
            .unwrap_or_else(|| self.settings.shared.model.model_for(header))
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
        if bar_hides_later || !self.toasts.is_empty() {
            Some(QUICK_CHECK_INTERVAL)
        } else if self.is_running() {
            Some(NOTICE_CHECK_INTERVAL)
        } else {
            None
        }
    }

    /// Whether a game is running: not paused, not behind the settings or a
    /// dialog, and not in a window in the background if the settings pause
    /// it there.
    fn is_running(&self) -> bool {
        let in_background = !self.focused && self.settings.pause_in_background;
        self.game.as_ref().is_some_and(|game| {
            !game.is_paused()
                && self.settings_tab.is_none()
                && self.dialog.is_none()
                && !in_background
        })
    }

    /// Tells the emulator whether to run, how fast and how loud.
    fn apply_to_game(&mut self) {
        let running = self.is_running();
        if let Some(game) = &mut self.game {
            game.session.apply(running, &self.settings.shared);
        }
    }

    /// Changes the settings and saves them, unless the app keeps nothing
    /// (`--fresh`).
    fn change_settings(&mut self, change: impl FnOnce(&mut Settings)) {
        change(&mut self.settings);

        if !self.fresh
            && let Err(error) = self.settings.save()
        {
            self.toasts.error(error);
        }
    }

    /// Turns problems reported by the running game into toasts.
    fn show_notices(&mut self) {
        for message in self.notices.take() {
            self.toasts.error(message);
        }
    }
}

/// Moves `step` places from `current` in `options`, stopping at the ends,
/// or starting over at the other end with `wrap`.
fn cycle<T: Copy + PartialEq>(options: &[T], current: T, step: isize, wrap: bool) -> T {
    let count = options.len().cast_signed();
    let index = options
        .iter()
        .position(|&option| option == current)
        .unwrap_or(0)
        .cast_signed();

    let next = if wrap {
        (index + step).rem_euclid(count)
    } else {
        (index + step).clamp(0, count - 1)
    };
    options[next.cast_unsigned()]
}

mod dialogs;
mod library_screen;
mod player_input;
mod playing;
mod settings_window;
#[cfg(test)]
mod tests;
mod window_events;
