use app::App;
use iced::Theme;
use std::env;
use std::path::PathBuf;

const DEFAULT_SCALE_FACTOR: f32 = 4.0;
const TOOLBAR_HEIGHT: f32 = 48.0;

fn parse_args() -> (Option<PathBuf>, bool) {
    let mut rom_path = None;
    let mut cgb_mode = false;

    for arg in env::args().skip(1) {
        if arg == "--cgb" {
            cgb_mode = true;
        } else {
            rom_path = Some(PathBuf::from(arg));
        }
    }

    (rom_path, cgb_mode)
}

fn main() -> iced::Result {
    let (rom_path, cgb_mode) = parse_args();

    iced::application(
        move || App::new(rom_path.clone(), cgb_mode),
        App::update,
        App::view,
    )
    .title(App::title)
    .subscription(App::subscription)
    .theme(Theme::Dark)
    .window_size((
        app::SCREEN_WIDTH as f32 * DEFAULT_SCALE_FACTOR,
        app::SCREEN_HEIGHT as f32 * DEFAULT_SCALE_FACTOR + TOOLBAR_HEIGHT,
    ))
    .centered()
    .run()
}

mod app;
mod audio;
mod cartridge_effects;
mod emulation;
mod input;
