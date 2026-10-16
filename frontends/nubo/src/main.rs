//! The app, built with Iced, for the desktop and the browser. The game runs
//! in `nubo_frontend::session` (on a thread of its own on the desktop, on
//! each frame tick in the browser build) and the window shows the latest
//! frame.
//! Run it with `cargo play [rom] [--cgb | --model <name>] [--scale <n>]
//! [--fresh] [--speed <n>] [--raw-screen]`; `command_line` explains the
//! options, and `cargo play --help` lists them.

// Release builds are Windows apps rather than console programs, so no empty
// console window opens next to Nubo. Debug builds keep the console for
// their messages.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use app::App;
use command_line::Options;
use nubo_core::Emulator;
use nubo_frontend::rom::Rom;
use std::process::ExitCode;

const DEFAULT_SCALE_FACTOR: f32 = 4.0;
const TOP_BAR_HEIGHT: f32 = 52.0;
const SCREEN_PADDING: f32 = 24.0;
const MINIMUM_SIZE: (f32, f32) = (480.0, 420.0);
/// The size of the window icon, which taskbars scale down as they need.
const WINDOW_ICON_SIZE: u32 = 256;
/// The name Linux desktops match the window to the app's menu entry by. GNOME
/// on Wayland ignores the window icon and shows the entry's icon instead. The
/// Flatpak's menu entry (`frontends/nubo/linux/`) is named after the same id.
#[cfg(target_os = "linux")]
const APPLICATION_ID: &str = "io.github.mschoenm.nubo";

/// Reads the ROM the command line names. A window that shows only the game
/// (`--scale`) has nowhere to show why a ROM couldn't start, so for one the
/// ROM must also have a cartridge header the emulator can start; otherwise
/// this returns why not, to print before the window opens.
fn read_rom(options: &Options) -> Result<Option<Result<Rom, String>>, String> {
    let Some(path) = &options.rom_path else {
        return Ok(None);
    };
    let rom = Rom::read(path);

    if options.scale.is_some() {
        let readable = rom.as_ref().map_err(Clone::clone)?;
        Emulator::read_rom_header(&readable.bytes)
            .map_err(|error| format!("Couldn't start {}: {error}", readable.name()))?;
    }
    Ok(Some(rom))
}

fn main() -> ExitCode {
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();

    #[cfg(not(target_arch = "wasm32"))]
    let options = match command_line::parse(std::env::args_os()) {
        Ok(command_line::Request::Play(options)) => options,
        Ok(command_line::Request::MeasureEfficiency { rom_path, model }) => {
            return efficiency::run(&rom_path, model);
        }
        Ok(command_line::Request::Capture(request)) => return capture(&request),
        Err(error) => error.exit(),
    };
    #[cfg(target_arch = "wasm32")]
    let options = Options::default();

    let rom = match read_rom(&options) {
        Ok(rom) => rom,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };

    match run(rom, options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Opens the window and runs the app until it closes.
fn run(rom: Option<Result<Rom, String>>, options: Options) -> iced::Result {
    let window = match options.scale {
        Some(scale) => screen_only_window(scale),
        None if options.fresh => default_window(None),
        None => default_window(settings::Settings::load().0.window_size),
    };

    launch(window, move || App::new(rom.clone(), &options))
}

/// The hidden `capture` command: shows a scene in a window of the scene's
/// size, or filling the screen, and saves a picture of it or keeps showing
/// it (see `app::scenes`).
#[cfg(not(target_arch = "wasm32"))]
fn capture(request: &command_line::CaptureRequest) -> ExitCode {
    let prepared = match app::prepare_capture(request) {
        Ok(prepared) => prepared,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let window = match prepared.options.scale {
        Some(scale) => screen_only_window(scale),
        None => iced::window::Settings {
            fullscreen: prepared.fullscreen,
            ..default_window(prepared.size)
        },
    };
    let (options, scene) = (prepared.options, prepared.run);
    let result = launch(window, move || {
        let (mut app, task) = App::new(None, &options);
        let scene_task = app.start_scene(scene.clone());
        (app, iced::Task::batch([task, scene_task]))
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Runs the app in a `window` set up as these settings say, starting it
/// with `boot`, until it closes.
fn launch(
    window: iced::window::Settings,
    boot: impl Fn() -> (App, iced::Task<app::Message>) + 'static,
) -> iced::Result {
    let application = iced::application(boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .font(ui::icons::FONT_BYTES);
    ui::theme::FONT_BYTES
        .into_iter()
        .fold(application, iced::Application::font)
        .default_font(ui::theme::BODY_FONT)
        .window(window)
        .run()
}

/// The window the app usually opens: at `saved_size`, the size it had when
/// it last closed (at least the smallest size the window allows; a size that
/// isn't a finite number, from a damaged `settings.toml`, is ignored), or
/// else wide enough for a whole row of library cards and high enough for the
/// game screen at four times its size.
fn default_window(saved_size: Option<(f32, f32)>) -> iced::window::Settings {
    let saved_size = saved_size.filter(|(width, height)| width.is_finite() && height.is_finite());
    let size = match saved_size {
        Some((width, height)) => {
            iced::Size::new(width.max(MINIMUM_SIZE.0), height.max(MINIMUM_SIZE.1))
        }
        None => iced::Size::new(
            (app::SCREEN_WIDTH as f32 * DEFAULT_SCALE_FACTOR + SCREEN_PADDING)
                .max(views::library::FULL_ROW_WINDOW_WIDTH),
            app::SCREEN_HEIGHT as f32 * DEFAULT_SCALE_FACTOR + SCREEN_PADDING + TOP_BAR_HEIGHT,
        ),
    };
    iced::window::Settings {
        size,
        min_size: Some(MINIMUM_SIZE.into()),
        ..window_settings()
    }
}

/// The window for `--scale`: just the game screen at `scale` times its size,
/// which the player can't resize. It opens at that size in logical pixels;
/// once it is open, the app makes it exact in the display's own pixels
/// (`app::screen_only_size`).
fn screen_only_window(scale: u32) -> iced::window::Settings {
    iced::window::Settings {
        size: app::screen_only_size(scale, 1.0, (app::SCREEN_WIDTH, app::SCREEN_HEIGHT)),
        resizable: false,
        ..window_settings()
    }
}

/// What every window of the app has: the icon, its place, and asking the app
/// before it closes.
fn window_settings() -> iced::window::Settings {
    iced::window::Settings {
        icon: iced::window::icon::from_rgba(
            nubo_logo::render(WINDOW_ICON_SIZE, 0.0),
            WINDOW_ICON_SIZE,
            WINDOW_ICON_SIZE,
        )
        .ok(),
        position: iced::window::Position::Centered,
        // Closing the window asks the app first (`Message::CloseRequested`),
        // so it can stop the game: that writes where the player left off,
        // the game's thumbnail and the battery saves still waiting. SIGTERM
        // and the other stop signals do the same on macOS and Linux
        // (`subscriptions::stop_requests`). On
        // macOS, Quit in the app menu (⌘Q) or the Dock ends the process
        // without asking: winit's default menu calls `terminate:`, and
        // Iced 0.14 doesn't pass winit's `exiting` event on. The battery
        // saves are on disk by then all the same, unless the game wrote
        // them in the last quarter of a second (see
        // `nubo_frontend::battery`), but the Resume slot and the thumbnail
        // aren't written. This hasn't been checked on a Mac yet.
        exit_on_close_request: false,
        #[cfg(target_os = "linux")]
        platform_specific: iced::window::settings::PlatformSpecific {
            application_id: APPLICATION_ID.to_owned(),
            ..iced::window::settings::PlatformSpecific::default()
        },
        ..iced::window::Settings::default()
    }
}

mod app;
mod command_line;
mod dialog;
#[cfg(not(target_arch = "wasm32"))]
mod efficiency;
mod game;
mod input;
mod menu_presses;
mod rom_picker;
mod selection;
mod settings;
mod subscriptions;
#[cfg(not(target_arch = "wasm32"))]
mod ticks;
mod ui;
mod views;

#[cfg(test)]
mod tests;
