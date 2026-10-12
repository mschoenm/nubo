//! The joypad: the Game Boy's eight buttons and the JOYP register (0xFF00)
//! the game reads them through.
//!
//! The buttons are wired as two groups of four: the directions (Right, Left,
//! Up, Down) and the action buttons (A, B, Select, Start). A game writes JOYP
//! to choose a group and then reads the state of that group's buttons in the
//! low four bits. Everything is active low: a 0 bit means "selected" or
//! "pressed".

use crate::controller::{ControllerMapping, ControllerState};
use getset::{CopyGetters, Setters};
use serializable_derive::Serializable;

/// A Game Boy button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Down on the D-pad.
    Down,
    /// Up on the D-pad.
    Up,
    /// Left on the D-pad.
    Left,
    /// Right on the D-pad.
    Right,
    /// The Start button.
    Start,
    /// The Select button.
    Select,
    /// The B button.
    B,
    /// The A button.
    A,
}

/// The joypad hardware.
///
/// Keys pressed on the keyboard and on a controller are tracked separately
/// and combined, so releasing a key on one doesn't release it while the other
/// still holds it.
///
/// Save states keep what the game set (the group select bits) and the
/// pending interrupt, but not which buttons are held: those belong to the
/// player rather than the console, so loading a state keeps the buttons held
/// now.
#[derive(Clone, Debug, CopyGetters, Setters, Serializable)]
pub struct Joypad {
    /// Bits 4 and 5 of JOYP. Bit 4 low selects the directions, bit 5 low
    /// selects the action buttons.
    #[getset(get_copy = "pub(super)", set = "pub(super)")]
    group_select: u8,

    /// Start, Select, B and A in bits 3 to 0, with 0 meaning pressed.
    #[getset(get_copy = "pub(super)", set = "pub(super)")]
    #[serializable(skip)]
    action_buttons: u8,

    /// Down, Up, Left and Right in bits 3 to 0, with 0 meaning pressed.
    #[getset(get_copy = "pub(super)", set = "pub(super)")]
    #[serializable(skip)]
    directional_buttons: u8,

    /// Set when one of the four JOYP input lines goes from 1 to 0, to request
    /// the joypad interrupt.
    #[getset(get_copy = "pub(super)", set = "pub(super)")]
    interrupt: bool,

    /// Keys held on the keyboard, one bit per key as laid out by `key_mask`.
    #[serializable(skip)]
    keyboard_keys: u8,
    /// Keys held on a controller, one bit per key as laid out by `key_mask`.
    #[serializable(skip)]
    controller_keys: u8,

    /// Which controller buttons act as which Game Boy buttons, a setting of
    /// the player's.
    #[getset(set = "pub(super)")]
    #[serializable(skip)]
    controller_mapping: ControllerMapping,
}

const DOWN_BIT: u8 = 3;
const UP_BIT: u8 = 2;
const LEFT_BIT: u8 = 1;
const RIGHT_BIT: u8 = 0;

const START_BIT: u8 = 3;
const SELECT_BIT: u8 = 2;
const B_BIT: u8 = 1;
const A_BIT: u8 = 0;

const DIRECTIONAL_KEYS_SHIFT: u8 = 4;

/// The bit for `key` in a byte that holds all eight keys: the action buttons
/// in the low four bits and the directions in the high four, 1 meaning held.
fn key_mask(key: Key) -> u8 {
    match key {
        Key::A => 1 << A_BIT,
        Key::B => 1 << B_BIT,
        Key::Select => 1 << SELECT_BIT,
        Key::Start => 1 << START_BIT,
        Key::Right => 1 << (DIRECTIONAL_KEYS_SHIFT + RIGHT_BIT),
        Key::Left => 1 << (DIRECTIONAL_KEYS_SHIFT + LEFT_BIT),
        Key::Up => 1 << (DIRECTIONAL_KEYS_SHIFT + UP_BIT),
        Key::Down => 1 << (DIRECTIONAL_KEYS_SHIFT + DOWN_BIT),
    }
}

impl Joypad {
    pub(super) fn new() -> Self {
        Joypad {
            group_select: 0x0,
            action_buttons: 0xF,
            directional_buttons: 0xF,
            interrupt: false,
            keyboard_keys: 0,
            controller_keys: 0,
            controller_mapping: ControllerMapping::default(),
        }
    }

    /// Handles a write to JOYP. Only the group select bits can be written.
    /// Selecting a group whose buttons are held pulls its lines low, which
    /// requests the interrupt just like pressing a button does.
    pub(super) fn write_byte(&mut self, value: u8) {
        let previous_lines = self.input_lines();
        self.group_select = value & 0x30;
        self.request_interrupt_on_falling_lines(previous_lines);
    }

    /// Handles a read of JOYP: the buttons of every selected group combined,
    /// with the top two bits always reading as 1.
    pub(super) fn read_byte(&self) -> u8 {
        0xC0 | self.group_select | self.input_lines()
    }

    /// The four input lines in the low bits of JOYP: a line is 0 while a
    /// button in a selected group is held on it.
    fn input_lines(&self) -> u8 {
        let mut lines = 0x0F;
        if self.group_select & 0x10 == 0 {
            lines &= self.directional_buttons;
        }
        if self.group_select & 0x20 == 0 {
            lines &= self.action_buttons;
        }
        lines & 0x0F
    }

    /// Whether a held button in a selected group pulls an input line low,
    /// which keeps STOP from stopping the CPU and wakes it from STOP mode.
    pub(super) fn any_line_low(&self) -> bool {
        self.input_lines() != 0x0F
    }

    /// Requests the joypad interrupt if any input line went from 1 to 0
    /// since it read `previous_lines`. Buttons in groups the game hasn't
    /// selected don't reach the lines, so they can't request it.
    fn request_interrupt_on_falling_lines(&mut self, previous_lines: u8) {
        if previous_lines & !self.input_lines() != 0 {
            self.interrupt = true;
        }
    }

    pub(super) fn press_key(&mut self, key: Key) {
        self.keyboard_keys |= key_mask(key);
        self.update_buttons();
    }

    pub(super) fn release_key(&mut self, key: Key) {
        self.keyboard_keys &= !key_mask(key);
        self.update_buttons();
    }

    /// Replaces the controller's held keys with the ones `state` maps to.
    pub(super) fn set_controller_state(&mut self, state: &ControllerState) {
        self.controller_keys = self
            .controller_mapping
            .pressed_keys(state)
            .fold(0, |keys, key| keys | key_mask(key));
        self.update_buttons();
    }

    /// Recomputes the button bits from the keyboard and controller keys, and
    /// requests the joypad interrupt when that pulls an input line low.
    fn update_buttons(&mut self) {
        let previous_lines = self.input_lines();
        let pressed = self.keyboard_keys | self.controller_keys;

        self.action_buttons = !pressed & 0x0F;
        self.directional_buttons = !(pressed >> DIRECTIONAL_KEYS_SHIFT) & 0x0F;

        self.request_interrupt_on_falling_lines(previous_lines);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::ControllerButton;

    #[test]
    fn reads_from_directional_keys() {
        let mut joypad = Joypad::new();
        joypad.set_group_select(0x20);
        joypad.set_directional_buttons(0x4);
        joypad.set_action_buttons(0x2);
        let result = joypad.read_byte();
        assert_eq!(result, 0xE4);
    }

    #[test]
    fn reads_from_select_keys() {
        let mut joypad = Joypad::new();
        joypad.set_group_select(0x10);
        joypad.set_directional_buttons(0x4);
        joypad.set_action_buttons(0x2);
        let result = joypad.read_byte();
        assert_eq!(result, 0xD2);
    }

    #[test]
    fn writes_to_joyp() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0x4);
        joypad.set_action_buttons(0x2);
        joypad.write_byte(0x20);
        assert_eq!(joypad.group_select(), 0x20);
    }

    #[test]
    fn stores_down_key_press() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_action_buttons(0xF);
        joypad.press_key(Key::Down);
        assert_eq!(joypad.directional_buttons(), 0x7);
    }

    #[test]
    fn stores_down_key_release() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0x7);
        joypad.set_action_buttons(0xF);
        joypad.release_key(Key::Down);
        assert_eq!(joypad.directional_buttons(), 0xF);
    }

    #[test]
    fn stores_up_key_press() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_action_buttons(0xF);
        joypad.press_key(Key::Up);
        assert_eq!(joypad.directional_buttons(), 0xB);
    }

    #[test]
    fn stores_up_key_release() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xB);
        joypad.set_action_buttons(0xF);
        joypad.release_key(Key::Up);
        assert_eq!(joypad.directional_buttons(), 0xF);
    }

    #[test]
    fn stores_left_key_press() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_action_buttons(0xF);
        joypad.press_key(Key::Left);
        assert_eq!(joypad.directional_buttons(), 0xD);
    }

    #[test]
    fn stores_left_key_release() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xD);
        joypad.set_action_buttons(0xF);
        joypad.release_key(Key::Left);
        assert_eq!(joypad.directional_buttons(), 0xF);
    }

    #[test]
    fn stores_right_key_press() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_action_buttons(0xF);
        joypad.press_key(Key::Right);
        assert_eq!(joypad.directional_buttons(), 0xE);
    }

    #[test]
    fn stores_right_key_release() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xE);
        joypad.set_action_buttons(0xF);
        joypad.release_key(Key::Right);
        assert_eq!(joypad.directional_buttons(), 0xF);
    }

    #[test]
    fn stores_start_key_press() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_action_buttons(0xF);
        joypad.press_key(Key::Start);
        assert_eq!(joypad.action_buttons(), 0x7);
    }

    #[test]
    fn stores_start_key_release() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_action_buttons(0x7);
        joypad.release_key(Key::Start);
        assert_eq!(joypad.action_buttons(), 0xF);
    }

    #[test]
    fn stores_select_key_press() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_action_buttons(0xF);
        joypad.press_key(Key::Select);
        assert_eq!(joypad.action_buttons(), 0xB);
    }

    #[test]
    fn stores_select_key_release() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_action_buttons(0xB);
        joypad.release_key(Key::Select);
        assert_eq!(joypad.action_buttons(), 0xF);
    }

    #[test]
    fn stores_b_key_press() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_action_buttons(0xF);
        joypad.press_key(Key::B);
        assert_eq!(joypad.action_buttons(), 0xD);
    }

    #[test]
    fn stores_b_key_release() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_action_buttons(0xD);
        joypad.release_key(Key::B);
        assert_eq!(joypad.action_buttons(), 0xF);
    }

    #[test]
    fn stores_a_key_press() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_action_buttons(0xF);
        joypad.press_key(Key::A);
        assert_eq!(joypad.action_buttons(), 0xE);
    }

    #[test]
    fn stores_a_key_release() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_action_buttons(0xE);
        joypad.release_key(Key::A);
        assert_eq!(joypad.action_buttons(), 0xF);
    }

    fn controller_pressing(button: ControllerButton) -> ControllerState {
        let mut state = ControllerState::new();
        state.set_button(button, true);
        state
    }

    #[test]
    fn key_press_clears_matching_bit_and_raises_interrupt() {
        let mut joypad = Joypad::new();
        joypad.press_key(Key::Start);
        assert_eq!(joypad.action_buttons(), 0x7);
        assert_eq!(joypad.directional_buttons(), 0xF);
        assert!(joypad.interrupt());
    }

    #[test]
    fn controller_press_clears_mapped_bit() {
        let mut joypad = Joypad::new();
        joypad.set_controller_state(&controller_pressing(ControllerButton::DPadLeft));
        assert_eq!(joypad.directional_buttons(), 0xD);
        assert!(joypad.interrupt());
    }

    #[test]
    fn keyboard_release_keeps_key_held_on_controller() {
        let mut joypad = Joypad::new();
        joypad.set_controller_state(&controller_pressing(ControllerButton::East));
        joypad.press_key(Key::A);
        joypad.release_key(Key::A);
        assert_eq!(joypad.action_buttons(), 0xE);
    }

    #[test]
    fn controller_release_keeps_key_held_on_keyboard() {
        let mut joypad = Joypad::new();
        joypad.press_key(Key::Up);
        joypad.set_controller_state(&controller_pressing(ControllerButton::DPadUp));
        joypad.set_controller_state(&ControllerState::new());
        assert_eq!(joypad.directional_buttons(), 0xB);
    }

    #[test]
    fn already_held_key_does_not_raise_interrupt_again() {
        let mut joypad = Joypad::new();
        joypad.press_key(Key::B);
        joypad.set_interrupt(false);
        joypad.set_controller_state(&controller_pressing(ControllerButton::South));
        assert!(!joypad.interrupt());
    }

    #[test]
    fn controller_mapping_can_be_replaced() {
        let mut joypad = Joypad::new();
        let mut mapping = ControllerMapping::default();
        mapping.bind(ControllerButton::North, Key::Select);
        joypad.set_controller_mapping(mapping);
        joypad.set_controller_state(&controller_pressing(ControllerButton::North));
        assert_eq!(joypad.action_buttons(), 0xB);
    }

    #[test]
    fn pressing_a_button_in_an_unselected_group_does_not_raise_interrupt() {
        let mut joypad = Joypad::new();
        joypad.write_byte(0x20);
        joypad.press_key(Key::A);
        assert!(!joypad.interrupt());
    }

    #[test]
    fn pressing_a_button_with_no_group_selected_does_not_raise_interrupt() {
        let mut joypad = Joypad::new();
        joypad.write_byte(0x30);
        joypad.press_key(Key::Down);
        assert!(!joypad.interrupt());
    }

    #[test]
    fn selecting_a_group_with_a_held_button_raises_interrupt() {
        let mut joypad = Joypad::new();
        joypad.write_byte(0x20);
        joypad.press_key(Key::A);
        joypad.write_byte(0x10);
        assert!(joypad.interrupt());
    }

    #[test]
    fn pressing_a_button_on_a_line_that_is_already_low_does_not_raise_interrupt() {
        let mut joypad = Joypad::new();
        joypad.press_key(Key::A);
        joypad.set_interrupt(false);
        joypad.press_key(Key::Right);
        assert!(!joypad.interrupt());
    }
}
