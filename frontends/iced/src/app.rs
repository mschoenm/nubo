use crate::emulation::Session;
use crate::input;
use iced::widget::{button, checkbox, column, container, image, row, text};
use iced::{keyboard, window, Alignment, ContentFit, Element, Fill, Subscription, Task};
use retroboy_core::emulator::Key;
use std::fs;
use std::path::PathBuf;

pub const SCREEN_WIDTH: u32 = 160;
pub const SCREEN_HEIGHT: u32 = 144;

const CONTROLS_HELP: &str = "Arrows: D-pad   Z: A   X: B   Enter: Start   Space: Select";

pub struct App {
    session: Option<Session>,
    rom_path: Option<PathBuf>,
    screen: Option<image::Handle>,
    last_frame: u64,
    cgb_mode: bool,
    paused: bool,
    error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    OpenRom,
    RomSelected(Option<PathBuf>),
    Reset,
    TogglePause,
    CgbModeToggled(bool),
    FrameTick,
    KeyPressed(Key),
    KeyReleased(Key),
}

impl App {
    pub fn new(rom_path: Option<PathBuf>, cgb_mode: bool) -> Self {
        let mut app = App {
            session: None,
            rom_path: None,
            screen: None,
            last_frame: 0,
            cgb_mode,
            paused: false,
            error: None,
        };

        if let Some(path) = rom_path {
            app.load_rom(path);
        }

        app
    }

    pub fn title(&self) -> String {
        match &self.session {
            Some(session) => format!("RetroBoy - {}", session.title()),
            None => String::from("RetroBoy"),
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenRom => return Task::perform(pick_rom(), Message::RomSelected),
            Message::RomSelected(Some(path)) => self.load_rom(path),
            Message::RomSelected(None) => {}
            Message::Reset => self.reload_rom(),
            Message::TogglePause => {
                self.paused = !self.paused;
                if let Some(session) = &self.session {
                    session.set_paused(self.paused);
                }
            }
            Message::CgbModeToggled(enabled) => {
                self.cgb_mode = enabled;
                self.reload_rom();
            }
            Message::FrameTick => self.refresh_screen(),
            Message::KeyPressed(key) => {
                if let Some(session) = &self.session {
                    session.press(key);
                }
            }
            Message::KeyReleased(key) => {
                if let Some(session) = &self.session {
                    session.release(key);
                }
            }
        }

        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let toolbar = row![
            button("Open ROM").on_press(Message::OpenRom),
            button(if self.paused { "Resume" } else { "Pause" })
                .on_press_maybe(self.session.as_ref().map(|_| Message::TogglePause)),
            button("Reset").on_press_maybe(self.rom_path.as_ref().map(|_| Message::Reset)),
            checkbox(self.cgb_mode)
                .label("Game Boy Color")
                .on_toggle(Message::CgbModeToggled),
        ]
        .spacing(8)
        .padding(8)
        .align_y(Alignment::Center);

        let screen: Element<'_, Message> = match (&self.screen, &self.error) {
            (_, Some(error)) => text(error).into(),
            (Some(handle), None) => image(handle.clone())
                .filter_method(image::FilterMethod::Nearest)
                .content_fit(ContentFit::Contain)
                .width(Fill)
                .height(Fill)
                .into(),
            (None, None) if self.session.is_none() => {
                column![text("Open a ROM to start playing"), text(CONTROLS_HELP).size(12)]
                    .spacing(8)
                    .align_x(Alignment::Center)
                    .into()
            }
            (None, None) => text("").into(),
        };

        column![toolbar, container(screen).center(Fill)].into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let keys = keyboard::listen().filter_map(input::to_message);

        if self.session.is_some() && !self.paused {
            Subscription::batch([keys, window::frames().map(|_| Message::FrameTick)])
        } else {
            keys
        }
    }

    fn load_rom(&mut self, path: PathBuf) {
        self.session = None;
        self.screen = None;
        self.last_frame = 0;
        self.paused = false;

        let result = fs::read(&path)
            .map_err(|error| error.to_string())
            .and_then(|rom| Session::start(rom, self.cgb_mode));

        match result {
            Ok(session) => {
                self.session = Some(session);
                self.error = None;
            }
            Err(error) => {
                self.error = Some(format!("Could not load {}: {}", path.display(), error));
            }
        }

        self.rom_path = Some(path);
    }

    fn reload_rom(&mut self) {
        if let Some(path) = self.rom_path.clone() {
            self.load_rom(path);
        }
    }

    fn refresh_screen(&mut self) {
        let Some(session) = &self.session else {
            return;
        };

        if let Some((sequence, pixels)) = session.frame_since(self.last_frame) {
            self.last_frame = sequence;
            self.screen = Some(image::Handle::from_rgba(SCREEN_WIDTH, SCREEN_HEIGHT, pixels));
        }
    }
}

async fn pick_rom() -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .add_filter("Game Boy ROMs", &["gb", "gbc"])
        .set_title("Select a Game Boy ROM")
        .pick_file()
        .await
        .map(|file| file.path().to_path_buf())
}
