//! Desktop frontend built with Iced. The game runs in
//! `nubo_frontend::session` (on a thread of its own on the desktop, on each
//! frame tick in the browser build) and the window shows the latest frame.
//! Run it with `cargo play [rom] [--cgb] [--model <name>]`.

// Release builds are Windows apps rather than console programs, so no empty
// console window opens next to Nubo. Debug builds keep the console for
// their messages.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use app::App;
use nubo_core::Model;
use nubo_frontend::rom::Rom;
use std::env;
use std::path::PathBuf;

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

/// Reads the optional ROM path and the `--cgb` / `--model <name>` options
/// from the command line. Invalid options are reported and ignored.
fn parse_args() -> (Option<PathBuf>, Option<Model>) {
    let mut rom_path = None;
    let mut model = None;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--cgb" {
            model = Some(Model::CgbE);
        } else if arg == "--model" {
            match args.next().map(|name| name.parse()) {
                Some(Ok(parsed)) => model = Some(parsed),
                Some(Err(error)) => eprintln!("{error}"),
                None => eprintln!("--model needs a value such as dmg, mgb, sgb, cgb or agb"),
            }
        } else {
            rom_path = Some(PathBuf::from(arg));
        }
    }

    (rom_path, model)
}

fn main() -> iced::Result {
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();

    #[cfg(not(target_arch = "wasm32"))]
    if let Some(exit_code) = efficiency::run_from_command_line() {
        std::process::exit(exit_code);
    }

    let (rom_path, model) = parse_args();
    let rom = rom_path.map(|path| Rom::read(&path));

    iced::application(move || App::new(rom.clone(), model), App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .font(ui::icons::FONT_BYTES)
        .window(iced::window::Settings {
            // Wide enough for a whole row of library cards, and high enough
            // for the game screen at four times its size.
            size: iced::Size::new(
                (app::SCREEN_WIDTH as f32 * DEFAULT_SCALE_FACTOR + SCREEN_PADDING)
                    .max(views::library::FULL_ROW_WINDOW_WIDTH),
                app::SCREEN_HEIGHT as f32 * DEFAULT_SCALE_FACTOR + SCREEN_PADDING + TOP_BAR_HEIGHT,
            ),
            min_size: Some(MINIMUM_SIZE.into()),
            icon: iced::window::icon::from_rgba(
                nubo_logo::render(WINDOW_ICON_SIZE, 0.0),
                WINDOW_ICON_SIZE,
                WINDOW_ICON_SIZE,
            )
            .ok(),
            position: iced::window::Position::Centered,
            // Closing the window asks the app first (`Message::CloseRequested`),
            // so it can stop the game: that writes where the player left off,
            // the game's thumbnail and the battery saves still waiting. On
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
        })
        .run()
}

mod app;
#[cfg(not(target_arch = "wasm32"))]
mod efficiency;
mod game;
mod input;
mod menu_presses;
mod rom_picker;
mod settings;
mod subscriptions;
#[cfg(not(target_arch = "wasm32"))]
mod ticks;
mod ui;
mod views;
