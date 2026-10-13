//! The controls the player can change: which keyboard key and which
//! controller button press each Game Boy button or app shortcut. Each action
//! has at most one key and one controller button, and each key or button
//! does at most one thing. The controls are saved with the other settings
//! in `settings.toml` (see `settings.rs`).

use iced::keyboard::key::Code;
use nubo_core::{ControllerButton, ControllerMapping, Key};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Where the player presses buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    Keyboard,
    Controller,
}

impl Device {
    pub const ALL: [Device; 2] = [Device::Keyboard, Device::Controller];
}

impl fmt::Display for Device {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Device::Keyboard => "Keyboard",
            Device::Controller => "Controller",
        })
    }
}

/// Something a key or controller button can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    Start,
    Select,
    /// Opens or closes the pause menu. On the keyboard Esc always does this.
    PauseMenu,
    /// Runs the game faster while held.
    FastForward,
    QuickSave,
    QuickLoad,
    Fullscreen,
    Screenshot,
}

impl Action {
    /// The Game Boy's buttons, in the order the settings list them.
    pub const GAME_BOY: [Action; 8] = [
        Action::Up,
        Action::Down,
        Action::Left,
        Action::Right,
        Action::A,
        Action::B,
        Action::Start,
        Action::Select,
    ];

    /// The shortcuts that can go on a keyboard key. The pause menu isn't one,
    /// because Esc always opens it.
    pub const KEYBOARD_SHORTCUTS: [Action; 5] = [
        Action::FastForward,
        Action::QuickSave,
        Action::QuickLoad,
        Action::Fullscreen,
        Action::Screenshot,
    ];

    /// The shortcuts that can go on a controller button.
    pub const CONTROLLER_SHORTCUTS: [Action; 6] = [
        Action::PauseMenu,
        Action::FastForward,
        Action::QuickSave,
        Action::QuickLoad,
        Action::Fullscreen,
        Action::Screenshot,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Action::Up => "Up",
            Action::Down => "Down",
            Action::Left => "Left",
            Action::Right => "Right",
            Action::A => "A",
            Action::B => "B",
            Action::Start => "Start",
            Action::Select => "Select",
            Action::PauseMenu => "Pause menu",
            Action::FastForward => "Fast forward (hold)",
            Action::QuickSave => "Quick save",
            Action::QuickLoad => "Quick load",
            Action::Fullscreen => "Fullscreen",
            Action::Screenshot => "Screenshot",
        }
    }

    /// Whether this action also moves through menus: the D-pad moves, A and
    /// Start pick, B steps back, and Select clears a setting.
    pub fn navigates(self) -> bool {
        matches!(
            self,
            Action::Up
                | Action::Down
                | Action::Left
                | Action::Right
                | Action::A
                | Action::B
                | Action::Start
                | Action::Select
        )
    }

    pub fn is_direction(self) -> bool {
        matches!(
            self,
            Action::Up | Action::Down | Action::Left | Action::Right
        )
    }

    /// The direction action for a D-pad key, such as one the stick pushes.
    pub fn for_direction(key: Key) -> Option<Action> {
        match key {
            Key::Up => Some(Action::Up),
            Key::Down => Some(Action::Down),
            Key::Left => Some(Action::Left),
            Key::Right => Some(Action::Right),
            _ => None,
        }
    }

    /// The Game Boy button this action presses, if it is one.
    pub fn game_boy_key(self) -> Option<Key> {
        match self {
            Action::Up => Some(Key::Up),
            Action::Down => Some(Key::Down),
            Action::Left => Some(Key::Left),
            Action::Right => Some(Key::Right),
            Action::A => Some(Key::A),
            Action::B => Some(Key::B),
            Action::Start => Some(Key::Start),
            Action::Select => Some(Key::Select),
            _ => None,
        }
    }
}

/// Which actions each input (a key or a controller button) does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Bindings<Input> {
    inputs: BTreeMap<Action, Input>,
}

impl<Input: Copy + PartialEq> Bindings<Input> {
    fn from_pairs(pairs: &[(Action, Input)]) -> Self {
        Bindings {
            inputs: pairs.iter().copied().collect(),
        }
    }

    /// The input that does `action`, if any.
    pub fn input(&self, action: Action) -> Option<Input> {
        self.inputs.get(&action).copied()
    }

    /// The action that `input` does, if any.
    pub fn action(&self, input: Input) -> Option<Action> {
        self.inputs
            .iter()
            .find(|&(_, &bound)| bound == input)
            .map(|(&action, _)| action)
    }

    /// Makes `input` do `action`, taking it away from any other action, or
    /// leaves `action` without an input when `input` is `None`.
    pub fn bind(&mut self, action: Action, input: Option<Input>) {
        match input {
            Some(input) => {
                self.inputs.retain(|_, bound| *bound != input);
                self.inputs.insert(action, input);
            }
            None => {
                self.inputs.remove(&action);
            }
        }
    }
}

/// The keyboard and controller bindings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Controls {
    pub keyboard: Bindings<KeyboardKey>,
    pub controller: Bindings<PadButton>,
}

impl Default for Controls {
    fn default() -> Self {
        Controls {
            keyboard: default_keyboard(),
            controller: default_controller(),
        }
    }
}

impl Controls {
    /// The name of the input that does `action` on `device`, if any.
    pub fn input_label(&self, device: Device, action: Action) -> Option<String> {
        match device {
            Device::Keyboard => self.keyboard.input(action).map(|key| key.to_string()),
            Device::Controller => self
                .controller
                .input(action)
                .map(|button| button.to_string()),
        }
    }

    /// Leaves `action` on `device` without an input.
    pub fn clear(&mut self, device: Device, action: Action) {
        match device {
            Device::Keyboard => self.keyboard.bind(action, None),
            Device::Controller => self.controller.bind(action, None),
        }
    }

    /// Puts back the bindings a new user gets on `device`.
    pub fn reset(&mut self, device: Device) {
        match device {
            Device::Keyboard => self.keyboard = default_keyboard(),
            Device::Controller => self.controller = default_controller(),
        }
    }

    /// The core's controller mapping for the Game Boy buttons. Shortcuts
    /// aren't part of it; the app handles those itself.
    pub fn controller_mapping(&self) -> ControllerMapping {
        let mut mapping = ControllerMapping::empty();

        for action in Action::GAME_BOY {
            if let (Some(button), Some(key)) =
                (self.controller.input(action), action.game_boy_key())
            {
                mapping.bind(button.0, key);
            }
        }

        mapping
    }
}

/// The keys a new user gets.
pub fn default_keyboard() -> Bindings<KeyboardKey> {
    Bindings::from_pairs(&[
        (Action::Up, KeyboardKey(Code::ArrowUp)),
        (Action::Down, KeyboardKey(Code::ArrowDown)),
        (Action::Left, KeyboardKey(Code::ArrowLeft)),
        (Action::Right, KeyboardKey(Code::ArrowRight)),
        (Action::A, KeyboardKey(Code::KeyZ)),
        (Action::B, KeyboardKey(Code::KeyX)),
        (Action::Start, KeyboardKey(Code::Enter)),
        (Action::Select, KeyboardKey(Code::Space)),
        (Action::FastForward, KeyboardKey(Code::Tab)),
        (Action::QuickSave, KeyboardKey(Code::F5)),
        (Action::QuickLoad, KeyboardKey(Code::F8)),
        (Action::Fullscreen, KeyboardKey(Code::F11)),
        (Action::Screenshot, KeyboardKey(Code::F12)),
    ])
}

/// The controller buttons a new user gets. A and B sit where they are on a
/// Game Boy: A on the right, B below it.
pub fn default_controller() -> Bindings<PadButton> {
    Bindings::from_pairs(&[
        (Action::Up, PadButton(ControllerButton::DPadUp)),
        (Action::Down, PadButton(ControllerButton::DPadDown)),
        (Action::Left, PadButton(ControllerButton::DPadLeft)),
        (Action::Right, PadButton(ControllerButton::DPadRight)),
        (Action::A, PadButton(ControllerButton::East)),
        (Action::B, PadButton(ControllerButton::South)),
        (Action::Start, PadButton(ControllerButton::Start)),
        (Action::Select, PadButton(ControllerButton::Select)),
        (Action::PauseMenu, PadButton(ControllerButton::LeftShoulder)),
        (
            Action::FastForward,
            PadButton(ControllerButton::RightShoulder),
        ),
        (Action::QuickLoad, PadButton(ControllerButton::LeftTrigger)),
        (Action::QuickSave, PadButton(ControllerButton::RightTrigger)),
    ])
}

/// A keyboard key, by its position on the keyboard (so the controls stay
/// in place on every layout). Saved by its name, such as "KeyZ".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct KeyboardKey(Code);

impl KeyboardKey {
    /// The key at `code`, if it can be bound. Esc and Backspace can't: they
    /// cancel and clear while picking a key.
    pub fn from_code(code: Code) -> Option<KeyboardKey> {
        KEYS.iter()
            .any(|&(known, _, _)| known == code)
            .then_some(KeyboardKey(code))
    }

    fn entry(self) -> (Code, &'static str, &'static str) {
        *KEYS
            .iter()
            .find(|&&(code, _, _)| code == self.0)
            .unwrap_or(&KEYS[0])
    }
}

impl fmt::Display for KeyboardKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.entry().2)
    }
}

impl From<KeyboardKey> for String {
    fn from(key: KeyboardKey) -> String {
        String::from(key.entry().1)
    }
}

impl TryFrom<String> for KeyboardKey {
    type Error = String;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        KEYS.iter()
            .find(|&&(_, known, _)| known == name)
            .map(|&(code, _, _)| KeyboardKey(code))
            .ok_or_else(|| format!("unknown key {name}"))
    }
}

/// A controller button. Saved by its name, such as "east".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PadButton(pub ControllerButton);

impl PadButton {
    fn names(self) -> (&'static str, &'static str) {
        match self.0 {
            ControllerButton::South => ("south", "Bottom button"),
            ControllerButton::East => ("east", "Right button"),
            ControllerButton::West => ("west", "Left button"),
            ControllerButton::North => ("north", "Top button"),
            ControllerButton::DPadUp => ("dpad_up", "D-pad ↑"),
            ControllerButton::DPadDown => ("dpad_down", "D-pad ↓"),
            ControllerButton::DPadLeft => ("dpad_left", "D-pad ←"),
            ControllerButton::DPadRight => ("dpad_right", "D-pad →"),
            ControllerButton::Start => ("start", "Start"),
            ControllerButton::Select => ("select", "Select"),
            ControllerButton::LeftShoulder => ("left_shoulder", "L1"),
            ControllerButton::RightShoulder => ("right_shoulder", "R1"),
            ControllerButton::LeftTrigger => ("left_trigger", "L2"),
            ControllerButton::RightTrigger => ("right_trigger", "R2"),
            ControllerButton::LeftStick => ("left_stick", "L3"),
            ControllerButton::RightStick => ("right_stick", "R3"),
        }
    }
}

impl fmt::Display for PadButton {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.names().1)
    }
}

impl From<PadButton> for String {
    fn from(button: PadButton) -> String {
        String::from(button.names().0)
    }
}

impl TryFrom<String> for PadButton {
    type Error = String;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        ControllerButton::ALL
            .into_iter()
            .map(PadButton)
            .find(|button| button.names().0 == name)
            .ok_or_else(|| format!("unknown controller button {name}"))
    }
}

/// Defines the table of bindable keys: each key's code, the name it is
/// saved by (the code's name) and the label the settings show.
macro_rules! keys {
    ($($code:ident => $label:expr),* $(,)?) => {
        const KEYS: &[(Code, &str, &str)] = &[$((Code::$code, stringify!($code), $label)),*];
    };
}

keys! {
    ArrowUp => "↑", ArrowDown => "↓", ArrowLeft => "←", ArrowRight => "→",
    KeyA => "A", KeyB => "B", KeyC => "C", KeyD => "D", KeyE => "E", KeyF => "F",
    KeyG => "G", KeyH => "H", KeyI => "I", KeyJ => "J", KeyK => "K", KeyL => "L",
    KeyM => "M", KeyN => "N", KeyO => "O", KeyP => "P", KeyQ => "Q", KeyR => "R",
    KeyS => "S", KeyT => "T", KeyU => "U", KeyV => "V", KeyW => "W", KeyX => "X",
    KeyY => "Y", KeyZ => "Z",
    Digit0 => "0", Digit1 => "1", Digit2 => "2", Digit3 => "3", Digit4 => "4",
    Digit5 => "5", Digit6 => "6", Digit7 => "7", Digit8 => "8", Digit9 => "9",
    F1 => "F1", F2 => "F2", F3 => "F3", F4 => "F4", F5 => "F5", F6 => "F6",
    F7 => "F7", F8 => "F8", F9 => "F9", F10 => "F10", F11 => "F11", F12 => "F12",
    Enter => "Enter", Space => "Space", Tab => "Tab",
    ShiftLeft => "Left Shift", ShiftRight => "Right Shift",
    ControlLeft => "Left Ctrl", ControlRight => "Right Ctrl",
    AltLeft => "Left Alt", AltRight => "Right Alt",
    Comma => ",", Period => ".", Slash => "/", Semicolon => ";", Quote => "'",
    BracketLeft => "[", BracketRight => "]", Backslash => "\\", Minus => "-",
    Equal => "=", Backquote => "`",
    Insert => "Insert", Delete => "Delete", Home => "Home", End => "End",
    PageUp => "Page Up", PageDown => "Page Down",
    Numpad0 => "Num 0", Numpad1 => "Num 1", Numpad2 => "Num 2", Numpad3 => "Num 3",
    Numpad4 => "Num 4", Numpad5 => "Num 5", Numpad6 => "Num 6", Numpad7 => "Num 7",
    Numpad8 => "Num 8", Numpad9 => "Num 9", NumpadEnter => "Num Enter",
    NumpadAdd => "Num +", NumpadSubtract => "Num -", NumpadMultiply => "Num *",
    NumpadDivide => "Num /", NumpadDecimal => "Num .",
}

#[cfg(test)]
mod tests;
