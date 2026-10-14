//! Keyboard input: turns key events into messages, with Iced's key codes
//! turned into Nubo's own ([`KeyCode`]), and finds the fixed app shortcuts.
//! Which key presses which Game Boy button is up to the player (see
//! `nubo_frontend::controls`).

use crate::app::{GameMessage, Message, SettingsMessage, WindowMessage};
use iced::keyboard::key::{Code, Named, Physical};
use iced::keyboard::{self, Event, Modifiers};
use nubo_frontend::controls::KeyCode;

/// A key going down or up.
#[derive(Debug, Clone)]
pub struct KeyInput {
    /// The key's position on the keyboard, which the controls go by, or
    /// `None` for a key that can't be bound.
    pub code: Option<KeyCode>,
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

/// Nubo's code for the key at `physical_key`, if it is one that can be
/// bound.
fn code(physical_key: Physical) -> Option<KeyCode> {
    match physical_key {
        Physical::Code(code) => key_code(code),
        Physical::Unidentified(_) => None,
    }
}

/// Nubo's code for Iced's key code `code`, if the key can be bound.
fn key_code(code: Code) -> Option<KeyCode> {
    KEY_CODES
        .iter()
        .find(|&&(known, _)| known == code)
        .map(|&(_, key)| key)
}

/// Defines the table of Iced's key codes and Nubo's codes for the same keys,
/// which have the same names.
macro_rules! key_codes {
    ($($code:ident),* $(,)?) => {
        /// Iced's key code of every key that can be bound, with Nubo's code
        /// for it.
        const KEY_CODES: &[(Code, KeyCode)] = &[$((Code::$code, KeyCode::$code)),*];
    };
}

key_codes! {
    ArrowUp, ArrowDown, ArrowLeft, ArrowRight,
    KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM,
    KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    Enter, Space, Tab, ShiftLeft, ShiftRight, ControlLeft, ControlRight, AltLeft, AltRight,
    Comma, Period, Slash, Semicolon, Quote, BracketLeft, BracketRight, Backslash, Minus,
    Equal, Backquote,
    Insert, Delete, Home, End, PageUp, PageDown,
    Numpad0, Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7, Numpad8,
    Numpad9, NumpadEnter, NumpadAdd, NumpadSubtract, NumpadMultiply, NumpadDivide,
    NumpadDecimal,
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

#[cfg(test)]
mod tests;
