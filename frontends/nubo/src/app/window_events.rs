//! The window itself: its size and fullscreen mode, the mouse over it,
//! files dragged onto it, the system's light or dark mode, and closing it.

use super::playing::read_rom;
use super::{App, Message, SCREEN_HEIGHT, SCREEN_WIDTH};
use iced::theme::Mode;
use iced::{Event, Size, Task, event, mouse, window};
use nubo_core::{SGB_FRAME_HEIGHT, SGB_FRAME_WIDTH};
use std::path::PathBuf;

/// What happens to the window.
#[derive(Debug, Clone)]
pub enum WindowMessage {
    ToggleFullscreen,
    /// The mouse moved to this height in the window.
    MouseMoved(f32),
    /// The window opened with this size.
    Opened(window::Id, Size),
    /// The window changed size, perhaps because it entered or left
    /// fullscreen or was minimized.
    Resized(Size),
    /// The display's scaling changed for the window, such as when it moved
    /// to another monitor.
    Rescaled(window::Id),
    FullscreenChanged(bool),
    FileHovered,
    FileHoverLeft,
    FileDropped(PathBuf),
    /// The window became the one in front (`true`) or stopped being it.
    Focused(bool),
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
                    game.mouse_moved(y);
                }
            }
            WindowMessage::Opened(id, size) => {
                return Task::batch([
                    self.update_window(WindowMessage::Resized(size)),
                    self.fit_to_screen(id),
                ]);
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
            WindowMessage::Rescaled(id) => return self.fit_to_screen(id),
            WindowMessage::FullscreenChanged(fullscreen) => {
                if fullscreen != self.fullscreen {
                    self.fullscreen_for_game = fullscreen && self.game.is_some();
                }
                self.fullscreen = fullscreen;
            }
            WindowMessage::FileHovered => self.file_hovered = true,
            WindowMessage::FileHoverLeft => self.file_hovered = false,
            WindowMessage::FileDropped(path) => {
                self.file_hovered = false;
                if self.screen_scale.is_none() {
                    return read_rom(path);
                }
            }
            WindowMessage::SystemThemeChanged(mode) => self.system_mode = mode,
            WindowMessage::Focused(focused) => {
                self.focused = focused;
                // Keys let go of in another window never come up here.
                if !focused && let Some(game) = &mut self.game {
                    game.session.release_held_keys();
                }
            }
            WindowMessage::CloseRequested => {
                self.stop_game();
                self.remember_window_size();
                return iced::exit();
            }
        }

        Task::none()
    }

    /// Keeps the window's size in the settings, so it opens at that size
    /// next time, unless it fills the screen, is minimized, shows only the
    /// game (`--scale`), the app keeps nothing (`--fresh`) or the settings
    /// couldn't be read (saving would write the defaults over them).
    fn remember_window_size(&mut self) {
        let size = (self.window_width, self.window_height);
        if cfg!(target_arch = "wasm32")
            || self.fresh
            || !self.settings_read
            || self.screen_scale.is_some()
            || self.fullscreen
            || self.minimized
            || size.0 <= 0.0
            || self.settings.window_size == Some(size)
        {
            return;
        }
        self.change_settings(|settings| settings.window_size = Some(size));
    }

    /// Sizes a window that shows only the game (`--scale`) so its inside is
    /// exactly the game screen at its scale in the display's own pixels,
    /// whatever the display's scaling (see [`screen_only_size`]).
    fn fit_to_screen(&self, id: window::Id) -> Task<Message> {
        let Some(scale) = self.screen_scale else {
            return Task::none();
        };
        let picture = self.screen_size();
        window::scale_factor(id).then(move |scale_factor| {
            window::resize(id, screen_only_size(scale, scale_factor, picture))
        })
    }

    /// The size, in Game Boy pixels, of the picture the game shows: the
    /// Game Boy screen, or on a Super Game Boy the picture with its border
    /// unless the settings hide it.
    fn screen_size(&self) -> (u32, u32) {
        match &self.game {
            Some(game) if game.session.model().is_sgb() && self.settings.shared.sgb_border => {
                (SGB_FRAME_WIDTH, SGB_FRAME_HEIGHT)
            }
            _ => (SCREEN_WIDTH, SCREEN_HEIGHT),
        }
    }
}

/// The size, in Iced's logical pixels, of a window whose inside is exactly
/// the game's picture of `width` by `height` Game Boy pixels (160×144, or
/// 256×224 with a Super Game Boy's border) `scale` times its size in pixels
/// of a display that scales by `scale_factor`, such as 1.5 at 150%. Iced and
/// winit multiply logical sizes by the scale factor and round, so the window
/// comes out exact.
pub fn screen_only_size(scale: u32, scale_factor: f32, (width, height): (u32, u32)) -> Size {
    let pixels = |length: u32| (length * scale) as f32 / scale_factor;
    Size::new(pixels(width), pixels(height))
}

/// Puts a window that went fullscreen for a game back to a normal window,
/// as the game closes: fullscreen is for playing.
pub(super) fn leave_fullscreen_for_game(app: &mut App) -> Task<Message> {
    if !(app.fullscreen && app.fullscreen_for_game) {
        return Task::none();
    }
    app.fullscreen_for_game = false;
    window::latest().and_then(|id| window::set_mode(id, window::Mode::Windowed))
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

/// Turns mouse movements, window sizes, scaling and focus, and file
/// drag-and-drop events into messages.
pub(super) fn message_for(
    event: Event,
    _status: event::Status,
    window: window::Id,
) -> Option<Message> {
    let message = match event {
        Event::Mouse(mouse::Event::CursorMoved { position }) => {
            WindowMessage::MouseMoved(position.y)
        }
        Event::Window(window::Event::Opened { size, .. }) => WindowMessage::Opened(window, size),
        Event::Window(window::Event::Resized(size)) => WindowMessage::Resized(size),
        Event::Window(window::Event::Rescaled(_)) => WindowMessage::Rescaled(window),
        Event::Window(window::Event::FileHovered(_)) => WindowMessage::FileHovered,
        Event::Window(window::Event::FilesHoveredLeft) => WindowMessage::FileHoverLeft,
        Event::Window(window::Event::FileDropped(path)) => WindowMessage::FileDropped(path),
        Event::Window(window::Event::Focused) => WindowMessage::Focused(true),
        Event::Window(window::Event::Unfocused) => WindowMessage::Focused(false),
        _ => return None,
    };
    Some(message.into())
}

#[cfg(test)]
mod tests;
