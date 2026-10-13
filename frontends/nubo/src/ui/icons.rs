//! Icons from the Lucide icon font (<https://lucide.dev>, ISC License),
//! drawn as text so they stay sharp at any size and take the text color.

use iced::Font;
use iced::widget::{Text, text};
pub use lucide_icons::Icon;

/// The bytes of the bundled Lucide font, loaded when the app starts.
pub const FONT_BYTES: &[u8] = lucide_icons::LUCIDE_FONT_BYTES;

/// The Lucide font, by the family name inside the font file.
pub const FONT: Font = Font::with_name("lucide");

/// The usual icon size, matching the body text.
pub const SIZE: f32 = 18.0;

/// The icon as a text widget of the usual size.
pub fn icon<'a>(icon: Icon) -> Text<'a> {
    text(char::from(icon).to_string())
        .font(FONT)
        .size(SIZE)
        .line_height(1.0)
}
