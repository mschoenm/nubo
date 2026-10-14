//! The Nubo logo: a friendly pixel puffed up into a little cloud ("nubo" is
//! Esperanto for cloud), with two eyes and a sparkle, on a purple rounded
//! square. It is defined once here as a few rounded rectangles, so the app's
//! own top bar, its window icon and the macOS app icon all come from the same
//! shapes and always match.

/// The logo is designed on a square this many units wide.
pub const DESIGN_SIZE: f32 = 64.0;

/// A part of the logo. Each has its own color, which the app may swap for
/// its theme's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// The rounded square everything sits on.
    Background,
    /// The pixel buddy's cloud-shaped body: a wide base with two round
    /// bumps on top.
    Body,
    /// One of the pixel buddy's eyes.
    Eye,
    /// The little square of light above the pixel buddy.
    Sparkle,
}

impl Part {
    /// The part's color in the logo, as RGBA.
    pub fn color(self) -> [u8; 4] {
        match self {
            Part::Background => [0x8B, 0x7C, 0xF6, 0xFF],
            Part::Body => [0xFF, 0xFF, 0xFF, 0xFF],
            Part::Eye => [0x2E, 0x24, 0x70, 0xFF],
            Part::Sparkle => [0xE4, 0xDF, 0xFF, 0xFF],
        }
    }
}

/// A rounded rectangle on the design square, in design units. A square
/// whose radius is half its width is a circle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape {
    /// Which part of the logo the shape is, which sets its color.
    pub part: Part,
    /// The left edge.
    pub x: f32,
    /// The top edge.
    pub y: f32,
    /// The width.
    pub width: f32,
    /// The height.
    pub height: f32,
    /// The radius of the rounded corners.
    pub radius: f32,
}

impl Shape {
    const fn new(part: Part, x: f32, y: f32, width: f32, height: f32, radius: f32) -> Shape {
        Shape {
            part,
            x,
            y,
            width,
            height,
            radius,
        }
    }

    /// Whether the point `(x, y)` (in design units) lies inside the shape,
    /// rounded corners included.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        if x < self.x || y < self.y || x >= self.x + self.width || y >= self.y + self.height {
            return false;
        }

        // Inside the rectangle; only the corner squares need a closer look.
        let nearest_x = x.clamp(self.x + self.radius, self.x + self.width - self.radius);
        let nearest_y = y.clamp(self.y + self.radius, self.y + self.height - self.radius);
        let (dx, dy) = (x - nearest_x, y - nearest_y);
        dx * dx + dy * dy <= self.radius * self.radius
    }
}

/// The logo's shapes, from back to front.
pub const SHAPES: [Shape; 7] = [
    Shape::new(Part::Background, 0.0, 0.0, 64.0, 64.0, 15.0),
    Shape::new(Part::Body, 12.0, 28.0, 40.0, 20.0, 8.0),
    Shape::new(Part::Body, 15.0, 20.0, 18.0, 18.0, 9.0),
    Shape::new(Part::Body, 27.0, 15.0, 22.0, 22.0, 11.0),
    Shape::new(Part::Eye, 24.0, 31.0, 5.0, 7.0, 1.0),
    Shape::new(Part::Eye, 35.0, 31.0, 5.0, 7.0, 1.0),
    Shape::new(Part::Sparkle, 48.0, 9.0, 6.0, 6.0, 1.0),
];

/// How many samples per pixel edge `render` takes, to smooth the edges.
const SAMPLES: u32 = 4;

/// Draws the logo as RGBA pixels on a transparent square `size` pixels wide.
/// `margin` is the empty border around it, as a fraction of `size` on each
/// side: 0 for a window icon, about 0.1 for a macOS app icon, whose icons
/// leave room around the shape.
pub fn render(size: u32, margin: f32) -> Vec<u8> {
    let inner = size as f32 * (1.0 - 2.0 * margin);
    let offset = size as f32 * margin;
    let scale = DESIGN_SIZE / inner;
    let mut pixels = vec![0; (size * size * 4) as usize];

    for row in 0..size {
        for column in 0..size {
            let mut sum = [0.0_f32; 4];

            for sample_y in 0..SAMPLES {
                for sample_x in 0..SAMPLES {
                    let to_design = |pixel: u32, sample: u32| {
                        ((pixel as f32 + (sample as f32 + 0.5) / SAMPLES as f32) - offset) * scale
                    };
                    let (x, y) = (to_design(column, sample_x), to_design(row, sample_y));

                    if let Some(shape) = SHAPES.iter().rev().find(|shape| shape.contains(x, y)) {
                        let color = shape.part.color();
                        for (total, channel) in sum.iter_mut().zip(color) {
                            *total += f32::from(channel);
                        }
                    }
                }
            }

            let samples = (SAMPLES * SAMPLES) as f32;
            let alpha = sum[3] / samples;
            let index = ((row * size + column) * 4) as usize;
            for channel in 0..3 {
                // The color channels are averaged over the covered samples
                // only, so edges fade out instead of darkening.
                pixels[index + channel] = if alpha > 0.0 {
                    (sum[channel] / samples * 255.0 / alpha).round() as u8
                } else {
                    0
                };
            }
            pixels[index + 3] = alpha.round() as u8;
        }
    }

    pixels
}

/// The logo as an SVG image, drawn with the same shapes as `render`, for
/// places that want it as vector graphics.
pub fn svg() -> String {
    let size = DESIGN_SIZE;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{size}\" height=\"{size}\" \
         viewBox=\"0 0 {size} {size}\">\n  <title>Nubo</title>\n"
    );

    for shape in SHAPES {
        let [red, green, blue, _] = shape.part.color();
        svg += &format!(
            "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" \
             fill=\"#{red:02X}{green:02X}{blue:02X}\"/>\n",
            shape.x, shape.y, shape.width, shape.height, shape.radius
        );
    }

    svg + "</svg>\n"
}

/// The sizes, in pixels, of the pictures in the Windows icon: what Explorer,
/// the taskbar and the Start menu pick from at the usual display scales.
const ICO_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

/// The logo as a Windows `.ico` file, for the executable and its installer.
/// Each size is stored as an uncompressed 32-bit bitmap with its alpha
/// channel, which every Windows since Vista reads.
pub fn ico() -> Vec<u8> {
    let images: Vec<Vec<u8>> = ICO_SIZES.iter().map(|&size| ico_bitmap(size)).collect();

    let mut ico = Vec::new();
    // ICONDIR: reserved, type 1 (icon), number of images.
    for field in [0, 1, ICO_SIZES.len() as u16] {
        ico.extend_from_slice(&field.to_le_bytes());
    }
    let mut offset = 6 + 16 * ICO_SIZES.len();
    for (&size, image) in ICO_SIZES.iter().zip(&images) {
        // ICONDIRENTRY: width and height (0 means 256), no palette, one
        // plane, 32 bits per pixel, then where the image is.
        let side = if size >= 256 { 0 } else { size as u8 };
        ico.extend_from_slice(&[side, side, 0, 0]);
        ico.extend_from_slice(&1_u16.to_le_bytes());
        ico.extend_from_slice(&32_u16.to_le_bytes());
        ico.extend_from_slice(&(image.len() as u32).to_le_bytes());
        ico.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += image.len();
    }
    for image in images {
        ico.extend_from_slice(&image);
    }
    ico
}

/// One picture of the `.ico` file: a BITMAPINFOHEADER, the pixels as BGRA
/// from the bottom row up, and the 1-bit mask of fully transparent pixels
/// that icons also carry (each row padded to four bytes).
fn ico_bitmap(size: u32) -> Vec<u8> {
    let pixels = render(size, 0.0);
    let mask_row_bytes = size.div_ceil(32) * 4;
    let mut bitmap = Vec::new();

    // The height counts the pixels and the mask together, as icons do.
    for field in [40, size, size * 2] {
        bitmap.extend_from_slice(&field.to_le_bytes());
    }
    bitmap.extend_from_slice(&1_u16.to_le_bytes());
    bitmap.extend_from_slice(&32_u16.to_le_bytes());
    // No compression, and the fields for sizes, resolution and palette left
    // at zero.
    bitmap.extend_from_slice(&[0; 24]);

    for row in (0..size).rev() {
        for column in 0..size {
            let index = ((row * size + column) * 4) as usize;
            let [red, green, blue, alpha] = pixels[index..index + 4] else {
                unreachable!("every pixel has four channels");
            };
            bitmap.extend_from_slice(&[blue, green, red, alpha]);
        }
    }
    for row in (0..size).rev() {
        let mut mask_row = vec![0_u8; mask_row_bytes as usize];
        for column in 0..size {
            if pixels[((row * size + column) * 4 + 3) as usize] == 0 {
                mask_row[(column / 8) as usize] |= 0x80 >> (column % 8);
            }
        }
        bitmap.extend_from_slice(&mask_row);
    }
    bitmap
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(pixels: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let index = ((y * size + x) * 4) as usize;
        pixels[index..index + 4].try_into().unwrap()
    }

    #[test]
    fn the_parts_land_where_they_are_drawn() {
        let pixels = render(64, 0.0);

        assert_eq!(pixel(&pixels, 64, 8, 8), Part::Background.color());
        assert_eq!(pixel(&pixels, 64, 32, 44), Part::Body.color());
        assert_eq!(pixel(&pixels, 64, 26, 34), Part::Eye.color());
        assert_eq!(pixel(&pixels, 64, 51, 12), Part::Sparkle.color());
    }

    #[test]
    fn the_corners_are_transparent_and_a_margin_stays_empty() {
        let pixels = render(64, 0.0);
        assert_eq!(pixel(&pixels, 64, 0, 0)[3], 0);
        assert_eq!(pixel(&pixels, 64, 16, 22), Part::Background.color());
        assert_eq!(pixel(&pixels, 64, 38, 17), Part::Body.color());

        let with_margin = render(100, 0.1);
        assert_eq!(pixel(&with_margin, 100, 5, 50)[3], 0);
        assert_eq!(pixel(&with_margin, 100, 50, 15)[3], 255);
    }

    #[test]
    fn the_svg_draws_every_shape() {
        let svg = svg();

        assert!(svg.starts_with("<svg "));
        assert_eq!(svg.matches("<rect ").count(), SHAPES.len());
        assert!(
            svg.contains(r##"<rect x="0" y="0" width="64" height="64" rx="15" fill="#8B7CF6"/>"##)
        );
    }

    #[test]
    fn the_ico_lists_every_size_and_points_at_its_bitmaps() {
        let ico = ico();
        let u16_at = |at: usize| u16::from_le_bytes([ico[at], ico[at + 1]]);
        let u32_at = |at: usize| u32::from_le_bytes(ico[at..at + 4].try_into().unwrap());

        assert_eq!((u16_at(0), u16_at(2)), (0, 1));
        assert_eq!(usize::from(u16_at(4)), ICO_SIZES.len());

        let mut expected_offset = 6 + 16 * ICO_SIZES.len();
        for (entry, &size) in ICO_SIZES.iter().enumerate() {
            let at = 6 + 16 * entry;
            let side = if size == 256 { 0 } else { size as u8 };
            assert_eq!((ico[at], ico[at + 1]), (side, side));
            assert_eq!(u16_at(at + 6), 32);

            let (length, offset) = (u32_at(at + 8) as usize, u32_at(at + 12) as usize);
            assert_eq!(offset, expected_offset);
            let mask_bytes = size.div_ceil(32) * 4 * size;
            assert_eq!(length as u32, 40 + size * size * 4 + mask_bytes);
            assert_eq!(u32_at(offset), 40);
            assert_eq!((u32_at(offset + 4), u32_at(offset + 8)), (size, size * 2));
            expected_offset += length;
        }
        assert_eq!(ico.len(), expected_offset);
    }

    #[test]
    fn the_ico_bitmaps_are_bgra_from_the_bottom_with_a_transparency_mask() {
        let size = 16;
        let bitmap = ico_bitmap(size);
        let pixels = render(size, 0.0);

        // The first stored row is the bottom one.
        let bottom_middle = pixel(&pixels, size, 8, 15);
        let stored = &bitmap[40 + 8 * 4..40 + 8 * 4 + 4];
        assert_eq!(
            stored,
            [
                bottom_middle[2],
                bottom_middle[1],
                bottom_middle[0],
                bottom_middle[3]
            ]
        );

        // The rounded corners are transparent, so their mask bits are set,
        // while the middle of the top row is opaque. The top row is stored
        // last.
        let top_row_mask = &bitmap[40 + (size * size * 4) as usize + 15 * 4..];
        assert_eq!(pixel(&pixels, size, 0, 0)[3], 0);
        assert_eq!(top_row_mask[0] & 0x80, 0x80);
        assert_eq!(pixel(&pixels, size, 8, 0)[3], 255);
        assert_eq!(top_row_mask[1] & 0x80, 0);
    }
}
