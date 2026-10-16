//! The controls the player can change: which keyboard key and which
//! controller button press each Game Boy button or app shortcut. Each action
//! has at most one key and one controller button, and each key or button
//! does at most one thing. The controls are saved with the other settings
//! in `settings.toml` (see [`crate::settings`]).
//!
//! The menus don't follow these bindings: they always take the same keys
//! and buttons ([`MenuButtons::action`] on a controller, where the player
//! only picks which of two face buttons picks and which goes back), so no
//! remapping can leave the app impossible to control. To get into the
//! menus while a game runs, the keyboard has Esc, which can't be bound, and
//! the controller has the pause menu's button, which can be moved but never
//! cleared (see [`Controls::bind_button`]).

use nubo_core::{ControllerButton, ControllerMapping, Key};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Where the player presses buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    /// The computer's keyboard.
    Keyboard,
    /// A game controller (gamepad).
    Controller,
}

impl Device {
    /// Both devices, in the order the settings show them.
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
    /// Up on the Game Boy's D-pad (the cross-shaped direction pad).
    Up,
    /// Down on the D-pad.
    Down,
    /// Left on the D-pad.
    Left,
    /// Right on the D-pad.
    Right,
    /// The Game Boy's A button.
    A,
    /// The Game Boy's B button.
    B,
    /// The Game Boy's Start button.
    Start,
    /// The Game Boy's Select button.
    Select,
    /// Opens or closes the pause menu. On the keyboard Esc always does this;
    /// on a controller it always has a button.
    PauseMenu,
    /// Runs the game faster while held.
    FastForward,
    /// Saves the game's state in the quick save slot.
    QuickSave,
    /// Loads the state in the quick save slot.
    QuickLoad,
    /// Turns fullscreen on or off.
    Fullscreen,
    /// Saves a picture of the screen.
    Screenshot,
    /// Goes back through the game's recent past while held.
    Rewind,
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
    pub const KEYBOARD_SHORTCUTS: [Action; 6] = [
        Action::Rewind,
        Action::FastForward,
        Action::QuickSave,
        Action::QuickLoad,
        Action::Fullscreen,
        Action::Screenshot,
    ];

    /// The shortcuts that can go on a controller button.
    pub const CONTROLLER_SHORTCUTS: [Action; 7] = [
        Action::PauseMenu,
        Action::Rewind,
        Action::FastForward,
        Action::QuickSave,
        Action::QuickLoad,
        Action::Fullscreen,
        Action::Screenshot,
    ];

    /// What the settings call the action, such as "Quick save".
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
            Action::Rewind => "Rewind (hold)",
        }
    }

    /// Whether this is one of the D-pad's four directions.
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
    /// Which key does each action.
    pub keyboard: Bindings<KeyCode>,
    /// Which controller button does each action.
    pub controller: Bindings<PadButton>,
    /// Which face buttons pick and go back in the menus. Only saved once it
    /// differs from the default, so files from versions without it write
    /// back unchanged.
    #[serde(skip_serializing_if = "is_default_menu_buttons")]
    pub menu_buttons: MenuButtons,
}

/// Whether `buttons` are the menu buttons a new user gets, which settings
/// leave out.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
fn is_default_menu_buttons(buttons: &MenuButtons) -> bool {
    *buttons == MenuButtons::default()
}

impl Default for Controls {
    fn default() -> Self {
        Controls {
            keyboard: default_keyboard(),
            controller: default_controller(),
            menu_buttons: MenuButtons::default(),
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

    /// Whether `action` on `device` may be left without an input: anything
    /// but the pause menu on a controller, which is the controller's only
    /// way into the menus while a game runs.
    pub fn can_clear(device: Device, action: Action) -> bool {
        !(device == Device::Controller && action == Action::PauseMenu)
    }

    /// Leaves `action` on `device` without an input, unless it must keep
    /// one (see [`Controls::can_clear`]).
    pub fn clear(&mut self, device: Device, action: Action) {
        if !Controls::can_clear(device, action) {
            return;
        }
        match device {
            Device::Keyboard => self.keyboard.bind(action, None),
            Device::Controller => self.controller.bind(action, None),
        }
    }

    /// Makes the controller button `button` do `action`, taking it away
    /// from the action it did before. When that was the pause menu, the
    /// pause menu gets `action`'s old button instead, or a free one if it
    /// had none, so the menu can always be opened.
    pub fn bind_button(&mut self, action: Action, button: PadButton) {
        let old = self.controller.input(action);
        let takes_pause_menu = action != Action::PauseMenu
            && self.controller.action(button) == Some(Action::PauseMenu);
        self.controller.bind(action, Some(button));
        if takes_pause_menu {
            let moved = old.or_else(|| self.free_button());
            self.controller.bind(Action::PauseMenu, moved);
        }
    }

    /// Gives the pause menu a controller button if it has none, such as
    /// in controls saved by a version that let the player clear it: the
    /// first free one, shoulder buttons first. Returns whether it did.
    pub fn give_pause_menu_a_button(&mut self) -> bool {
        if self.controller.input(Action::PauseMenu).is_some() {
            return false;
        }
        let button = self.free_button();
        self.controller.bind(Action::PauseMenu, button);
        button.is_some()
    }

    /// A controller button that does nothing yet, the shoulder buttons and
    /// triggers first. There are more buttons than actions, so there always
    /// is one.
    fn free_button(&self) -> Option<PadButton> {
        const PREFERRED: [ControllerButton; 4] = [
            ControllerButton::LeftShoulder,
            ControllerButton::RightShoulder,
            ControllerButton::LeftTrigger,
            ControllerButton::RightTrigger,
        ];
        PREFERRED
            .into_iter()
            .chain(ControllerButton::ALL)
            .map(PadButton)
            .find(|&button| self.controller.action(button).is_none())
    }

    /// Puts back the bindings a new user gets on `device`, and on a
    /// controller the menu buttons too.
    pub fn reset(&mut self, device: Device) {
        match device {
            Device::Keyboard => self.keyboard = default_keyboard(),
            Device::Controller => {
                self.controller = default_controller();
                self.menu_buttons = MenuButtons::default();
            }
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

/// Which controller face buttons pick and go back in the menus (the
/// library, the pause menu, the settings and dialogs). The rest of the menu
/// buttons are fixed whatever the bindings say: the D-pad moves, Start picks
/// too, and Select opens a game's menu or clears a setting. The left stick
/// moves too (see [`Action::for_direction`]). Buttons are named by where
/// they are, since the letters on them differ: an Xbox controller's A is at
/// the bottom, where a Nintendo controller has its B.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MenuButtons {
    /// The right face button (B on an Xbox controller) picks and the bottom
    /// one (A on Xbox) goes back, as on Nintendo's consoles, and as in Game
    /// Boy games with the default bindings, which put the Game Boy's A on the
    /// right.
    #[default]
    Nintendo,
    /// The bottom face button (A on an Xbox controller) picks and the right
    /// one (B on Xbox) goes back, as on an Xbox or a PlayStation.
    Xbox,
}

impl MenuButtons {
    /// Both layouts, in the order the settings show them.
    pub const ALL: [MenuButtons; 2] = [MenuButtons::Nintendo, MenuButtons::Xbox];

    /// The face button that picks.
    pub fn pick(self) -> ControllerButton {
        match self {
            MenuButtons::Nintendo => ControllerButton::East,
            MenuButtons::Xbox => ControllerButton::South,
        }
    }

    /// The face button that goes back.
    pub fn back(self) -> ControllerButton {
        match self {
            MenuButtons::Nintendo => ControllerButton::South,
            MenuButtons::Xbox => ControllerButton::East,
        }
    }

    /// What a controller button does in the menus: A for picking and B for
    /// going back, as the menus take them, or `None` for a button the menus
    /// leave alone.
    pub fn action(self, button: ControllerButton) -> Option<Action> {
        match button {
            ControllerButton::DPadUp => Some(Action::Up),
            ControllerButton::DPadDown => Some(Action::Down),
            ControllerButton::DPadLeft => Some(Action::Left),
            ControllerButton::DPadRight => Some(Action::Right),
            ControllerButton::Start => Some(Action::Start),
            ControllerButton::Select => Some(Action::Select),
            _ if button == self.pick() => Some(Action::A),
            _ if button == self.back() => Some(Action::B),
            _ => None,
        }
    }
}

impl fmt::Display for MenuButtons {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            MenuButtons::Nintendo => "Nintendo",
            MenuButtons::Xbox => "Xbox",
        })
    }
}

/// The keys a new user gets.
pub fn default_keyboard() -> Bindings<KeyCode> {
    Bindings::from_pairs(&[
        (Action::Up, KeyCode::ArrowUp),
        (Action::Down, KeyCode::ArrowDown),
        (Action::Left, KeyCode::ArrowLeft),
        (Action::Right, KeyCode::ArrowRight),
        (Action::A, KeyCode::KeyZ),
        (Action::B, KeyCode::KeyX),
        (Action::Start, KeyCode::Enter),
        (Action::Select, KeyCode::Space),
        (Action::FastForward, KeyCode::Tab),
        (Action::QuickSave, KeyCode::F5),
        (Action::QuickLoad, KeyCode::F8),
        (Action::Fullscreen, KeyCode::F11),
        (Action::Screenshot, KeyCode::F12),
        (Action::Rewind, KeyCode::KeyR),
    ])
}

/// The controller buttons a new user gets. A and B sit where they are on a
/// Game Boy: A on the right, B below it. The other face buttons are left for
/// the player. Rewind (R1) and fast forward (R2) are held, side by side
/// under the right hand. Quick save and quick load get no button: a stray
/// quick load throws away everything since the last save, and the pause
/// menu (L1) has both.
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
        (Action::Rewind, PadButton(ControllerButton::RightShoulder)),
        (
            Action::FastForward,
            PadButton(ControllerButton::RightTrigger),
        ),
    ])
}

/// The round of new actions [`add_new_actions`] gives keys for, which
/// `Settings::controls_revision` counts up to.
pub const REVISION: u32 = 1;

/// Gives rewind, which came after the player's controls were saved, the key
/// and controller button it came with (R and the left face button, as the
/// controller's defaults were then), if they do nothing yet. Returns whether
/// anything changed. An app calls it once, after which the player's own
/// choice (even no input at all) stays.
pub fn add_new_actions(controls: &mut Controls) -> bool {
    let mut changed = false;
    let key = KeyCode::KeyR;
    if controls.keyboard.input(Action::Rewind).is_none() && controls.keyboard.action(key).is_none()
    {
        controls.keyboard.bind(Action::Rewind, Some(key));
        changed = true;
    }
    let button = PadButton(ControllerButton::West);
    if controls.controller.input(Action::Rewind).is_none()
        && controls.controller.action(button).is_none()
    {
        controls.controller.bind(Action::Rewind, Some(button));
        changed = true;
    }
    changed
}

impl fmt::Display for KeyCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

impl From<KeyCode> for String {
    fn from(key: KeyCode) -> String {
        String::from(key.name())
    }
}

impl TryFrom<String> for KeyCode {
    type Error = String;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        KeyCode::ALL
            .iter()
            .copied()
            .find(|key| key.name() == name)
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

/// Defines [`KeyCode`] with every key that can be bound: each key's name,
/// which is also the name it is saved by, and the label the settings show.
macro_rules! key_codes {
    ($($code:ident => $label:expr),* $(,)?) => {
        /// A keyboard key that can be bound, by its position on the keyboard,
        /// so the controls stay in place on every layout. The keys are named
        /// like the web's `KeyboardEvent.code` (and Iced's key codes), such as
        /// `KeyZ` for the key that has Z on a US keyboard, and saved in
        /// `settings.toml` by that name. Esc and Backspace aren't among them:
        /// they cancel and clear while picking a key. An app maps its UI
        /// toolkit's key codes to these (the Iced app in its `input.rs`).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub enum KeyCode {
            $(
                #[doc = concat!("The key labelled `", $label, "` in the settings.")]
                $code
            ),*
        }

        impl KeyCode {
            /// Every key that can be bound.
            pub const ALL: &[KeyCode] = &[$(KeyCode::$code),*];

            /// The name the key is saved by, such as "KeyZ".
            pub fn name(self) -> &'static str {
                match self {
                    $(KeyCode::$code => stringify!($code)),*
                }
            }

            /// What the settings call the key, such as "Z".
            pub fn label(self) -> &'static str {
                match self {
                    $(KeyCode::$code => $label),*
                }
            }
        }
    };
}

key_codes! {
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
