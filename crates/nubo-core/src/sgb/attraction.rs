//! The attraction mode: the Super Game Boy's screensaver. On a real one, the
//! built-in borders come alive once the controller has been left alone for
//! about seven minutes, until a button is pressed again (Pan Docs). Those
//! animations are Nintendo's, so Nubo's border has one of its own: little
//! clouds, shaped like the one in Nubo's logo, drift across the sky, behind
//! the panel, the buddy peeking over it and the cloud bank. The game keeps
//! running on the screen.
//!
//! A game can turn the attraction mode off (`ATRC_EN`). A real Super Game
//! Boy also starts it when the player presses R, L, L, L, L, R on the Super
//! Nintendo's controller, buttons a Game Boy game never sees; in Nubo only
//! the wait starts it.

use crate::sgb::BORDERED_WIDTH;
use crate::sgb::border::BorderPicture;
use crate::sgb::nubo_border::{NuboBorder, dithered_rgb555, mix};
use nubo_logo::{Part, SHAPES};

/// How many frames the controller has to be left alone before the
/// attraction mode starts: seven minutes at the Game Boy's 59.73 frames a
/// second.
pub(super) const IDLE_FRAMES: u32 = 25_086;
/// How many frames the clouds take to fade in.
const FADE_IN_FRAMES: u32 = 64;
/// The part of the logo's design square the cloud fills, eyes included: its
/// left and top edges, width and height, in design units.
const CLOUD_AREA: (f32, f32, f32, f32) = (12.0, 15.0, 40.0, 33.0);
/// How many samples per pixel edge a cloud is drawn with, to smooth its
/// edges.
const SAMPLES: usize = 4;

/// The clouds: their width in pixels, their top edge, how many frames each
/// takes to move a pixel to the right, and how far along its way each
/// starts. Their rows don't overlap: two are above the panel, passing behind
/// the buddy, and four pass behind the panel and show only at its sides,
/// above the cloud bank.
const CLOUDS: [(usize, usize, u32, usize); 6] = [
    (16, 2, 6, 40),
    (12, 17, 9, 180),
    (20, 64, 4, 100),
    (16, 100, 5, 0),
    (14, 132, 7, 230),
    (10, 166, 8, 150),
];

/// A cloud drifting across the border.
#[derive(Clone, Debug)]
pub(super) struct Cloud {
    /// Its width in pixels.
    width: usize,
    /// Its RGBA pixels, row by row.
    pixels: Vec<[u8; 4]>,
    /// Its top edge in the 256×224 picture.
    top: usize,
    /// How many frames it takes to move a pixel to the right.
    frames_per_pixel: u32,
    /// How many pixels along its way it starts, its way being from just off
    /// the left edge of the picture to just off the right.
    start: usize,
}

/// The attraction mode's clouds, drawn once.
pub(super) fn clouds() -> Vec<Cloud> {
    CLOUDS
        .iter()
        .map(|&(width, top, frames_per_pixel, start)| Cloud {
            width,
            pixels: draw_cloud(width),
            top,
            frames_per_pixel,
            start,
        })
        .collect()
}

/// Draws the cloud of Nubo's logo, with its eyes but without the purple
/// square behind it, `width` pixels wide, as RGBA pixels row by row.
fn draw_cloud(width: usize) -> Vec<[u8; 4]> {
    let (left, top, area_width, area_height) = CLOUD_AREA;
    let scale = area_width / width as f32;
    let height = (area_height / scale).ceil() as usize;
    let shapes: Vec<_> = SHAPES
        .iter()
        .filter(|shape| matches!(shape.part, Part::Body | Part::Eye))
        .collect();

    (0..width * height)
        .map(|index| {
            let (column, row) = (index % width, index / width);
            let mut sum = [0.0_f32; 3];
            let mut covered = 0;
            for sample_y in 0..SAMPLES {
                for sample_x in 0..SAMPLES {
                    let offset = |sample: usize| (sample as f32 + 0.5) / SAMPLES as f32;
                    let x = left + (column as f32 + offset(sample_x)) * scale;
                    let y = top + (row as f32 + offset(sample_y)) * scale;
                    if let Some(shape) = shapes.iter().rev().find(|shape| shape.contains(x, y)) {
                        for (total, channel) in sum.iter_mut().zip(shape.part.color()) {
                            *total += f32::from(channel);
                        }
                        covered += 1;
                    }
                }
            }
            if covered == 0 {
                return [0; 4];
            }
            let [red, green, blue] = sum.map(|total| (total / covered as f32).round() as u8);
            let alpha = (covered as f32 / (SAMPLES * SAMPLES) as f32 * 255.0).round() as u8;
            [red, green, blue, alpha]
        })
        .collect()
}

/// Nubo's border at `frame` of the attraction mode, counting from 0 when it
/// starts: the clouds fade in and then drift on, each coming back in from
/// the left once it has left the picture on the right.
pub(super) fn draw(border: &NuboBorder, frame: u32) -> BorderPicture {
    let mut picture = border.picture.clone();
    let visibility = frame.min(FADE_IN_FRAMES) as f32 / FADE_IN_FRAMES as f32;
    if visibility == 0.0 {
        return picture;
    }
    for cloud in &border.clouds {
        let way = BORDERED_WIDTH + cloud.width;
        let along = (frame / cloud.frames_per_pixel) as usize % way;
        let position = (along + cloud.start) % way;
        for (index, pixel) in cloud.pixels.iter().enumerate() {
            let Some(x) = (position + index % cloud.width)
                .checked_sub(cloud.width)
                .filter(|&x| x < BORDERED_WIDTH)
            else {
                continue;
            };
            let y = cloud.top + index / cloud.width;
            let target = x + y * BORDERED_WIDTH;
            // Only the open sky's share of the pixel shows the cloud.
            let amount = f32::from(pixel[3]) / 255.0 * visibility * border.open_frame[target];
            if amount == 0.0 {
                continue;
            }
            let color = [pixel[0], pixel[1], pixel[2]].map(f32::from);
            let mixed = mix(border.colors[target], color, amount);
            picture.pixels[target] = dithered_rgb555(mixed, x, y);
        }
    }
    picture
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sgb::{BORDERED_HEIGHT, SCREEN_OFFSET, SgbBorderTime};

    #[test]
    fn the_clouds_fade_in_over_the_open_frame_only() {
        let border = NuboBorder::get(SgbBorderTime::Day);
        assert_eq!(draw(border, 0).pixels, border.picture.pixels);

        let picture = draw(border, 5_000);
        let changed: Vec<_> = (0..BORDERED_WIDTH * BORDERED_HEIGHT)
            .filter(|&index| picture.pixels[index] != border.picture.pixels[index])
            .collect();
        assert!(!changed.is_empty());
        for index in changed {
            let (x, y) = (index % BORDERED_WIDTH, index / BORDERED_WIDTH);
            assert!(border.open_frame[index] > 0.0, "{x}, {y}");
            let (left, top) = SCREEN_OFFSET;
            assert!(!((left..left + 160).contains(&x) && (top..top + 144).contains(&y)));
        }
    }

    #[test]
    fn the_clouds_keep_moving() {
        let border = NuboBorder::get(SgbBorderTime::Night);
        assert_ne!(draw(border, 1_000).pixels, draw(border, 1_100).pixels);
    }

    #[test]
    fn every_cloud_has_a_cloud_shape() {
        for cloud in clouds() {
            assert_eq!(cloud.pixels.len() % cloud.width, 0);
            assert!(cloud.pixels.iter().any(|pixel| pixel[3] == 0xFF));
            assert!(cloud.pixels.iter().any(|pixel| pixel[3] == 0));
        }
    }
}
