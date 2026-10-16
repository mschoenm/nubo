//! The Nubo logo, from the shapes in the `nubo-logo` crate: a pixel buddy
//! puffed into a cloud, on a rounded square that is purple elsewhere. Here
//! the square takes the theme's accent color, so it fits both the light and
//! the dark theme.
//!
//! The square is a container's background and the rest a picture drawn once
//! for each size. Drawn as a canvas, the logo's triangles came back on screen
//! in the browser after it was gone (Iced 0.14 with WebGL): a later frame
//! with fewer canvases, such as the library after the welcome or after the
//! settings' About tab, showed the old logo again where it had been. Nothing
//! else in the app draws triangles.

use iced::widget::{container, image};
use iced::{Border, Element, Theme};
use nubo_logo::{DESIGN_SIZE, Part, SHAPES};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, PoisonError};

/// How many pixels the picture has for each pixel of the logo's size, so it
/// stays sharp on high-density screens.
const PIXEL_DENSITY: f32 = 2.0;

/// The logo, `size` pixels wide and high.
pub fn logo<'a, Message: 'a>(size: f32) -> Element<'a, Message> {
    let radius = SHAPES
        .iter()
        .find(|shape| shape.part == Part::Background)
        .map_or(0.0, |shape| shape.radius * size / DESIGN_SIZE);

    container(image(foreground(size)).width(size).height(size))
        .width(size)
        .height(size)
        .style(move |theme: &Theme| container::Style {
            background: Some(theme.palette().primary.into()),
            border: Border {
                radius: radius.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

/// The logo without its square, for `size`: drawn the first time that size
/// is shown, and the same handle after, so it is uploaded to the graphics
/// card only once.
fn foreground(size: f32) -> image::Handle {
    static HANDLES: OnceLock<Mutex<HashMap<u32, image::Handle>>> = OnceLock::new();

    let pixels = (size * PIXEL_DENSITY).round().max(1.0) as u32;
    HANDLES
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(pixels)
        .or_insert_with(|| {
            let rgba = nubo_logo::render_in(pixels, 0.0, |part| match part {
                Part::Background => [0, 0, 0, 0],
                part => part.color(),
            });
            image::Handle::from_rgba(pixels, pixels, rgba)
        })
        .clone()
}
