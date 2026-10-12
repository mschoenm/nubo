//! Nubo's light and dark color themes and the styles of its widgets.
//! Every style takes its colors from the theme's palette, so each widget
//! looks right in both themes.

use crate::settings::Appearance;
use iced::font::Weight;
use iced::theme::{Mode, Palette};
use iced::widget::{button, container, text, text_input};
use iced::{Background, Border, Color, Font, Shadow, Theme, Vector};
use std::sync::LazyLock;

/// The font for body text. The desktop build uses the system's sans-serif
/// font. The browser has no system fonts, so the browser build bundles Fira
/// Sans and has to ask for it by name.
pub const BODY_FONT: Font = if cfg!(target_arch = "wasm32") {
    Font::with_name("Fira Sans")
} else {
    Font::DEFAULT
};

/// The font for titles and the app's name. The browser build only bundles
/// Fira Sans in its regular weight, so its titles aren't semibold.
pub const HEADING_FONT: Font = if cfg!(target_arch = "wasm32") {
    BODY_FONT
} else {
    Font {
        weight: Weight::Semibold,
        ..Font::DEFAULT
    }
};

const CORNER_RADIUS: f32 = 8.0;
const PANEL_CORNER_RADIUS: f32 = 14.0;

static DARK: LazyLock<Theme> = LazyLock::new(|| {
    Theme::custom(
        "Nubo Dark",
        Palette {
            background: Color::from_rgb8(0x17, 0x18, 0x1f),
            text: Color::from_rgb8(0xe7, 0xe8, 0xef),
            primary: Color::from_rgb8(0x8b, 0x7c, 0xf6),
            success: Color::from_rgb8(0x4a, 0xde, 0x80),
            warning: Color::from_rgb8(0xfb, 0xbf, 0x24),
            danger: Color::from_rgb8(0xf8, 0x71, 0x71),
        },
    )
});

static LIGHT: LazyLock<Theme> = LazyLock::new(|| {
    Theme::custom(
        "Nubo Light",
        Palette {
            background: Color::from_rgb8(0xf7, 0xf7, 0xfa),
            text: Color::from_rgb8(0x1c, 0x1d, 0x25),
            primary: Color::from_rgb8(0x65, 0x52, 0xe0),
            success: Color::from_rgb8(0x16, 0xa3, 0x4a),
            warning: Color::from_rgb8(0xd9, 0x77, 0x06),
            danger: Color::from_rgb8(0xdc, 0x26, 0x26),
        },
    )
});

/// The theme for the user's chosen appearance. `system_mode` is the
/// operating system's light or dark mode; when it is unknown, "System"
/// picks dark.
pub fn for_appearance(appearance: Appearance, system_mode: Mode) -> Theme {
    let dark = match appearance {
        Appearance::System => system_mode != Mode::Light,
        Appearance::Light => false,
        Appearance::Dark => true,
    };

    if dark { DARK.clone() } else { LIGHT.clone() }
}

/// Text in a softer color, for hints and descriptions.
pub fn muted_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(theme.palette().text.scale_alpha(0.62)),
    }
}

/// Text in the accent color.
pub fn accent_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(theme.palette().primary),
    }
}

/// The bar along the top of the window.
pub fn top_bar(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(theme.extended_palette().background.weakest.color.into()),
        ..container::Style::default()
    }
}

/// The one-pixel line under the top bar.
pub fn divider(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(theme.extended_palette().background.strong.color.into()),
        ..container::Style::default()
    }
}

/// The area around the Game Boy screen: nearly black in the dark theme, so
/// the picture stands out, and a soft gray in the light theme.
pub fn screen_area(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    let background = if palette.is_dark {
        Color::from_rgb8(0x0c, 0x0d, 0x11)
    } else {
        palette.background.weak.color
    };

    container::Style {
        background: Some(background.into()),
        ..container::Style::default()
    }
}

/// A raised panel, such as the settings window.
pub fn panel(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();

    container::Style {
        text_color: Some(palette.background.base.text),
        background: Some(palette.background.base.color.into()),
        border: Border::default()
            .rounded(PANEL_CORNER_RADIUS)
            .width(1)
            .color(palette.background.strong.color),
        shadow: Shadow {
            color: Color::BLACK.scale_alpha(0.3),
            offset: Vector::new(0.0, 8.0),
            blur_radius: 28.0,
        },
        snap: true,
    }
}

/// A small floating card, used for notifications and the paused badge.
pub fn floating_card(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();

    container::Style {
        text_color: Some(palette.background.base.text),
        background: Some(palette.background.weakest.color.into()),
        border: Border::default()
            .rounded(10)
            .width(1)
            .color(palette.background.strong.color),
        shadow: Shadow {
            color: Color::BLACK.scale_alpha(0.25),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 16.0,
        },
        snap: true,
    }
}

/// Dims everything behind a dialog.
pub fn backdrop(theme: &Theme) -> container::Style {
    let alpha = if theme.extended_palette().is_dark {
        0.6
    } else {
        0.35
    };

    container::Style {
        background: Some(Color::BLACK.scale_alpha(alpha).into()),
        ..container::Style::default()
    }
}

/// A keyboard key drawn as a small keycap, used to show the controls.
pub fn keycap(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();

    container::Style {
        text_color: Some(palette.background.base.text),
        background: Some(palette.background.weak.color.into()),
        border: Border::default()
            .rounded(6)
            .width(1)
            .color(palette.background.strong.color),
        ..container::Style::default()
    }
}

/// The dark label that appears when hovering an icon button.
pub fn tooltip(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();

    container::Style {
        text_color: Some(palette.background.base.color),
        background: Some(palette.background.base.text.scale_alpha(0.92).into()),
        border: Border::default().rounded(6),
        ..container::Style::default()
    }
}

/// A borderless button that only shows a background when hovered, used for
/// the icons in the top bar.
pub fn icon_button(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let idle = button::Style {
        background: None,
        text_color: palette.background.base.text,
        border: Border::default().rounded(CORNER_RADIUS),
        ..button::Style::default()
    };

    match status {
        button::Status::Active => idle,
        button::Status::Hovered => button::Style {
            background: Some(palette.background.weak.color.into()),
            ..idle
        },
        button::Status::Pressed => button::Style {
            background: Some(palette.background.strong.color.into()),
            ..idle
        },
        button::Status::Disabled => button::Style {
            text_color: idle.text_color.scale_alpha(0.35),
            ..idle
        },
    }
}

/// The filled accent-colored button for the main action on a screen.
pub fn primary_button(theme: &Theme, status: button::Status) -> button::Style {
    let style = button::primary(theme, status);

    button::Style {
        border: style.border.rounded(CORNER_RADIUS),
        ..style
    }
}

/// A quieter button with an outline, for secondary actions.
pub fn secondary_button(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let background = match status {
        button::Status::Hovered => palette.background.weak.color,
        button::Status::Pressed => palette.background.strong.color,
        button::Status::Active | button::Status::Disabled => palette.background.weakest.color,
    };

    button::Style {
        background: Some(Background::Color(background)),
        text_color: palette.background.base.text,
        border: Border::default()
            .rounded(CORNER_RADIUS)
            .width(1)
            .color(palette.background.strong.color),
        ..button::Style::default()
    }
}

/// One option of a segmented control: the selected option is tinted with
/// the accent color, the others look like icon buttons.
pub fn segment(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        if !selected {
            return icon_button(theme, status);
        }

        let palette = theme.extended_palette();

        button::Style {
            background: Some(palette.primary.weak.color.scale_alpha(0.35).into()),
            text_color: palette.background.base.text,
            border: Border::default()
                .rounded(CORNER_RADIUS - 2.0)
                .width(1)
                .color(palette.primary.base.color),
            ..button::Style::default()
        }
    }
}

/// The frame around the options of a segmented control.
pub fn segmented_control(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();

    container::Style {
        background: Some(palette.background.weakest.color.into()),
        border: Border::default()
            .rounded(CORNER_RADIUS)
            .width(1)
            .color(palette.background.strong.color),
        ..container::Style::default()
    }
}

/// A game card in the library: raised slightly, with an accent outline
/// when hovered.
pub fn card(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let (background, border) = match status {
        button::Status::Hovered => (palette.background.weak.color, palette.primary.base.color),
        button::Status::Pressed => (
            palette.background.strong.color,
            palette.primary.strong.color,
        ),
        button::Status::Active | button::Status::Disabled => (
            palette.background.weakest.color,
            palette.background.strong.color,
        ),
    };
    let text_color = if status == button::Status::Disabled {
        palette.background.base.text.scale_alpha(0.45)
    } else {
        palette.background.base.text
    };

    button::Style {
        background: Some(background.into()),
        text_color,
        border: Border::default().rounded(12).width(1).color(border),
        ..button::Style::default()
    }
}

/// Fills the picture area of a game card that has no thumbnail yet.
pub fn thumbnail_placeholder(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(theme.extended_palette().background.weak.color.into()),
        border: Border::default().rounded(6),
        ..container::Style::default()
    }
}

/// The small "GB" or "GBC" label on a game card. Color games get the accent
/// color.
pub fn badge(color_game: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let palette = theme.extended_palette();
        let (background, text) = if color_game {
            (palette.primary.base.color, palette.primary.base.text)
        } else {
            (
                palette.background.strong.color,
                palette.background.strong.text,
            )
        };

        container::Style {
            text_color: Some(text),
            background: Some(background.into()),
            border: Border::default().rounded(4),
            ..container::Style::default()
        }
    }
}

/// A box for messages such as "No games found".
pub fn notice(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();

    container::Style {
        background: Some(palette.background.weakest.color.into()),
        border: Border::default()
            .rounded(12)
            .width(1)
            .color(palette.background.strong.color),
        ..container::Style::default()
    }
}

/// The search field in the top bar.
pub fn search_field(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let style = text_input::default(theme, status);

    text_input::Style {
        border: style.border.rounded(CORNER_RADIUS),
        ..style
    }
}

/// The key or button shown next to an action in the controls settings. It
/// lights up in the accent color while waiting for a new one, and fades when
/// nothing is bound.
pub fn binding_chip(
    capturing: bool,
    empty: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let palette = theme.extended_palette();

        let (background, border) = if capturing {
            (
                palette.primary.weak.color.scale_alpha(0.35),
                palette.primary.base.color,
            )
        } else if status == button::Status::Hovered {
            (palette.background.strong.color, palette.primary.base.color)
        } else {
            (
                palette.background.weak.color,
                palette.background.strong.color,
            )
        };

        let text_color = if empty && !capturing {
            palette.background.base.text.scale_alpha(0.5)
        } else {
            palette.background.base.text
        };

        button::Style {
            background: Some(background.into()),
            text_color,
            border: Border::default().rounded(6).width(1).color(border),
            ..button::Style::default()
        }
    }
}

/// A card that can be highlighted, such as the game or save slot the D-pad
/// is on. A highlighted card looks hovered.
pub fn selectable_card(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let status = if selected && status == button::Status::Active {
            button::Status::Hovered
        } else {
            status
        };
        card(theme, status)
    }
}

/// A row in a list, such as a cheat, outlined in the accent color while the
/// D-pad is on it.
pub fn selectable_row(selected: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let palette = theme.extended_palette();
        let border = if selected {
            palette.primary.base.color
        } else {
            palette.background.strong.color
        };

        container::Style {
            background: Some(palette.background.weakest.color.into()),
            border: Border::default().rounded(10).width(1).color(border),
            ..container::Style::default()
        }
    }
}

/// The thin rounded frame around a palette's color swatches.
pub fn swatch_frame(theme: &Theme) -> container::Style {
    container::Style {
        border: Border::default()
            .rounded(4)
            .width(1)
            .color(theme.extended_palette().background.strong.color),
        ..container::Style::default()
    }
}

/// The dark frame a game's picture sits on in its library card, like the
/// glass around a Game Boy screen.
pub fn card_screen(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    let background = if palette.is_dark {
        Color::from_rgb8(0x0c, 0x0d, 0x11)
    } else {
        Color::from_rgb8(0x2a, 0x2c, 0x36)
    };

    container::Style {
        background: Some(background.into()),
        border: Border::default().rounded(6),
        ..container::Style::default()
    }
}

/// The outline around the setting the D-pad is on. It is transparent
/// otherwise, so the layout doesn't shift when it appears.
pub fn focus_ring(focused: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let color = if focused {
            theme.extended_palette().primary.base.color
        } else {
            Color::TRANSPARENT
        };

        container::Style {
            border: Border::default().rounded(10).width(2).color(color),
            ..container::Style::default()
        }
    }
}
