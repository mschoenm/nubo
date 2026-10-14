//! Pictures an app shows, such as the game screen, a game's title screen or
//! box art, and the pictures of save states, as plain RGBA pixels that any UI
//! toolkit can draw.
//!
//! A toolkit usually wants a picture in a form of its own before it can draw
//! it, such as a texture on the graphics card. [`Picture::drawn`] keeps that
//! form with the picture, so it is made once and not again on every redraw.

use crate::palettes::DmgPalette;
use nubo_core::{Emulator, SGB_FRAME_HEIGHT, SGB_FRAME_WIDTH, SGB_SCREEN_POSITION};
use std::any::Any;
use std::fmt;
use std::sync::{Arc, OnceLock};

/// How many pixels wide the Game Boy screen is.
pub const SCREEN_WIDTH: u32 = 160;
/// How many pixels high the Game Boy screen is.
pub const SCREEN_HEIGHT: u32 = 144;

/// A frame as the emulator finished it, before the player's palette or
/// choices change it: the Game Boy screen, or on a Super Game Boy the TV
/// picture with the border around the screen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frame {
    /// RGBA pixels, row by row from the top left.
    pub pixels: Vec<u8>,
    /// How many pixels wide the frame is.
    pub width: u32,
    /// How many pixels high the frame is.
    pub height: u32,
    /// Where the Game Boy screen's top-left corner is in the frame.
    pub screen_position: (u32, u32),
    /// Whether the Game Boy screen is in the emulator's grays
    /// (`nubo_core::DMG_SHADES`), which a [`DmgPalette`] repaints.
    pub in_dmg_shades: bool,
}

impl Frame {
    /// The emulator's newest frame (see `Emulator::frame_with_border`).
    pub fn latest(emulator: &Emulator) -> Frame {
        let mut frame = Frame::default();
        frame.update(emulator);
        frame
    }

    /// Makes this the emulator's newest frame, reusing its pixels' memory.
    /// Returns whether anything changed.
    pub fn update(&mut self, emulator: &Emulator) -> bool {
        let (pixels, width, height, screen_position) = match emulator.frame_with_border() {
            Some(pixels) => (
                pixels,
                SGB_FRAME_WIDTH,
                SGB_FRAME_HEIGHT,
                SGB_SCREEN_POSITION,
            ),
            None => (emulator.frame_buffer(), SCREEN_WIDTH, SCREEN_HEIGHT, (0, 0)),
        };
        let in_dmg_shades = emulator.frame_in_dmg_shades();
        if self.pixels == pixels && self.width == width && self.in_dmg_shades == in_dmg_shades {
            return false;
        }
        self.pixels.clear();
        self.pixels.extend_from_slice(pixels);
        self.width = width;
        self.height = height;
        self.screen_position = screen_position;
        self.in_dmg_shades = in_dmg_shades;
        true
    }

    /// Whether the frame has a Super Game Boy border around the screen.
    pub fn has_border(&self) -> bool {
        self.width != SCREEN_WIDTH || self.height != SCREEN_HEIGHT
    }

    /// The Game Boy screen alone, 160×144 pixels.
    pub fn screen_pixels(&self) -> Vec<u8> {
        let (left, top) = self.screen_position;
        let row_bytes = SCREEN_WIDTH as usize * 4;
        (0..SCREEN_HEIGHT as usize)
            .flat_map(|row| {
                let start = ((top as usize + row) * self.width as usize + left as usize) * 4;
                &self.pixels[start..start + row_bytes]
            })
            .copied()
            .collect()
    }

    /// The frame as the player sees it: with or without its border, and with
    /// the Game Boy screen in `palette` when it is in the emulator's grays.
    pub fn shown(&self, palette: DmgPalette, with_border: bool) -> Picture {
        let repaint = self.in_dmg_shades && palette != DmgPalette::Gray;
        if with_border || !self.has_border() {
            let mut pixels = self.pixels.clone();
            if repaint {
                let (left, top) = self.screen_position;
                let row_bytes = SCREEN_WIDTH as usize * 4;
                for row in 0..SCREEN_HEIGHT as usize {
                    let start = ((top as usize + row) * self.width as usize + left as usize) * 4;
                    palette.recolor(&mut pixels[start..start + row_bytes]);
                }
            }
            Picture::new(self.width, self.height, pixels)
        } else {
            let mut pixels = self.screen_pixels();
            if repaint {
                palette.recolor(&mut pixels);
            }
            Picture::new(SCREEN_WIDTH, SCREEN_HEIGHT, pixels)
        }
    }
}

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
