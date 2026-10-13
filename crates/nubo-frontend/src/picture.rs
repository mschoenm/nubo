//! Pictures an app shows, such as the game screen, a game's title screen or
//! box art, and the pictures of save states, as plain RGBA pixels that any UI
//! toolkit can draw.
//!
//! A toolkit usually wants a picture in a form of its own before it can draw
//! it, such as a texture on the graphics card. [`Picture::drawn`] keeps that
//! form with the picture, so it is made once and not again on every redraw.

use std::any::Any;
use std::fmt;
use std::sync::{Arc, OnceLock};

/// A picture as RGBA pixels: four bytes (red, green, blue and alpha) per
/// pixel, row by row from the top left. Cloning it is cheap: the clones share
/// the pixels and what [`Picture::drawn`] keeps.
#[derive(Clone)]
pub struct Picture {
    pixels: Arc<Pixels>,
    /// What the app made to draw the picture (see [`Picture::drawn`]).
    drawn: Arc<OnceLock<Box<dyn Any + Send + Sync>>>,
}

/// A picture's size and pixels, shared by its clones and by what a UI
/// toolkit made to draw it.
struct Pixels {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Picture {
    /// A picture `width` pixels wide and `height` high, made of `rgba`
    /// (`width × height × 4` bytes).
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Picture {
        debug_assert_eq!(rgba.len(), width as usize * height as usize * 4);
        Picture {
            pixels: Arc::new(Pixels {
                width,
                height,
                rgba,
            }),
            drawn: Arc::new(OnceLock::new()),
        }
    }

    /// How many pixels wide the picture is.
    pub fn width(&self) -> u32 {
        self.pixels.width
    }

    /// How many pixels high the picture is.
    pub fn height(&self) -> u32 {
        self.pixels.height
    }

    /// The RGBA pixels, row by row from the top left.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels.rgba
    }

    /// The pixels as something a UI toolkit can keep without copying them,
    /// such as the bytes of an image it uploads.
    pub fn shared_pixels(&self) -> impl AsRef<[u8]> + Send + Sync + 'static + use<> {
        SharedPixels(self.pixels.clone())
    }

    /// What the app draws this picture with, such as a UI toolkit's image
    /// handle: made by `draw` the first time it is asked for, and kept for as
    /// long as the picture or a clone of it lives. A toolkit that keys its
    /// textures on the handle then uploads the picture only once.
    pub fn drawn<T>(&self, draw: impl Fn(&Picture) -> T) -> T
    where
        T: Any + Clone + Send + Sync,
    {
        let kept = self.drawn.get_or_init(|| Box::new(draw(self)));
        match kept.downcast_ref::<T>() {
            Some(drawn) => drawn.clone(),
            // Something of another type was kept first. An app draws its
            // pictures one way, so this doesn't happen; it is drawn anew.
            None => draw(self),
        }
    }
}

impl fmt::Debug for Picture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Picture({}×{})", self.width(), self.height())
    }
}

/// A picture's pixels, kept by something other than the picture (see
/// [`Picture::shared_pixels`]). It doesn't keep what [`Picture::drawn`] keeps,
/// so a drawn picture that holds its pixels doesn't keep itself alive.
struct SharedPixels(Arc<Pixels>);

impl AsRef<[u8]> for SharedPixels {
    fn as_ref(&self) -> &[u8] {
        &self.0.rgba
    }
}

#[cfg(test)]
mod tests;
