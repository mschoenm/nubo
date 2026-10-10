use crate::app::Message;
use iced::keyboard::key::{Code, Physical};
use iced::keyboard::Event;
use retroboy_core::emulator::Key;

fn map_key(physical_key: Physical) -> Option<Key> {
    match physical_key {
        Physical::Code(Code::ArrowUp) => Some(Key::Up),
        Physical::Code(Code::ArrowDown) => Some(Key::Down),
        Physical::Code(Code::ArrowLeft) => Some(Key::Left),
        Physical::Code(Code::ArrowRight) => Some(Key::Right),
        Physical::Code(Code::Enter) => Some(Key::Start),
        Physical::Code(Code::Space) => Some(Key::Select),
        Physical::Code(Code::KeyX) => Some(Key::B),
        Physical::Code(Code::KeyZ) => Some(Key::A),
        _ => None,
    }
}

pub fn to_message(event: Event) -> Option<Message> {
    match event {
        Event::KeyPressed {
            physical_key,
            repeat: false,
            ..
        } => map_key(physical_key).map(Message::KeyPressed),
        Event::KeyReleased { physical_key, .. } => map_key(physical_key).map(Message::KeyReleased),
        _ => None,
    }
}
