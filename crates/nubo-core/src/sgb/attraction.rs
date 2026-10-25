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
pub(super) const CLOUD_AREA: (f32, f32, f32, f32) = (12.0, 15.0, 40.0, 33.0);
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
}

/// What the attraction mode's picture at a frame depends on: how far the
/// clouds have faded in, and where each cloud is along its way. The clouds
/// move a pixel only every few frames, so frames with the same pose draw the
/// same picture, and the border is only drawn again when the pose changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Pose {
    /// How many of the `FADE_IN_FRAMES` the clouds have faded in for.
    fade_in: u32,
    /// How many pixels along its way each cloud is, in the order of
    /// `CLOUDS`, its way being from just off the left edge of the picture to
    /// just off the right.
    positions: [usize; CLOUDS.len()],
}

/// The attraction mode's clouds, drawn once.
pub(super) fn clouds() -> Vec<Cloud> {
    CLOUDS
        .iter()
        .map(|&(width, top, _, _)| Cloud {
            width,
            pixels: draw_cloud(width),
            top,
        })
        .collect()
}

/// The pose at `frame` of the attraction mode, counting from 0 when it
/// starts: the clouds fade in and then drift on, each coming back in from
/// the left once it has left the picture on the right.
pub(super) fn pose(frame: u32) -> Pose {
    Pose {
        fade_in: frame.min(FADE_IN_FRAMES),
        positions: CLOUDS.map(|(width, _, frames_per_pixel, start)| {
            let way = BORDERED_WIDTH + width;
            let along = (frame / frames_per_pixel) as usize % way;
            (along + start) % way
        }),
    }
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

/// Nubo's border with the attraction mode's clouds in `pose`.
pub(super) fn draw(border: &NuboBorder, pose: Pose) -> BorderPicture {
    let mut picture = border.picture.clone();
    let visibility = pose.fade_in as f32 / FADE_IN_FRAMES as f32;
    if visibility == 0.0 {
        return picture;
    }
    for (cloud, &position) in border.clouds.iter().zip(&pose.positions) {
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
            let amount = f32::from(pixel[3]) / 255.0 * visibility * border.open_sky[target];
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
    use crate::sgb::{BORDERED_HEIGHT, SCREEN_HEIGHT, SCREEN_OFFSET, SCREEN_WIDTH, SgbBorderTime};

    #[test]
    fn the_clouds_fade_in_over_the_open_sky_only() {
        let border = NuboBorder::get(SgbBorderTime::Day);
        assert_eq!(draw(border, pose(0)).pixels, border.picture.pixels);

        let picture = draw(border, pose(5_000));
        let changed: Vec<_> = (0..BORDERED_WIDTH * BORDERED_HEIGHT)
            .filter(|&index| picture.pixels[index] != border.picture.pixels[index])
            .collect();
        assert!(!changed.is_empty());
        for index in changed {
            let (x, y) = (index % BORDERED_WIDTH, index / BORDERED_WIDTH);
            assert!(border.open_sky[index] > 0.0, "{x}, {y}");
            let (left, top) = SCREEN_OFFSET;
            assert!(
                !((left..left + SCREEN_WIDTH).contains(&x)
                    && (top..top + SCREEN_HEIGHT).contains(&y))
            );
        }
    }

    #[test]
    fn the_clouds_keep_moving() {
        let border = NuboBorder::get(SgbBorderTime::Night);
        assert_ne!(
            draw(border, pose(1_000)).pixels,
            draw(border, pose(1_100)).pixels
        );
    }

    #[test]
    fn the_pose_changes_only_when_the_picture_can() {
        assert_ne!(pose(10), pose(11));
        // 1,008 is a multiple of every cloud's frames per pixel but 5, so no
        // cloud moves to the next frame, and the cloud taking 5 moves after.
        assert_eq!(pose(1_008), pose(1_009));
        assert_ne!(pose(1_009), pose(1_010));
        let border = NuboBorder::get(SgbBorderTime::Night);
        assert_eq!(
            draw(border, pose(1_008)).pixels,
            draw(border, pose(1_009)).pixels
        );
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
