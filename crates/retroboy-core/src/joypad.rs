use crate::utils::{reset_bit, set_bit};
use getset::{CopyGetters, Setters};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Down,
    Up,
    Left,
    Right,
    Start,
    Select,
    B,
    A
}

#[derive(Debug, CopyGetters, Setters)]
#[getset(get_copy = "pub(super)", set = "pub(super)")]
pub struct Joypad {
    column: u8,
    select_buttons: u8,
    directional_buttons: u8,
    interrupt: bool
}

const DOWN_BIT: u8 = 3;
const UP_BIT: u8 = 2;
const LEFT_BIT: u8 = 1;
const RIGHT_BIT: u8 = 0;

const START_BIT: u8 = 3;
const SELECT_BIT: u8 = 2;
const B_BIT: u8 = 1;
const A_BIT: u8 = 0;

impl Joypad {
    pub(super) fn new() -> Self {
        Joypad {
            column: 0x0,
            select_buttons: 0xF,
            directional_buttons: 0xF,
            interrupt: false
        }
    }

    pub(super) fn write_byte(&mut self, value: u8) {
        self.column = value & 0x30;
    }

    pub(super) fn read_byte(&self) -> u8 {
        if self.column & 0x20 == 0 {
            0xD0 | (self.select_buttons & 0x0F)
        }
        else if self.column & 0x10 == 0 {
            0xE0 | (self.directional_buttons & 0x0F)
        }
        else {
            0xFF
        }  
    }

    pub(super) fn handle_key_press(&mut self, key: &Key) {
        match key {
            Key::Down => self.directional_buttons = reset_bit(self.directional_buttons, DOWN_BIT),
            Key::Up => self.directional_buttons = reset_bit(self.directional_buttons, UP_BIT),
            Key::Left => self.directional_buttons = reset_bit(self.directional_buttons, LEFT_BIT),
            Key::Right => self.directional_buttons = reset_bit(self.directional_buttons, RIGHT_BIT),
            Key::Start => self.select_buttons = reset_bit(self.select_buttons, START_BIT),
            Key::Select => self.select_buttons = reset_bit(self.select_buttons, SELECT_BIT),
            Key::B => self.select_buttons = reset_bit(self.select_buttons, B_BIT),
            Key::A => self.select_buttons = reset_bit(self.select_buttons, A_BIT),
        }
        self.interrupt = true;
    }

    pub(super) fn handle_key_release(&mut self, key: &Key) {
        match key {
            Key::Down => self.directional_buttons = set_bit(self.directional_buttons, DOWN_BIT),
            Key::Up => self.directional_buttons = set_bit(self.directional_buttons, UP_BIT),
            Key::Left => self.directional_buttons = set_bit(self.directional_buttons, LEFT_BIT),
            Key::Right => self.directional_buttons = set_bit(self.directional_buttons, RIGHT_BIT),
            Key::Start => self.select_buttons = set_bit(self.select_buttons, START_BIT),
            Key::Select => self.select_buttons = set_bit(self.select_buttons, SELECT_BIT),
            Key::B => self.select_buttons = set_bit(self.select_buttons, B_BIT),
            Key::A => self.select_buttons = set_bit(self.select_buttons, A_BIT),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_from_directional_keys() {
        let mut joypad = Joypad::new();
        joypad.set_column(0x20);
        joypad.set_directional_buttons(0x4);
        joypad.set_select_buttons(0x2);
        let result = joypad.read_byte();
        assert_eq!(result, 0xE4);
    }

    #[test]
    fn reads_from_select_keys() {
        let mut joypad = Joypad::new();
        joypad.set_column(0x10);
        joypad.set_directional_buttons(0x4);
        joypad.set_select_buttons(0x2);
        let result = joypad.read_byte();
        assert_eq!(result, 0xD2);
    }

    #[test]
    fn writes_to_joyp() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0x4);
        joypad.set_select_buttons(0x2);
        joypad.write_byte(0x20);
        assert_eq!(joypad.column(), 0x20);
    }

    #[test]
    fn stores_down_key_press() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_select_buttons(0xF);
        joypad.handle_key_press(&Key::Down);
        assert_eq!(joypad.directional_buttons(), 0x7);
    }

    #[test]
    fn stores_down_key_release() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0x7);
        joypad.set_select_buttons(0xF);
        joypad.handle_key_release(&Key::Down);
        assert_eq!(joypad.directional_buttons(), 0xF);
    }

    #[test]
    fn stores_up_key_press() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_select_buttons(0xF);
        joypad.handle_key_press(&Key::Up);
        assert_eq!(joypad.directional_buttons(), 0xB);
    }

    #[test]
    fn stores_up_key_release() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xB);
        joypad.set_select_buttons(0xF);
        joypad.handle_key_release(&Key::Up);
        assert_eq!(joypad.directional_buttons(), 0xF);
    }

    #[test]
    fn stores_left_key_press() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_select_buttons(0xF);
        joypad.handle_key_press(&Key::Left);
        assert_eq!(joypad.directional_buttons(), 0xD);
    }

    #[test]
    fn stores_left_key_release() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xD);
        joypad.set_select_buttons(0xF);
        joypad.handle_key_release(&Key::Left);
        assert_eq!(joypad.directional_buttons(), 0xF);
    }

    #[test]
    fn stores_right_key_press() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_select_buttons(0xF);
        joypad.handle_key_press(&Key::Right);
        assert_eq!(joypad.directional_buttons(), 0xE);
    }

    #[test]
    fn stores_right_key_release() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xE);
        joypad.set_select_buttons(0xF);
        joypad.handle_key_release(&Key::Right);
        assert_eq!(joypad.directional_buttons(), 0xF);
    }

    #[test]
    fn stores_start_key_press() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_select_buttons(0xF);
        joypad.handle_key_press(&Key::Start);
        assert_eq!(joypad.select_buttons(), 0x7);
    }

    #[test]
    fn stores_start_key_release() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_select_buttons(0x7);
        joypad.handle_key_release(&Key::Start);
        assert_eq!(joypad.select_buttons(), 0xF);
    }

    #[test]
    fn stores_select_key_press() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_select_buttons(0xF);
        joypad.handle_key_press(&Key::Select);
        assert_eq!(joypad.select_buttons(), 0xB);
    }

    #[test]
    fn stores_select_key_release() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_select_buttons(0xB);
        joypad.handle_key_release(&Key::Select);
        assert_eq!(joypad.select_buttons(), 0xF);
    }

    #[test]
    fn stores_b_key_press() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_select_buttons(0xF);
        joypad.handle_key_press(&Key::B);
        assert_eq!(joypad.select_buttons(), 0xD);
    }

    #[test]
    fn stores_b_key_release() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_select_buttons(0xD);
        joypad.handle_key_release(&Key::B);
        assert_eq!(joypad.select_buttons(), 0xF);
    }

    #[test]
    fn stores_a_key_press() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_select_buttons(0xF);
        joypad.handle_key_press(&Key::A);
        assert_eq!(joypad.select_buttons(), 0xE);
    }

    #[test]
    fn stores_a_key_release() {
        let mut joypad = Joypad::new();
        joypad.set_directional_buttons(0xF);
        joypad.set_select_buttons(0xE);
        joypad.handle_key_release(&Key::A);
        assert_eq!(joypad.select_buttons(), 0xF);
    }
}
