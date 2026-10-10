use sdl2::rect::Rect;

pub const GAME_BOY_WIDTH: u32 = 160;
pub const GAME_BOY_HEIGHT: u32 = 144;

pub fn render_frame(
    canvas: &mut sdl2::render::Canvas<sdl2::video::Window>,
    texture: &mut sdl2::render::Texture,
    frame: &[u8],
    window_width: u32,
    window_height: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    update_texture(texture, frame)?;
    canvas.clear();

    let (w, h, x, y) = calculate_scaled_dimensions(window_width, window_height);
    canvas.copy(texture, None, Some(Rect::new(x, y, w, h)))?;
    canvas.present();

    Ok(())
}

fn copy_rgba_to_rgb24(src: &[u8], dst: &mut [u8], pitch: usize) {
    for y in 0..GAME_BOY_HEIGHT as usize {
        let src_row = y * GAME_BOY_WIDTH as usize * 4;
        let dst_row = y * pitch;

        for x in 0..GAME_BOY_WIDTH as usize {
            let si = src_row + x * 4;
            let di = dst_row + x * 3;
            dst[di..di + 3].copy_from_slice(&src[si..si + 3]);
        }
    }
}

fn update_texture(texture: &mut sdl2::render::Texture, frame: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    texture.with_lock(None, |buffer: &mut [u8], pitch: usize| {
        copy_rgba_to_rgb24(frame, buffer, pitch);
    })?;
    Ok(())
}

fn calculate_scaled_dimensions(window_width: u32, window_height: u32) -> (u32, u32, i32, i32) {
    let scale_x = window_width / GAME_BOY_WIDTH;
    let scale_y = window_height / GAME_BOY_HEIGHT;

    let scale = scale_x.min(scale_y).max(1);

    let scaled_width = GAME_BOY_WIDTH * scale;
    let scaled_height = GAME_BOY_HEIGHT * scale;

    let x_offset = ((window_width as i32) - (scaled_width as i32)) / 2;
    let y_offset = ((window_height as i32) - (scaled_height as i32)) / 2;

    (scaled_width, scaled_height, x_offset, y_offset)
}
