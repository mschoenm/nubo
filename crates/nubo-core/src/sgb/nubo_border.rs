//! Nubo's own border around the Game Boy screen on a Super Game Boy, drawn
//! instead of Nintendo's built-in ones.
//!
//! The screen sits in a raised panel under an open sky, with a bank of
//! clouds along the bottom. Nubo's cloud buddy (the cloud from its logo,
//! `nubo_logo`, with its eyes) peeks over the panel's top-left edge with its
//! little hands on it. The border has two looks, which the player's clock
//! picks between (`SgbBorderTime`): at night a dusk sky with faint stars and
//! a dark purple panel, by day a blue sky with a sun behind a small cloud and
//! a periwinkle panel.
//!
//! Each look is drawn once, the first time it shows, and shared by every
//! emulator (`NuboBorder::get`). The shapes are drawn with smoothed edges,
//! several samples a pixel. The Super Game Boy shows RGB555 colors, 32 levels
//! a channel, which band visibly in the sky's gradients, so the colors are
//! rounded with an ordered dither (`dithered_rgb555`).

use crate::sgb::attraction::{self, Cloud};
use crate::sgb::border::{BorderPicture, TRANSPARENT};
use crate::sgb::{BORDERED_HEIGHT, BORDERED_WIDTH, SCREEN_OFFSET, SgbBorderTime};
use nubo_logo::{Part, SHAPES};
use std::sync::OnceLock;

/// The colors of Nubo's border that other parts of the Super Game Boy share,
/// as RGB 8-bit channels.
pub(super) mod nubo_colors {
    /// The thin dark rim right around the screen at night.
    pub(in crate::sgb) const RIM: [u8; 3] = [0x14, 0x0E, 0x38];
}

/// A color with 8-bit channels kept as floats, so blending doesn't round.
type Color = [f32; 3];

/// The colors of one look of the border, as RGB 8-bit channels.
struct Look {
    /// The sky at the top of the picture.
    sky_top: [u8; 3],
    /// The sky at the bottom, which the top fades into.
    sky_bottom: [u8; 3],
    /// The clouds at the back of the bank along the bottom.
    back_clouds: [u8; 3],
    /// The clouds at the front of the bank.
    front_clouds: [u8; 3],
    /// The panel the screen sits in.
    panel: [u8; 3],
    /// How much the panel's top edge is lightened towards white, so the
    /// panel looks lit from above.
    panel_highlight: f32,
    /// The color the panel darkens towards at its bottom.
    panel_shade: [u8; 3],
    /// How far the panel's bottom darkens towards `panel_shade`.
    panel_shade_amount: f32,
    /// The color of the panel's soft shadow.
    shadow: [u8; 3],
    /// How dark the panel's shadow is, from 0 to 1.
    shadow_opacity: f32,
    /// The thin rim right around the screen.
    rim: [u8; 3],
    /// The thin outline around the buddy and its hands, which keeps them
    /// clear of the white clouds by day; `None` at night.
    buddy_outline: Option<[u8; 3]>,
}

/// The night look: a dusk sky over a dark purple panel.
const NIGHT: Look = Look {
    sky_top: [0x15, 0x0F, 0x3A],
    sky_bottom: [0x30, 0x25, 0x7A],
    back_clouds: [0x3A, 0x2F, 0x8E],
    front_clouds: [0x46, 0x3A, 0xA0],
    panel: [0x34, 0x29, 0x8A],
    panel_highlight: 0.14,
    panel_shade: [0x0A, 0x06, 0x24],
    panel_shade_amount: 0.18,
    shadow: [0x0A, 0x06, 0x24],
    shadow_opacity: 0.55,
    rim: nubo_colors::RIM,
    buddy_outline: None,
};

/// The day look: a blue sky over a periwinkle panel.
const DAY: Look = Look {
    sky_top: [0x6C, 0x8B, 0xE2],
    sky_bottom: [0xBA, 0xC7, 0xF5],
    back_clouds: [0xC9, 0xD3, 0xF6],
    front_clouds: [0xE4, 0xE9, 0xFE],
    panel: [0x8E, 0xA2, 0xE8],
    panel_highlight: 0.3,
    panel_shade: [0x2B, 0x34, 0x78],
    panel_shade_amount: 0.1,
    shadow: [0x2B, 0x34, 0x78],
    shadow_opacity: 0.3,
    rim: [0x34, 0x40, 0x8A],
    buddy_outline: Some([0x8F, 0x86, 0xD8]),
};

/// The panel the screen sits in: its left, top, right and bottom edges and
/// the radius of its corners, in pixels.
const PANEL: (f32, f32, f32, f32, f32) = (36.0, 28.0, 220.0, 196.0, 10.0);
/// The rim around the screen.
const RIM: (f32, f32, f32, f32, f32) = (44.0, 36.0, 212.0, 188.0, 4.0);
/// The buddy's left and top edges and its width, in pixels. Its top shows
/// above the panel, the rest is behind it.
const BUDDY: (f32, f32, f32) = (50.0, 2.0, 40.0);
/// Where the buddy looks, in the logo's design units away from straight
/// ahead: a little right and down, at the game.
const BUDDY_GAZE: (f32, f32) = (1.0, 1.2);
/// The centers of the buddy's hands on the panel's top edge, and their
/// radius, in pixels.
const HANDS: ([(f32, f32); 2], f32) = ([(59.0, 28.5), (81.0, 28.5)], 3.4);
/// How many samples per pixel edge the shapes are drawn with, to smooth
/// their edges.
const SAMPLES: usize = 4;

/// Nubo's border, drawn in one look. Besides the picture, it keeps what the
/// attraction mode (`attraction.rs`) needs to let clouds drift across the
/// sky, behind the panel, the buddy and the cloud bank.
#[derive(Clone, Debug)]
pub(super) struct NuboBorder {
    /// The border as it normally shows.
    pub(super) picture: BorderPicture,
    /// The same picture before its colors were rounded to RGB555, row by
    /// row.
    pub(super) colors: Vec<[f32; 3]>,
    /// How much of each pixel is open sky rather than the panel, the buddy
    /// or the cloud bank, from 0 to 1.
    pub(super) open_sky: Vec<f32>,
    /// The clouds of the attraction mode.
    pub(super) clouds: Vec<Cloud>,
}

impl NuboBorder {
    /// The border in `time`'s look, drawn the first time it is asked for.
    pub(super) fn get(time: SgbBorderTime) -> &'static NuboBorder {
        static NIGHT_BORDER: OnceLock<NuboBorder> = OnceLock::new();
        static DAY_BORDER: OnceLock<NuboBorder> = OnceLock::new();
        let border = match time {
            SgbBorderTime::Night => &NIGHT_BORDER,
            SgbBorderTime::Day => &DAY_BORDER,
        };
        border.get_or_init(|| NuboBorder::new(time))
    }

    /// Draws the border in `time`'s look: the sky, the cloud bank, the buddy
    /// behind the panel, the panel and the rim, and the buddy's hands on the
    /// panel's edge.
    fn new(time: SgbBorderTime) -> Self {
        let look = match time {
            SgbBorderTime::Night => &NIGHT,
            SgbBorderTime::Day => &DAY,
        };
        let mut canvas = Canvas::new();
        let (sky_top, sky_bottom) = (rgb(look.sky_top), rgb(look.sky_bottom));
        for (index, pixel) in canvas.colors.iter_mut().enumerate() {
            let y = (index / BORDERED_WIDTH) as f32;
            *pixel = mix(sky_top, sky_bottom, y / BORDERED_HEIGHT as f32);
        }
        match time {
            SgbBorderTime::Night => draw_stars(&mut canvas),
            SgbBorderTime::Day => draw_sun_and_clouds(&mut canvas),
        }
        draw_cloud_bank(&mut canvas, look);
        draw_buddy(&mut canvas, look);
        draw_panel(&mut canvas, look);
        for (x, y) in HANDS.0 {
            draw_hand(&mut canvas, look, x, y);
        }

        let pixels = canvas
            .colors
            .iter()
            .enumerate()
            .map(|(index, &color)| {
                let (x, y) = (index % BORDERED_WIDTH, index / BORDERED_WIDTH);
                if in_game_screen(x, y) {
                    TRANSPARENT
                } else {
                    dithered_rgb555(color, x, y)
                }
            })
            .collect();
        NuboBorder {
            picture: BorderPicture { pixels },
            colors: canvas.colors,
            open_sky: canvas.open_sky,
            clouds: attraction::clouds(),
        }
    }
}

/// Scatters faint stars over the night sky, leaving out the part behind the
/// panel.
fn draw_stars(canvas: &mut Canvas) {
    for star in 0..60 {
        let x = (hash(star * 2) % BORDERED_WIDTH as u32) as f32;
        let y = (hash(star * 2 + 1) % 120) as f32;
        if (30.0..226.0).contains(&x) && y > 22.0 {
            continue;
        }
        let size = if hash(star + 99).is_multiple_of(4) {
            2.0
        } else {
            1.0
        };
        let brightness = 0.25 + 0.35 * (1.0 - y / 140.0);
        canvas.paint(
            &[Shape::rectangle(x, y, x + size, y + size)],
            |_, _| rgb([0xE4, 0xDF, 0xFF]),
            brightness,
            Layer::Sky,
        );
    }
}

/// Draws the daytime sun, half behind a small cloud in the top-right corner,
/// and two more small clouds drifting beside the panel.
fn draw_sun_and_clouds(canvas: &mut Canvas) {
    let (x, y, radius) = (238.0, 15.0, 9.0);
    canvas.soften(
        &[Shape::circle(x, y, radius + 3.0)],
        (0.0, 0.0),
        6,
        rgb([0xFF, 0xF0, 0xB0]),
        0.45,
    );
    canvas.paint(
        &[Shape::circle(x, y, radius)],
        |px, py| {
            let distance = ((px - x) * (px - x) + (py - y) * (py - y)).sqrt() / radius;
            let distance = distance.min(1.0);
            mix(
                rgb([0xFF, 0xFB, 0xE8]),
                rgb([0xFF, 0xD3, 0x6E]),
                distance * distance,
            )
        },
        1.0,
        Layer::Sky,
    );
    canvas.paint(
        &small_cloud(232.0, 22.0, 8.0),
        |_, _| rgb([0xF8, 0xF9, 0xFF]),
        1.0,
        Layer::Sky,
    );
    for (x, y, size) in [(243.0, 84.0, 8.0), (14.0, 160.0, 7.0)] {
        canvas.paint(
            &small_cloud(x, y, size),
            |_, _| rgb([0xF4, 0xF6, 0xFF]),
            0.7,
            Layer::Sky,
        );
    }
}

/// A small cloud of three puffs on a flat base, `size` pixels from its middle
/// to either side.
fn small_cloud(x: f32, y: f32, size: f32) -> [Shape; 4] {
    [
        Shape::circle(x - 0.9 * size, y + 0.25 * size, 0.55 * size),
        Shape::circle(x, y - 0.1 * size, 0.8 * size),
        Shape::circle(x + 0.95 * size, y + 0.2 * size, 0.6 * size),
        Shape::rectangle(x - size, y, x + size, y + 0.75 * size),
    ]
}

/// Draws the bank of clouds along the bottom: a row of round puffs at the
/// back and a lower row at the front.
fn draw_cloud_bank(canvas: &mut Canvas, look: &Look) {
    let mut back = cloud_puffs(7, 222.0, 12.0);
    back.push(Shape::rectangle(
        0.0,
        220.0,
        BORDERED_WIDTH as f32,
        BORDERED_HEIGHT as f32,
    ));
    let back_color = rgb(look.back_clouds);
    canvas.paint(&back, |_, _| back_color, 1.0, Layer::Front);
    let front_color = rgb(look.front_clouds);
    canvas.paint(
        &cloud_puffs(31, 230.0, 8.0),
        |_, _| front_color,
        1.0,
        Layer::Front,
    );
}

/// A row of round puffs across the picture with their middles at `base`,
/// each 10 pixels across plus up to `extra` more, sized by `seed`.
fn cloud_puffs(seed: u32, base: f32, extra: f32) -> Vec<Shape> {
    let mut puffs = Vec::new();
    let mut x = -10.0;
    let mut index = 0;
    while x < BORDERED_WIDTH as f32 + 20.0 {
        let radius = 10.0 + (hash(seed + index) % 100) as f32 / 100.0 * extra;
        puffs.push(Shape::circle(x, base, radius));
        x += radius * 1.3;
        index += 1;
    }
    puffs
}

/// Draws the buddy, the cloud of Nubo's logo with its eyes, rosy cheeks and
/// a glint in each eye, behind where the panel goes.
fn draw_buddy(canvas: &mut Canvas, look: &Look) {
    let (left, top, width) = BUDDY;
    let scale = width / 40.0;
    // The logo's cloud spans 12 to 52 across and starts 15 down its
    // 64-unit design square.
    let to_pixels = |(x, y, width, height, radius): (f32, f32, f32, f32, f32)| {
        Shape::rounded(
            left + (x - 12.0) * scale,
            top + (y - 15.0) * scale,
            left + (x + width - 12.0) * scale,
            top + (y + height - 15.0) * scale,
            radius * scale,
        )
    };
    let body: Vec<Shape> = SHAPES
        .iter()
        .filter(|shape| shape.part == Part::Body)
        .map(|shape| to_pixels((shape.x, shape.y, shape.width, shape.height, shape.radius)))
        .collect();
    if let Some(outline) = look.buddy_outline {
        let outline = rgb(outline);
        let grown: Vec<Shape> = body.iter().map(|shape| shape.grown(1.0)).collect();
        canvas.paint(&grown, |_, _| outline, 0.55, Layer::Front);
    }
    let height = 33.0 * scale;
    canvas.paint(&body, |_, y| buddy_color(y, top, height), 1.0, Layer::Front);

    let to_point = |x: f32, y: f32| (left + (x - 12.0) * scale, top + (y - 15.0) * scale);
    let cheeks = [19.0, 45.0].map(|x| {
        let (x, y) = to_point(x, 40.0);
        Shape::ellipse(x, y, 3.4 * scale, 2.2 * scale)
    });
    canvas.paint(&cheeks, |_, _| rgb([0xFF, 0x8F, 0xC0]), 0.6, Layer::Front);

    let [red, green, blue, _] = Part::Eye.color();
    let eye_color = rgb([red, green, blue]);
    let (gaze_x, gaze_y) = BUDDY_GAZE;
    for eye_x in [24.0, 35.0] {
        let (x, y) = (eye_x + gaze_x, 31.0 + gaze_y);
        canvas.paint(
            &[to_pixels((x, y, 5.0, 7.0, 2.2))],
            |_, _| eye_color,
            1.0,
            Layer::Front,
        );
        let (glint_x, glint_y) = to_point(x + 1.6, y + 1.8);
        canvas.paint(
            &[Shape::circle(glint_x, glint_y, 1.1 * scale)],
            |_, _| rgb([0xFF, 0xFF, 0xFF]),
            0.95,
            Layer::Front,
        );
    }
}

/// The buddy's color at row `y`: near white at its top, shading to lavender
/// at its bottom, `height` below, as if lit from above.
fn buddy_color(y: f32, top: f32, height: f32) -> Color {
    let along = ((y - top) / height).clamp(0.0, 1.0);
    mix(
        rgb([0xFB, 0xF9, 0xFF]),
        rgb([0xC9, 0xBF, 0xFF]),
        along * along,
    )
}

/// Draws one of the buddy's hands, a little round puff in its colors, on the
/// panel's edge.
fn draw_hand(canvas: &mut Canvas, look: &Look, x: f32, y: f32) {
    let radius = HANDS.1;
    if let Some(outline) = look.buddy_outline {
        let outline = rgb(outline);
        canvas.paint(
            &[Shape::circle(x, y, radius + 1.0)],
            |_, _| outline,
            0.55,
            Layer::Front,
        );
    }
    canvas.paint(
        &[Shape::circle(x, y, radius)],
        |_, py| buddy_color(py, y - radius, 2.8 * radius),
        1.0,
        Layer::Front,
    );
}

/// Draws the panel, raised by a soft shadow below it, a lit top edge and a
/// bottom that darkens a little, and the rim around the screen.
fn draw_panel(canvas: &mut Canvas, look: &Look) {
    let (left, top, right, bottom, radius) = PANEL;
    let panel = Shape::rounded(left, top, right, bottom, radius);
    canvas.soften(
        &[panel],
        (0.0, 3.0),
        3,
        rgb(look.shadow),
        look.shadow_opacity,
    );
    let color = rgb(look.panel);
    let highlight = mix(color, rgb([0xFF, 0xFF, 0xFF]), look.panel_highlight);
    canvas.paint(&[panel], |_, _| highlight, 1.0, Layer::Front);
    let shade = rgb(look.panel_shade);
    canvas.paint(
        &[Shape::rounded(left, top + 1.0, right, bottom, radius)],
        |_, y| {
            let along = ((y - top) / (bottom - top)).clamp(0.0, 1.0);
            mix(color, shade, along * look.panel_shade_amount)
        },
        1.0,
        Layer::Front,
    );
    let (left, top, right, bottom, radius) = RIM;
    let rim = rgb(look.rim);
    canvas.paint(
        &[Shape::rounded(left, top, right, bottom, radius)],
        |_, _| rim,
        1.0,
        Layer::Front,
    );
}

/// Which layer of the border a shape is painted on: the attraction mode's
/// clouds drift in front of the sky and behind the front.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layer {
    /// The sky and what is in it: the stars, the sun, its small clouds.
    Sky,
    /// The cloud bank, the panel and the buddy.
    Front,
}

/// A shape to paint, in pixels.
#[derive(Clone, Copy, Debug)]
enum Shape {
    /// A rectangle with rounded corners.
    Rounded {
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        radius: f32,
    },
    /// An ellipse with its middle at `x`, `y`.
    Ellipse {
        x: f32,
        y: f32,
        width_radius: f32,
        height_radius: f32,
    },
}

impl Shape {
    /// A rectangle with rounded corners of `radius`.
    fn rounded(left: f32, top: f32, right: f32, bottom: f32, radius: f32) -> Shape {
        Shape::Rounded {
            left,
            top,
            right,
            bottom,
            radius,
        }
    }

    /// A rectangle with square corners.
    fn rectangle(left: f32, top: f32, right: f32, bottom: f32) -> Shape {
        Shape::rounded(left, top, right, bottom, 0.0)
    }

    /// An ellipse with its middle at `x`, `y`.
    fn ellipse(x: f32, y: f32, width_radius: f32, height_radius: f32) -> Shape {
        Shape::Ellipse {
            x,
            y,
            width_radius,
            height_radius,
        }
    }

    /// A circle with its middle at `x`, `y`.
    fn circle(x: f32, y: f32, radius: f32) -> Shape {
        Shape::ellipse(x, y, radius, radius)
    }

    /// The same shape, `amount` pixels bigger all round, for an outline.
    fn grown(self, amount: f32) -> Shape {
        match self {
            Shape::Rounded {
                left,
                top,
                right,
                bottom,
                radius,
            } => Shape::rounded(
                left - amount,
                top - amount,
                right + amount,
                bottom + amount,
                radius + amount,
            ),
            Shape::Ellipse {
                x,
                y,
                width_radius,
                height_radius,
            } => Shape::ellipse(x, y, width_radius + amount, height_radius + amount),
        }
    }

    /// Whether the point `x`, `y` lies inside the shape.
    fn contains(self, x: f32, y: f32) -> bool {
        match self {
            Shape::Rounded {
                left,
                top,
                right,
                bottom,
                radius,
            } => {
                if x < left || y < top || x >= right || y >= bottom {
                    return false;
                }
                let nearest_x = x.clamp(left + radius, right - radius);
                let nearest_y = y.clamp(top + radius, bottom - radius);
                let (dx, dy) = (x - nearest_x, y - nearest_y);
                dx * dx + dy * dy <= radius * radius
            }
            Shape::Ellipse {
                x: middle_x,
                y: middle_y,
                width_radius,
                height_radius,
            } => {
                let dx = (x - middle_x) / width_radius;
                let dy = (y - middle_y) / height_radius;
                dx * dx + dy * dy <= 1.0
            }
        }
    }

    /// The shape's left, top, right and bottom edges.
    fn bounds(self) -> (f32, f32, f32, f32) {
        match self {
            Shape::Rounded {
                left,
                top,
                right,
                bottom,
                ..
            } => (left, top, right, bottom),
            Shape::Ellipse {
                x,
                y,
                width_radius,
                height_radius,
            } => (
                x - width_radius,
                y - height_radius,
                x + width_radius,
                y + height_radius,
            ),
        }
    }
}

/// The 256×224 picture being drawn, with how much of each pixel is still
/// open sky.
struct Canvas {
    /// Every pixel's color, row by row.
    colors: Vec<Color>,
    /// How much of each pixel is open sky, from 0 to 1.
    open_sky: Vec<f32>,
}

impl Canvas {
    /// An empty canvas, all open sky.
    fn new() -> Self {
        Canvas {
            colors: vec![[0.0; 3]; BORDERED_WIDTH * BORDERED_HEIGHT],
            open_sky: vec![1.0; BORDERED_WIDTH * BORDERED_HEIGHT],
        }
    }

    /// How much of each pixel the shapes together cover, from 0 to 1, for
    /// the pixels they reach: the pixel's index and its coverage.
    fn coverage(shapes: &[Shape]) -> impl Iterator<Item = (usize, f32)> + '_ {
        let (mut left, mut top, mut right, mut bottom) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for shape in shapes {
            let (shape_left, shape_top, shape_right, shape_bottom) = shape.bounds();
            left = left.min(shape_left);
            top = top.min(shape_top);
            right = right.max(shape_right);
            bottom = bottom.max(shape_bottom);
        }
        let columns =
            left.floor().max(0.0) as usize..(right.ceil().max(0.0) as usize).min(BORDERED_WIDTH);
        let rows =
            top.floor().max(0.0) as usize..(bottom.ceil().max(0.0) as usize).min(BORDERED_HEIGHT);
        rows.flat_map(move |y| columns.clone().map(move |x| (x, y)))
            .filter_map(move |(x, y)| {
                let mut covered = 0;
                for sample_y in 0..SAMPLES {
                    for sample_x in 0..SAMPLES {
                        let offset = |sample: usize| (sample as f32 + 0.5) / SAMPLES as f32;
                        let (point_x, point_y) =
                            (x as f32 + offset(sample_x), y as f32 + offset(sample_y));
                        if shapes.iter().any(|shape| shape.contains(point_x, point_y)) {
                            covered += 1;
                        }
                    }
                }
                (covered > 0).then(|| {
                    (
                        x + y * BORDERED_WIDTH,
                        covered as f32 / (SAMPLES * SAMPLES) as f32,
                    )
                })
            })
    }

    /// Paints the shapes together, each pixel in `fill`'s color at its
    /// middle and `opacity` of the way over what is there. Shapes on the
    /// front layer take what they cover out of the open sky.
    fn paint(
        &mut self,
        shapes: &[Shape],
        fill: impl Fn(f32, f32) -> Color,
        opacity: f32,
        layer: Layer,
    ) {
        for (index, covered) in Canvas::coverage(shapes) {
            let amount = covered * opacity;
            let (x, y) = (index % BORDERED_WIDTH, index / BORDERED_WIDTH);
            let color = fill(x as f32 + 0.5, y as f32 + 0.5);
            self.colors[index] = mix(self.colors[index], color, amount);
            if layer == Layer::Front {
                self.open_sky[index] *= 1.0 - amount;
            }
        }
    }

    /// Paints a soft, blurred copy of the shapes in `color`, moved by
    /// `offset`, for a shadow or a glow. `radius` sets how far the blur
    /// spreads, in pixels.
    fn soften(
        &mut self,
        shapes: &[Shape],
        offset: (f32, f32),
        radius: usize,
        color: Color,
        opacity: f32,
    ) {
        let moved: Vec<Shape> = shapes
            .iter()
            .map(|&shape| match shape {
                Shape::Rounded {
                    left,
                    top,
                    right,
                    bottom,
                    radius,
                } => Shape::rounded(
                    left + offset.0,
                    top + offset.1,
                    right + offset.0,
                    bottom + offset.1,
                    radius,
                ),
                Shape::Ellipse {
                    x,
                    y,
                    width_radius,
                    height_radius,
                } => Shape::ellipse(x + offset.0, y + offset.1, width_radius, height_radius),
            })
            .collect();
        let mut amounts = vec![0.0; BORDERED_WIDTH * BORDERED_HEIGHT];
        for (index, covered) in Canvas::coverage(&moved) {
            amounts[index] = covered;
        }
        // Three box blurs in a row come close to a smooth, round blur.
        for _ in 0..3 {
            amounts = box_blur(&amounts, radius);
        }
        for (pixel, amount) in self.colors.iter_mut().zip(amounts) {
            *pixel = mix(*pixel, color, amount * opacity);
        }
    }
}

/// Averages every value with its neighbors up to `radius` away, first along
/// the rows and then along the columns. Past the picture's edges the edge
/// values carry on.
fn box_blur(values: &[f32], radius: usize) -> Vec<f32> {
    let span = (2 * radius + 1) as f32;
    // The positions within `radius` of `position`, held inside 0..length.
    let neighbors = move |position: usize, length: usize| {
        (0..=2 * radius).map(move |step| (position + step).saturating_sub(radius).min(length - 1))
    };
    let mut across = vec![0.0; values.len()];
    for y in 0..BORDERED_HEIGHT {
        for x in 0..BORDERED_WIDTH {
            let sum: f32 = neighbors(x, BORDERED_WIDTH)
                .map(|along| values[along + y * BORDERED_WIDTH])
                .sum();
            across[x + y * BORDERED_WIDTH] = sum / span;
        }
    }
    let mut down = vec![0.0; values.len()];
    for y in 0..BORDERED_HEIGHT {
        for x in 0..BORDERED_WIDTH {
            let sum: f32 = neighbors(y, BORDERED_HEIGHT)
                .map(|along| across[x + along * BORDERED_WIDTH])
                .sum();
            down[x + y * BORDERED_WIDTH] = sum / span;
        }
    }
    down
}

/// Whether `x`, `y` in the 256×224 picture is on the game screen.
fn in_game_screen(x: usize, y: usize) -> bool {
    let (screen_left, screen_top) = SCREEN_OFFSET;
    (screen_left..screen_left + 160).contains(&x) && (screen_top..screen_top + 144).contains(&y)
}

/// The RGB555 color for `color` at `x`, `y` in the picture. Where a channel
/// falls between two of RGB555's 32 levels, a 4×4 pattern (an ordered, or
/// Bayer, dither) picks the upper level for that share of the pixels, so a
/// gradient blends smoothly instead of showing bands. Channels close to a
/// level stay flat, so flat colors show no pattern.
pub(super) fn dithered_rgb555(color: [f32; 3], x: usize, y: usize) -> u16 {
    const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    let threshold = (f32::from(BAYER[y % 4][x % 4]) + 0.5) / 16.0;
    let channel = |value: f32| {
        let exact = value.clamp(0.0, 255.0) * 31.0 / 255.0;
        let level = exact.floor();
        let fraction = exact - level;
        let up = if fraction < 0.2 {
            false
        } else if fraction > 0.8 {
            true
        } else {
            fraction > threshold
        };
        level as u16 + u16::from(up)
    };
    channel(color[0]) | channel(color[1]) << 5 | channel(color[2]) << 10
}

/// An RGB color with 8-bit channels, as floats.
fn rgb(color: [u8; 3]) -> Color {
    color.map(f32::from)
}

/// Mixes `over` into `under`, `amount` (0 to 1) of the way.
pub(super) fn mix(under: Color, over: Color, amount: f32) -> Color {
    std::array::from_fn(|channel| under[channel] + (over[channel] - under[channel]) * amount)
}

/// Scrambles `value` into a number that looks random but is always the
/// same for the same value, to scatter the stars and size the clouds.
fn hash(value: u32) -> u32 {
    let mut mixed = value.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    mixed ^= mixed >> 15;
    mixed = mixed.wrapping_mul(0x2C1B_3C6D);
    mixed ^= mixed >> 12;
    mixed = mixed.wrapping_mul(0x297A_2D39);
    mixed ^ (mixed >> 15)
}
