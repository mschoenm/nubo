//! The joypad: the Game Boy's eight buttons and the JOYP register (0xFF00)
//! the game reads them through.
//!
//! The buttons are wired as two groups of four: the directions (Right, Left,
//! Up, Down) and the action buttons (A, B, Select, Start). A game writes JOYP
//! to choose a group and then reads the state of that group's buttons in the
//! low four bits. Everything is active low: a 0 bit means "selected" or
//! "pressed".
//!
//! On a Super Game Boy, JOYP is wired to the Super Nintendo, which can read up
//! to four controllers through a multitap. Once a game asks for more than one
//! player (the `MLT_REQ` command, see `sgb.rs`), deselecting both groups reads
//! the current player's number, and selecting the action buttons and then
//! deselecting them moves on to the next player (SameBoy's `GB_sgb_write` and
//! `GB_update_joyp`).

use crate::controller::{ControllerMapping, ControllerState};
use getset::{CopyGetters, Setters};
use nubo_serializable_derive::Serializable;

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

/// How many players a Super Game Boy's multitap connects.
pub(crate) const SGB_PLAYERS: usize = 4;

/// The joypad hardware.
///
/// Keys pressed on the keyboard and on each controller are tracked separately
/// and combined, so releasing a key on one doesn't release it while another
/// still holds it. The first player has the keyboard and every controller,
/// until a Super Game Boy game asks for more players: then the first
/// controller is the first player's, the second the second player's, and so
/// on, up to four. Frontends without separate controllers for them, such as
/// RetroArch's ports, set the other players' keys with `set_player_keys`.
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

    /// How many controllers a Super Game Boy reads, 1 to 4 (3 only when a
    /// game asks for it, which real hardware doesn't expect). Other models
    /// keep 1.
    sgb_player_count: u8,
    /// The controller a Super Game Boy reads now, counting from 0 for the
    /// first player.
    sgb_current_player: u8,

    /// Keys held on the keyboard, one bit per key as laid out by `key_mask`.
    #[serializable(skip)]
    keyboard_keys: u8,
    /// Keys held on each of the first four controllers, one bit per key as
    /// laid out by `key_mask`.
    #[serializable(skip)]
    controller_keys: [u8; SGB_PLAYERS],
    /// Keys held on any controller after the fourth, which only ever count
    /// for the first player.
    #[serializable(skip)]
    other_controller_keys: u8,
    /// Keys the frontend holds for the second to fourth players itself
    /// (`set_player_keys`).
    #[serializable(skip)]
    player_keys: [u8; SGB_PLAYERS - 1],
    /// What the second to fourth players hold, worked out from the keys
    /// above, one bit per key.
    #[serializable(skip)]
    other_players_pressed: [u8; SGB_PLAYERS - 1],

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
            sgb_player_count: 1,
            sgb_current_player: 0,
            keyboard_keys: 0,
            controller_keys: [0; SGB_PLAYERS],
            other_controller_keys: 0,
            player_keys: [0; SGB_PLAYERS - 1],
            other_players_pressed: [0; SGB_PLAYERS - 1],
            controller_mapping: ControllerMapping::default(),
        }
    }

    /// Handles a write to JOYP. Only the group select bits can be written.
    /// Selecting a group whose buttons are held pulls its lines low, which
    /// requests the interrupt just like pressing a button does.
    pub(super) fn write_byte(&mut self, value: u8) {
        self.write_byte_with_sgb_players(value, None);
    }

    /// Handles a write to JOYP on a Super Game Boy. `requested_players` is
    /// how many players an `MLT_REQ` command finished by this write asks for.
    ///
    /// With two or four players, deselecting the action buttons moves on to
    /// the next player before the command takes effect. Asking for three
    /// players moves on once more, a quirk of the Super Game Boy's program
    /// (SameBoy).
    pub(super) fn write_byte_with_sgb_players(&mut self, value: u8, requested_players: Option<u8>) {
        let previous_lines = self.input_lines();

        let action_buttons_deselected = value & 0x20 != 0 && self.group_select & 0x20 == 0;
        if action_buttons_deselected && self.sgb_player_count & 1 == 0 {
            self.sgb_current_player = self.sgb_current_player.wrapping_add(1);
            self.sgb_current_player &= self.sgb_player_mask();
        }
        if let Some(players) = requested_players {
            self.sgb_player_count = players;
            if players == 3 {
                self.sgb_current_player = self.sgb_current_player.wrapping_add(1);
            }
            self.sgb_current_player &= self.sgb_player_mask();
            self.combine_keys();
        }

        self.group_select = value & 0x30;
        self.request_interrupt_on_falling_lines(previous_lines);
    }

    /// The mask that keeps the current player's number below the player
    /// count: 0, 1, 2 or 3 for one to four players.
    fn sgb_player_mask(&self) -> u8 {
        self.sgb_player_count.wrapping_sub(1) & 0x03
    }

    /// Handles a read of JOYP: the buttons of every selected group combined,
    /// with the top two bits always reading as 1.
    pub(super) fn read_byte(&self) -> u8 {
        0xC0 | self.group_select | self.input_lines()
    }

    /// The four input lines in the low bits of JOYP: a line is 0 while a
    /// button in a selected group is held on it. On a Super Game Boy reading
    /// more than one controller, deselecting both groups reads 0xF minus the
    /// current player's number.
    fn input_lines(&self) -> u8 {
        let player = self.sgb_current_player & 0x03;
        if self.group_select == 0x30 && self.sgb_player_count > 1 {
            return 0x0F - player;
        }
        let (directional_buttons, action_buttons) = match player {
            0 => (self.directional_buttons, self.action_buttons),
            _ => {
                let pressed = self.other_players_pressed[usize::from(player - 1)];
                (!(pressed >> DIRECTIONAL_KEYS_SHIFT) & 0x0F, !pressed & 0x0F)
            }
        };
        let mut lines = 0x0F;
        if self.group_select & 0x10 == 0 {
            lines &= directional_buttons;
        }
        if self.group_select & 0x20 == 0 {
            lines &= action_buttons;
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

    /// Replaces the controllers' held keys with the ones `states` map to,
    /// the first controller's first. Controllers that aren't there hold
    /// nothing.
    pub(super) fn set_controller_states(&mut self, states: &[ControllerState]) {
        let keys = |state: &ControllerState| {
            self.controller_mapping
                .pressed_keys(state)
                .fold(0, |keys, key| keys | key_mask(key))
        };
        let mut controller_keys = [0; SGB_PLAYERS];
        for (held, state) in controller_keys.iter_mut().zip(states) {
            *held = keys(state);
        }
        self.other_controller_keys = states
            .iter()
            .skip(SGB_PLAYERS)
            .fold(0, |held, state| held | keys(state));
        self.controller_keys = controller_keys;
        self.update_buttons();
    }

    /// Replaces the keys a frontend holds for `player` (2 to 4) itself, such
    /// as RetroArch for its second to fourth ports. Other players are
    /// ignored.
    pub(super) fn set_player_keys(&mut self, player: u8, keys: &[Key]) {
        let Some(held) = usize::from(player)
            .checked_sub(2)
            .and_then(|index| self.player_keys.get_mut(index))
        else {
            return;
        };
        *held = keys.iter().fold(0, |held, &key| held | key_mask(key));
        self.update_buttons();
    }

    /// Whether any player holds any button, on the keyboard or a controller.
    pub(super) fn any_button_held(&self) -> bool {
        self.keyboard_keys != 0
            || self.other_controller_keys != 0
            || self.controller_keys.iter().any(|&keys| keys != 0)
            || self.player_keys.iter().any(|&keys| keys != 0)
    }

    /// Gives each player the buttons held now, after a loaded save state
    /// changed how many players a Super Game Boy reads. The buttons held
    /// aren't part of the state, so no interrupt follows.
    pub(super) fn finish_loading_save_state(&mut self) {
        self.combine_keys();
    }

    /// Recomputes the button bits from the keyboard and controller keys, and
    /// requests the joypad interrupt when that pulls an input line low.
    fn update_buttons(&mut self) {
        let previous_lines = self.input_lines();
        self.combine_keys();
        self.request_interrupt_on_falling_lines(previous_lines);
    }

    /// Works out what each player holds: the first player the keyboard and
    /// every controller, or with more than one player the first controller
    /// and those after the fourth; each other player their own controller and
    /// the keys the frontend holds for them.
    fn combine_keys(&mut self) {
        let first_controller = if self.sgb_player_count > 1 {
            self.controller_keys[0] | self.other_controller_keys
        } else {
            self.controller_keys
                .iter()
                .fold(self.other_controller_keys, |held, &keys| held | keys)
        };
        let pressed = self.keyboard_keys | first_controller;
        self.action_buttons = !pressed & 0x0F;
        self.directional_buttons = !(pressed >> DIRECTIONAL_KEYS_SHIFT) & 0x0F;

        for (index, pressed) in self.other_players_pressed.iter_mut().enumerate() {
            *pressed = self.player_keys[index] | self.controller_keys[index + 1];
        }
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
        joypad.set_controller_states(&[controller_pressing(ControllerButton::DPadLeft)]);
        assert_eq!(joypad.directional_buttons(), 0xD);
        assert!(joypad.interrupt());
    }

    #[test]
    fn keyboard_release_keeps_key_held_on_controller() {
        let mut joypad = Joypad::new();
        joypad.set_controller_states(&[controller_pressing(ControllerButton::East)]);
        joypad.press_key(Key::A);
        joypad.release_key(Key::A);
        assert_eq!(joypad.action_buttons(), 0xE);
    }

    #[test]
    fn controller_release_keeps_key_held_on_keyboard() {
        let mut joypad = Joypad::new();
        joypad.press_key(Key::Up);
        joypad.set_controller_states(&[controller_pressing(ControllerButton::DPadUp)]);
        joypad.set_controller_states(&[ControllerState::new()]);
        assert_eq!(joypad.directional_buttons(), 0xB);
    }

    #[test]
    fn already_held_key_does_not_raise_interrupt_again() {
        let mut joypad = Joypad::new();
        joypad.press_key(Key::B);
        joypad.set_interrupt(false);
        joypad.set_controller_states(&[controller_pressing(ControllerButton::South)]);
        assert!(!joypad.interrupt());
    }

    #[test]
    fn controller_mapping_can_be_replaced() {
        let mut joypad = Joypad::new();
        let mut mapping = ControllerMapping::default();
        mapping.bind(ControllerButton::North, Key::Select);
        joypad.set_controller_mapping(mapping);
        joypad.set_controller_states(&[controller_pressing(ControllerButton::North)]);
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

    #[test]
    fn a_super_game_boy_with_two_players_reads_the_current_player() {
        let mut joypad = Joypad::new();
        joypad.write_byte_with_sgb_players(0x30, Some(2));
        assert_eq!(joypad.read_byte() & 0x0F, 0x0F);

        joypad.write_byte(0x10);
        joypad.write_byte(0x30);
        assert_eq!(joypad.read_byte() & 0x0F, 0x0E);

        joypad.write_byte(0x10);
        joypad.write_byte(0x30);
        assert_eq!(joypad.read_byte() & 0x0F, 0x0F);
    }

    #[test]
    fn other_players_buttons_read_as_released() {
        let mut joypad = Joypad::new();
        joypad.write_byte_with_sgb_players(0x30, Some(4));
        joypad.press_key(Key::A);
        joypad.write_byte(0x10);
        assert_eq!(joypad.read_byte() & 0x0F, 0x0E);
        joypad.write_byte(0x30);
        joypad.write_byte(0x10);
        assert_eq!(joypad.read_byte() & 0x0F, 0x0F);
    }

    #[test]
    fn asking_for_three_players_moves_on_a_player() {
        let mut joypad = Joypad::new();
        joypad.write_byte_with_sgb_players(0x30, Some(2));
        joypad.write_byte(0x10);
        joypad.write_byte(0x30);
        joypad.write_byte_with_sgb_players(0x30, Some(3));
        assert_eq!(joypad.read_byte() & 0x0F, 0x0D);
    }

    #[test]
    fn going_back_to_one_player_reads_the_first_players_buttons() {
        let mut joypad = Joypad::new();
        joypad.write_byte_with_sgb_players(0x30, Some(2));
        joypad.write_byte(0x10);
        joypad.write_byte_with_sgb_players(0x30, Some(1));
        joypad.press_key(Key::B);
        joypad.write_byte(0x10);
        assert_eq!(joypad.read_byte() & 0x0F, 0x0D);
    }

    #[test]
    fn with_more_players_each_controller_plays_its_own() {
        let mut joypad = Joypad::new();
        joypad.set_controller_states(&[
            ControllerState::new(),
            controller_pressing(ControllerButton::East),
        ]);
        joypad.write_byte(0x10);
        assert_eq!(
            joypad.read_byte() & 0x0F,
            0x0E,
            "one player: any controller"
        );

        joypad.write_byte_with_sgb_players(0x30, Some(2));
        joypad.write_byte(0x10);
        assert_eq!(
            joypad.read_byte() & 0x0F,
            0x0F,
            "the first player's controller"
        );
        joypad.write_byte(0x30);
        joypad.write_byte(0x10);
        assert_eq!(
            joypad.read_byte() & 0x0F,
            0x0E,
            "the second player's controller"
        );
    }

    #[test]
    fn a_frontend_can_hold_keys_for_other_players() {
        let mut joypad = Joypad::new();
        joypad.write_byte_with_sgb_players(0x30, Some(4));
        joypad.set_player_keys(3, &[Key::Start]);
        joypad.set_player_keys(5, &[Key::A]);
        for _ in 0..2 {
            joypad.write_byte(0x10);
            joypad.write_byte(0x30);
        }
        joypad.write_byte(0x10);
        assert_eq!(
            joypad.read_byte() & 0x0F,
            0x07,
            "the third player holds Start"
        );
    }

    #[test]
    fn a_held_button_of_the_next_player_requests_the_interrupt() {
        let mut joypad = Joypad::new();
        joypad.write_byte_with_sgb_players(0x10, Some(2));
        joypad.set_player_keys(2, &[Key::B]);
        joypad.set_interrupt(false);
        joypad.write_byte(0x30);
        joypad.write_byte(0x10);
        assert!(joypad.interrupt());
    }

    #[test]
    fn a_loaded_state_with_more_players_gives_each_controller_its_player() {
        use crate::serializable::Serializable;

        let mut saved = Joypad::new();
        saved.write_byte_with_sgb_players(0x30, Some(2));
        let mut bytes = Vec::new();
        saved.serialize(&mut bytes).unwrap();

        let mut joypad = Joypad::new();
        joypad.set_controller_states(&[
            ControllerState::new(),
            controller_pressing(ControllerButton::East),
        ]);
        joypad.deserialize(&mut bytes.as_slice()).unwrap();
        joypad.finish_loading_save_state();
        joypad.write_byte(0x10);
        assert_eq!(
            joypad.read_byte() & 0x0F,
            0x0F,
            "the second controller isn't the first player's"
        );
    }

    #[test]
    fn controllers_after_the_fourth_keep_playing_the_first_player() {
        let mut joypad = Joypad::new();
        let mut states = vec![ControllerState::new(); 4];
        states.push(controller_pressing(ControllerButton::East));
        joypad.set_controller_states(&states);
        joypad.write_byte_with_sgb_players(0x10, Some(4));
        assert_eq!(joypad.read_byte() & 0x0F, 0x0E);
    }
}
