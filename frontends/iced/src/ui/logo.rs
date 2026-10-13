//! The Nubo logo drawn in the app with a canvas, from the shapes in the
//! `nubo-logo` crate: a pixel buddy puffed into a cloud, on a rounded square
//! that is purple elsewhere. Here its background takes the theme's accent
//! color, so it fits both the light and the dark theme.

use iced::widget::canvas::{self, Frame, Geometry, Path};
use iced::{Color, Element, Point, Rectangle, Renderer, Size, Theme, mouse};
use nubo_logo::{DESIGN_SIZE, Part, SHAPES};

/// The logo, `size` pixels wide and high.
pub fn logo<'a, Message: 'a>(size: f32) -> Element<'a, Message> {
    canvas::Canvas::new(Logo).width(size).height(size).into()
}

/// Draws the logo's shapes, scaled to the canvas.
struct Logo;

impl<Message> canvas::Program<Message> for Logo {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let scale = bounds.width.min(bounds.height) / DESIGN_SIZE;

        for shape in SHAPES {
            let path = Path::rounded_rectangle(
                Point::new(shape.x * scale, shape.y * scale),
                Size::new(shape.width * scale, shape.height * scale),
                (shape.radius * scale).into(),
            );
            frame.fill(&path, color(shape.part, theme));
        }

        vec![frame.into_geometry()]
    }
}

/// The color of a part of the logo: the theme's accent for the background,
/// the logo's own colors for the rest.
fn color(part: Part, theme: &Theme) -> Color {
    if part == Part::Background {
        return theme.palette().primary;
    }

    let [red, green, blue, alpha] = part.color();
    Color::from_rgba8(red, green, blue, f32::from(alpha) / 255.0)
}
