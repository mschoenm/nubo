//! Keyboard input: turns key events into messages, and finds the fixed app
//! shortcuts. Which key presses which Game Boy button is up to the player
//! (see `controls.rs`).

use crate::app::{GameMessage, Message, SettingsMessage, WindowMessage};
use iced::keyboard::key::{Code, Named, Physical};
use iced::keyboard::{self, Event, Modifiers};

/// A key going down or up.
#[derive(Debug, Clone)]
pub struct KeyInput {
    /// The key's position on the keyboard, which the controls go by.
    pub code: Option<Code>,
    /// What the key types, which the app shortcuts go by.
    pub key: keyboard::Key,
    pub modifiers: Modifiers,
    pub pressed: bool,
    /// Whether this is a repeat of a key held down.
    pub repeat: bool,
}

/// Turns a key event into a message.
pub fn to_message(event: &Event) -> Option<Message> {
    match event {
        Event::KeyPressed {
            key,
            modifiers,
            physical_key,
            repeat,
            ..
        } => Some(Message::Key(KeyInput {
            code: code(*physical_key),
            key: key.clone(),
            modifiers: *modifiers,
            pressed: true,
            repeat: *repeat,
        })),
        Event::KeyReleased {
            key,
            modifiers,
            physical_key,
            ..
        } => Some(Message::Key(KeyInput {
            code: code(*physical_key),
            key: key.clone(),
            modifiers: *modifiers,
            pressed: false,
            repeat: false,
        })),
        Event::ModifiersChanged(_) => None,
    }
}

fn code(physical_key: Physical) -> Option<Code> {
    match physical_key {
        Physical::Code(code) => Some(code),
        Physical::Unidentified(_) => None,
    }
}

/// The shortcuts that can't be changed: Esc for the pause menu, and the
/// command key (⌘ on macOS, Ctrl elsewhere) with a letter, like in other
/// apps. Letters go by the character on the key, so they follow the
/// keyboard layout.
pub fn fixed_shortcut(key: &keyboard::Key, modifiers: Modifiers) -> Option<Message> {
    match key.as_ref() {
        keyboard::Key::Named(Named::Escape) => Some(Message::Escape),
        keyboard::Key::Character("f") if modifiers.command() && modifiers.control() => {
            Some(Message::Window(WindowMessage::ToggleFullscreen))
        }
        keyboard::Key::Character("o") if modifiers.command() => {
            Some(Message::Game(GameMessage::OpenRom))
        }
        keyboard::Key::Character(",") if modifiers.command() => {
            Some(Message::Settings(SettingsMessage::Open))
        }
        keyboard::Key::Character("p") if modifiers.command() => {
            Some(Message::Game(GameMessage::TogglePause))
        }
        keyboard::Key::Character("r") if modifiers.command() => {
            Some(Message::Game(GameMessage::Reset))
        }
        _ => None,
    }
}
