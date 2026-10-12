//! The window itself: its size and fullscreen mode, the mouse over it,
//! files dragged onto it, the system's light or dark mode, and closing it.

use super::playing::read_rom;
use super::{App, Message};
use iced::theme::Mode;
use iced::{Event, Size, Task, event, mouse, window};
use std::path::PathBuf;

/// What happens to the window.
#[derive(Debug, Clone)]
pub enum WindowMessage {
    ToggleFullscreen,
    /// The mouse moved to this height in the window.
    MouseMoved(f32),
    /// The window opened or changed size, perhaps because it entered or left
    /// fullscreen or was minimized.
    Resized(Size),
    FullscreenChanged(bool),
    FileHovered,
    FileHoverLeft,
    FileDropped(PathBuf),
    SystemThemeChanged(Mode),
    CloseRequested,
}

impl From<WindowMessage> for Message {
    fn from(message: WindowMessage) -> Self {
        Message::Window(message)
    }
}

impl App {
    pub(super) fn update_window(&mut self, message: WindowMessage) -> Task<Message> {
        match message {
            WindowMessage::ToggleFullscreen => return toggle_fullscreen(),
            WindowMessage::MouseMoved(y) => {
                if let Some(game) = &mut self.game {
                    game.mouse_moved(y, self.window_height);
                }
            }
            WindowMessage::Resized(size) => {
                self.minimized = size.width <= 0.0 || size.height <= 0.0;
                if !self.minimized {
                    self.window_width = size.width;
                    self.window_height = size.height;
                }
                return window::latest().and_then(window::mode).map(|mode| {
                    WindowMessage::FullscreenChanged(mode == window::Mode::Fullscreen).into()
                });
            }
            WindowMessage::FullscreenChanged(fullscreen) => self.fullscreen = fullscreen,
            WindowMessage::FileHovered => self.file_hovered = true,
            WindowMessage::FileHoverLeft => self.file_hovered = false,
            WindowMessage::FileDropped(path) => {
                self.file_hovered = false;
                return read_rom(path);
            }
            WindowMessage::SystemThemeChanged(mode) => self.system_mode = mode,
            WindowMessage::CloseRequested => {
                self.stop_game();
                return iced::exit();
            }
        }

        Task::none()
    }
}

/// Switches the window between fullscreen and a normal window.
pub(super) fn toggle_fullscreen() -> Task<Message> {
    window::latest().and_then(|id| {
        window::mode(id).then(move |mode| {
            let mode = if mode == window::Mode::Fullscreen {
                window::Mode::Windowed
            } else {
                window::Mode::Fullscreen
            };
            window::set_mode(id, mode)
        })
    })
}

/// Turns mouse movements, window sizes and file drag-and-drop events into
/// messages.
pub(super) fn message_for(
    event: Event,
    _status: event::Status,
    _window: window::Id,
) -> Option<Message> {
    let message = match event {
        Event::Mouse(mouse::Event::CursorMoved { position }) => {
            WindowMessage::MouseMoved(position.y)
        }
        Event::Window(window::Event::Opened { size, .. } | window::Event::Resized(size)) => {
            WindowMessage::Resized(size)
        }
        Event::Window(window::Event::FileHovered(_)) => WindowMessage::FileHovered,
        Event::Window(window::Event::FilesHoveredLeft) => WindowMessage::FileHoverLeft,
        Event::Window(window::Event::FileDropped(path)) => WindowMessage::FileDropped(path),
        _ => return None,
    };
    Some(message.into())
}
