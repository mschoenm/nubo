use super::*;
use crate::gpu::DMG_SHADES;
use crate::sgb::packets::PACKET_SIZE;

/// A Super Game Boy running a game that supports it, with the JOYP select
/// bits a game would have written last.
struct Console {
    sgb: Sgb,
    select: u8,
}

impl Console {
    fn new() -> Self {
        let mut sgb = Sgb::new();
        sgb.start(0x03, 0x33);
        sgb.finish_intro();
        Console { sgb, select: 0x30 }
    }

    /// Writes JOYP and returns the players an `MLT_REQ` asked for.
    fn write(&mut self, value: u8) -> Option<u8> {
        let players = self.sgb.write_joyp(self.select, value);
        self.select = value & 0x30;
        players
    }

    /// Sends the packets of `command` (padded to whole packets) the way games
    /// do: a start pulse, the bits lowest first, and a 0 stop bit.
    fn send(&mut self, command: &[u8]) -> Option<u8> {
        let mut players = None;
        for packet in command.chunks(PACKET_SIZE) {
            let mut bytes = [0; PACKET_SIZE];
            bytes[..packet.len()].copy_from_slice(packet);
            self.write(0x00);
            self.write(0x30);
            for byte in bytes {
                for bit in 0..8 {
                    self.write(if byte & (1 << bit) != 0 { 0x10 } else { 0x20 });
                    self.write(0x30);
                }
            }
            self.write(0x20);
            players = self.write(0x30).or(players);
        }
        players
    }

    /// Finishes a frame whose pixels all have `shade`, and returns the game
    /// screen.
    fn frame_of_shade(&mut self, shade: usize) -> Vec<u8> {
        let frame = DMG_SHADES[shade].repeat(SCREEN_PIXELS);
        self.frame(Some(&frame))
    }

    /// Finishes a frame and returns the game screen.
    fn frame(&mut self, frame: Option<&[u8]>) -> Vec<u8> {
        let mut game_screen = vec![0; SCREEN_PIXELS * 4];
        self.sgb.finish_frame(frame, &mut game_screen);
        game_screen
    }
}

/// The first byte of a command of `packets` packets.
fn header(command: u8, packets: u8) -> u8 {
    command << 3 | packets
}

/// The RGBA color of the game screen's pixel at `x`, `y`.
fn pixel(game_screen: &[u8], x: usize, y: usize) -> [u8; 4] {
    let index = (x + y * SCREEN_WIDTH) * 4;
    game_screen[index..index + 4].try_into().unwrap()
}

/// A `PAL01` command setting color 0 and colors 1 to 3 of palettes 0 and 1.
fn pal01(colors: [u16; 7]) -> Vec<u8> {
    let mut command = vec![header(command::PAL01, 1)];
    for color in colors {
        command.extend_from_slice(&color.to_le_bytes());
    }
    command
}

#[test]
fn a_whole_packet_with_its_stop_bit_runs_the_command() {
    let mut console = Console::new();
    assert_eq!(console.send(&[header(command::MLT_REQ, 1), 0x03]), Some(4));
    assert_eq!(console.send(&[header(command::MLT_REQ, 1), 0x01]), Some(2));
}

#[test]
fn a_stop_bit_of_1_still_ends_the_packet() {
    let mut console = Console::new();
    console.write(0x00);
    console.write(0x30);
    for byte in [header(command::MLT_REQ, 1), 0x03]
        .into_iter()
        .chain([0; 14])
    {
        for bit in 0..8 {
            console.write(if byte & (1 << bit) != 0 { 0x10 } else { 0x20 });
            console.write(0x30);
        }
    }
    console.write(0x10);
    assert_eq!(console.write(0x30), Some(4));
}

#[test]
fn bits_before_a_start_pulse_are_ignored() {
    let mut console = Console::new();
    for _ in 0..64 {
        console.write(0x10);
        console.write(0x30);
    }
    assert_eq!(console.send(&[header(command::MLT_REQ, 1), 0x03]), Some(4));
}

#[test]
fn games_without_super_game_boy_support_send_nothing() {
    let mut console = Console::new();
    console.sgb.start(0x00, 0x33);
    assert_eq!(console.send(&[header(command::MLT_REQ, 1), 0x03]), None);
    console.sgb.start(0x03, 0x01);
    assert_eq!(console.send(&[header(command::MLT_REQ, 1), 0x03]), None);
}

#[test]
fn the_screen_keeps_the_game_boy_grays_until_a_game_sets_colors() {
    let mut console = Console::new();
    let game_screen = console.frame_of_shade(2);
    assert_eq!(pixel(&game_screen, 0, 0), DMG_SHADES[2]);
    assert!(!console.sgb.frame_colorized());

    console.send(&pal01([
        0x0000, 0x001F, 0x03E0, 0x7C00, 0x7FFF, 0x0000, 0x0000,
    ]));
    let game_screen = console.frame_of_shade(2);
    assert_eq!(pixel(&game_screen, 0, 0), [0x00, 0xFF, 0x00, 0xFF]);
    assert!(console.sgb.frame_colorized());
}

#[test]
fn color_0_is_shared_by_every_palette() {
    let mut console = Console::new();
    console.send(&pal01([0x001F, 0, 0, 0, 0, 0, 0]));
    for palette in 0..4 {
        assert_eq!(console.sgb.palette_color(palette * 4), 0x001F);
    }
}

#[test]
fn attribute_blocks_color_their_inside_edge_and_outside() {
    let mut console = Console::new();
    console.send(&pal01([
        0x0000, 0x001F, 0x001F, 0x001F, 0x03E0, 0x03E0, 0x03E0,
    ]));
    // One block from square (2, 2) to (6, 6): palette 1 inside and on the
    // edge, palette 0 outside.
    console.send(&[header(command::ATTR_BLK, 1), 1, 0x07, 0b00_0101, 2, 2, 6, 6]);
    let game_screen = console.frame_of_shade(3);
    assert_eq!(pixel(&game_screen, 4 * 8, 4 * 8), [0x00, 0xFF, 0x00, 0xFF]);
    assert_eq!(pixel(&game_screen, 2 * 8, 2 * 8), [0x00, 0xFF, 0x00, 0xFF]);
    assert_eq!(pixel(&game_screen, 0, 0), [0xFF, 0x00, 0x00, 0xFF]);
}

#[test]
fn attribute_lines_divisions_and_squares_set_the_attribute_map() {
    let mut console = Console::new();
    console.send(&[header(command::ATTR_LIN, 1), 2, 0x80 | 0x20 | 3, 0x40 | 5]);
    assert_eq!(console.sgb.attribute_map[7 + 3 * 20], 1);
    assert_eq!(console.sgb.attribute_map[5 + 10 * 20], 2);

    console.send(&[header(command::ATTR_DIV, 1), 0x40 | 0b10_01_11, 9]);
    assert_eq!(console.sgb.attribute_map[0], 1);
    assert_eq!(console.sgb.attribute_map[9 * 20], 2);
    assert_eq!(console.sgb.attribute_map[17 * 20], 3);

    console.send(&[header(command::ATTR_CHR, 1), 19, 0, 3, 0, 0, 0b11_10_01_00]);
    assert_eq!(console.sgb.attribute_map[19], 3);
    assert_eq!(console.sgb.attribute_map[20], 2);
    assert_eq!(console.sgb.attribute_map[21], 1);
}

#[test]
fn a_transfer_reads_the_screen_three_frames_later() {
    let mut console = Console::new();
    console.send(&[header(command::PAL_TRN, 1)]);
    console.frame_of_shade(1);
    console.frame_of_shade(1);
    assert!(console.sgb.system_palettes.iter().all(|&byte| byte == 0));
    console.frame_of_shade(1);
    // Shade 1 sets the low bit plane: 0xFF, 0x00 for every row.
    assert_eq!(&console.sgb.system_palettes[..4], &[0xFF, 0x00, 0xFF, 0x00]);

    // PAL_SET picks palettes from them: 0x00FF is a red of 31 and a green of 7.
    console.send(&[header(command::PAL_SET, 1), 0, 0, 1, 0, 2, 0, 3, 0, 0]);
    assert_eq!(console.sgb.palette_color(5), 0x00FF);
}

#[test]
fn an_attribute_transfer_fills_the_attribute_files_that_attr_set_loads() {
    let mut console = Console::new();
    console.send(&[header(command::ATTR_TRN, 1)]);
    for _ in 0..3 {
        console.frame_of_shade(3);
    }
    console.send(&[header(command::ATTR_SET, 1), 0]);
    assert!(
        console
            .sgb
            .attribute_map
            .iter()
            .all(|&palette| palette == 3)
    );
}

#[test]
fn the_mask_freezes_or_blanks_the_screen() {
    let mut console = Console::new();
    console.frame_of_shade(1);
    console.send(&[header(command::MASK_EN, 1), 1]);
    let game_screen = console.frame_of_shade(3);
    assert_eq!(pixel(&game_screen, 0, 0), DMG_SHADES[1]);

    console.send(&[header(command::MASK_EN, 1), 2]);
    let game_screen = console.frame_of_shade(3);
    assert_eq!(pixel(&game_screen, 0, 0), [0x00, 0x00, 0x00, 0xFF]);

    console.send(&[header(command::MASK_EN, 1), 0]);
    let game_screen = console.frame_of_shade(3);
    assert_eq!(pixel(&game_screen, 0, 0), DMG_SHADES[3]);
}

#[test]
fn the_picture_stays_while_the_lcd_draws_nothing() {
    let mut console = Console::new();
    console.frame_of_shade(2);
    let game_screen = console.frame(None);
    assert_eq!(pixel(&game_screen, 80, 70), DMG_SHADES[2]);
}

#[test]
fn nubo_border_surrounds_the_screen_until_a_game_sends_its_own() {
    let mut console = Console::new();
    console.frame_of_shade(3);
    let bordered = console.sgb.bordered_frame();
    let (left, top) = SCREEN_OFFSET;
    let at = |x: usize, y: usize| {
        let index = (x + y * BORDERED_WIDTH) * 4;
        <[u8; 4]>::try_from(&bordered[index..index + 4]).unwrap()
    };
    assert_eq!(at(left, top), DMG_SHADES[3]);
    assert_ne!(at(0, 0), DMG_SHADES[3]);
    assert_eq!(at(0, 0)[3], 0xFF);
}

#[test]
fn a_game_border_fades_in_after_the_old_one_fades_out() {
    let mut console = Console::new();
    // Color 1 of the first border palette everywhere: tile 0 with every
    // pixel's lowest bit set.
    console.send(&[header(command::CHR_TRN, 1), 0]);
    for _ in 0..3 {
        console.frame_of_shade(1);
    }
    assert_eq!(console.sgb.pending_border.tiles[0], 0xFF);

    console.send(&[header(command::PCT_TRN, 1)]);
    for _ in 0..3 {
        console.frame_of_shade(0);
    }
    assert!(console.sgb.pending_border.from_game);
    assert!(!console.sgb.border.from_game);

    for _ in 0..(BORDER_FADE_FRAMES - 32) {
        console.frame_of_shade(0);
    }
    assert!(console.sgb.border.from_game);
    for _ in 0..32 {
        console.frame_of_shade(0);
    }
    assert_eq!(console.sgb.border_fade, 0);
}

#[test]
fn border_tiles_flip_and_pick_their_palette() {
    let mut border = Border::default();
    // Tile 1: only its top-left pixel has color 3.
    border.tiles[32] = 0x80;
    border.tiles[33] = 0x80;
    let entry: u16 = 1 | 1 << 10 | 0x4000;
    border.map[..2].copy_from_slice(&entry.to_le_bytes());
    let color_3_of_palette_1 = 32 * 32 * 2 + (16 + 3) * 2;
    border.map[color_3_of_palette_1..color_3_of_palette_1 + 2]
        .copy_from_slice(&0x1234_u16.to_le_bytes());
    assert_eq!(border.pixel(7, 0), Some(0x1234));
    assert_eq!(border.pixel(0, 0), None);

    // Black with the top bit set is still black, not the transparent marker.
    border.map[color_3_of_palette_1..color_3_of_palette_1 + 2]
        .copy_from_slice(&0x8000_u16.to_le_bytes());
    assert_eq!(border.pixel(7, 0), Some(0x0000));
}

#[test]
fn faded_colors_darken_every_channel() {
    assert_eq!(faded(0x7FFF, 1), 0x7BDE);
    assert_eq!(faded(0x7FFF, 31), 0x0000);
    assert_eq!(faded(0x0421, 2), 0x0000);
}

#[test]
fn a_loaded_save_state_goes_on_like_the_super_game_boy_that_saved_it() {
    use crate::serializable::Serializable;

    let mut console = Console::new();
    console.send(&pal01([
        0x1234, 0x0011, 0x0222, 0x3333, 0x4444, 0x5555, 0x6666,
    ]));
    console.send(&[header(command::ATTR_DIV, 1), 0b10_01_11, 7]);
    console.send(&[header(command::MASK_EN, 1), 1]);
    console.frame_of_shade(2);
    console.send(&[header(command::PCT_TRN, 1)]);
    console.frame_of_shade(1);

    let mut bytes = Vec::new();
    console.sgb.serialize(&mut bytes).unwrap();
    let mut loaded = Console::new();
    loaded.sgb.start(0x00, 0x00);
    loaded.sgb.deserialize(&mut bytes.as_slice()).unwrap();

    for _ in 0..BORDER_FADE_FRAMES {
        assert_eq!(console.frame_of_shade(3), loaded.frame_of_shade(3));
    }
    assert_eq!(format!("{:?}", console.sgb), format!("{:?}", loaded.sgb));
}

#[test]
fn the_intro_holds_the_game_boy_in_reset_and_then_shows_its_picture() {
    let mut console = Console::new();
    console.sgb = Sgb::new();
    for _ in 0..RESET_HOLD_FRAMES {
        assert!(console.sgb.holds_game_boy_in_reset());
        console.frame(None);
    }
    assert!(!console.sgb.holds_game_boy_in_reset());

    for _ in RESET_HOLD_FRAMES..INTRO_FRAMES {
        let game_screen = console.frame_of_shade(3);
        assert_ne!(
            pixel(&game_screen, 0, 0),
            DMG_SHADES[3],
            "the intro still shows"
        );
        assert!(console.sgb.frame_colorized());
    }
    let game_screen = console.frame_of_shade(3);
    assert_eq!(pixel(&game_screen, 0, 0), DMG_SHADES[3]);
    assert!(!console.sgb.frame_colorized());
}

/// A palette the player picks: white, red, green and blue.
const PLAYER_PALETTE: [u16; 4] = [0x7FFF, 0x001F, 0x03E0, 0x7C00];

#[test]
fn the_player_palette_colors_the_whole_screen_until_the_player_changes_it() {
    let mut console = Console::new();
    console.send(&[header(command::ATTR_DIV, 1), 0b10_01_11, 7]);
    console.sgb.set_player_palette(Some(PLAYER_PALETTE));
    let game_screen = console.frame_of_shade(1);
    assert_eq!(pixel(&game_screen, 0, 0), [0xFF, 0x00, 0x00, 0xFF]);
    assert_eq!(pixel(&game_screen, 159, 143), [0xFF, 0x00, 0x00, 0xFF]);
    assert!(console.sgb.frame_colorized());

    console.send(&pal01([0, 0x7C00, 0, 0, 0x7C00, 0, 0]));
    assert_eq!(console.sgb.player_palette(), Some(PLAYER_PALETTE));

    console.sgb.set_player_palette(None);
    let game_screen = console.frame_of_shade(1);
    assert_eq!(pixel(&game_screen, 0, 0), [0x00, 0x00, 0xFF, 0xFF]);
}

#[test]
fn with_pal_pri_on_the_game_takes_its_colors_back_when_it_sets_new_ones() {
    let mut console = Console::new();
    console.send(&[header(command::PAL_PRI, 1), 0x01]);
    console.sgb.set_player_palette(Some(PLAYER_PALETTE));
    console.send(&[header(command::PAL_TRN, 1)]);
    assert_eq!(console.sgb.player_palette(), Some(PLAYER_PALETTE));

    console.send(&pal01([0, 0x7C00, 0, 0, 0x7C00, 0, 0]));
    assert_eq!(console.sgb.player_palette(), None);

    console.sgb.set_player_palette(Some(PLAYER_PALETTE));
    console.send(&[header(command::PAL_SET, 1)]);
    assert_eq!(console.sgb.player_palette(), None);
}

#[test]
fn icon_en_takes_the_palettes_away_and_can_stop_all_commands() {
    let mut console = Console::new();
    console.sgb.set_player_palette(Some(PLAYER_PALETTE));
    console.send(&[header(command::ICON_EN, 1), 0x01]);
    assert!(!console.sgb.player_palettes_allowed());
    assert_eq!(console.sgb.player_palette(), None);
    console.sgb.set_player_palette(Some(PLAYER_PALETTE));
    assert_eq!(console.sgb.player_palette(), None);

    console.send(&[header(command::ICON_EN, 1), 0x02]);
    assert!(console.sgb.player_palettes_allowed());
    assert_eq!(console.send(&[header(command::MLT_REQ, 1), 0x03]), Some(4));

    console.send(&[header(command::ICON_EN, 1), 0x04]);
    assert_eq!(console.send(&[header(command::MLT_REQ, 1), 0x03]), None);
    console.send(&[header(command::ICON_EN, 1), 0x00]);
    assert!(console.sgb.player_palettes_allowed());
}

/// Sends a whole border whose every pixel has color 1 of the first border
/// palette, and waits until it shows.
fn send_game_border(console: &mut Console) {
    console.send(&[header(command::CHR_TRN, 1), 0]);
    for _ in 0..3 {
        console.frame_of_shade(1);
    }
    console.send(&[header(command::PCT_TRN, 1)]);
    for _ in 0..3 + usize::from(BORDER_FADE_FRAMES) {
        console.frame_of_shade(0);
    }
}

#[test]
fn the_player_can_pick_nubos_border_over_the_games() {
    let mut console = Console::new();
    console.frame_of_shade(0);
    let nubo_border = console.sgb.bordered_frame().to_vec();
    send_game_border(&mut console);
    assert_ne!(console.sgb.bordered_frame(), nubo_border);

    console.sgb.set_border_choice(SgbBorder::Nubo);
    console.frame_of_shade(0);
    assert_eq!(console.sgb.bordered_frame(), nubo_border);
    console.sgb.set_border_choice(SgbBorder::Game);
    console.frame_of_shade(0);
    assert_ne!(console.sgb.bordered_frame(), nubo_border);
}

#[test]
fn nubos_border_has_a_night_and_a_day_look() {
    let mut console = Console::new();
    console.frame_of_shade(3);
    let night = console.sgb.bordered_frame().to_vec();
    console.sgb.set_border_time(SgbBorderTime::Day);
    console.frame_of_shade(3);
    let day = console.sgb.bordered_frame().to_vec();
    assert_ne!(day, night);

    // Only the border changes: the game screen is the same.
    let (left, top) = SCREEN_OFFSET;
    for y in top..top + 144 {
        let row = (left + y * BORDERED_WIDTH) * 4..(left + 160 + y * BORDERED_WIDTH) * 4;
        assert_eq!(day[row.clone()], night[row]);
    }

    console.sgb.set_border_time(SgbBorderTime::Night);
    console.frame_of_shade(3);
    assert_eq!(console.sgb.bordered_frame(), night);
}

#[test]
fn a_games_own_border_looks_the_same_by_day() {
    let mut console = Console::new();
    send_game_border(&mut console);
    let night = console.sgb.bordered_frame().to_vec();
    console.sgb.set_border_time(SgbBorderTime::Day);
    console.frame_of_shade(0);
    assert_eq!(console.sgb.bordered_frame(), night);
}

/// Runs frames until the attraction mode has been on for a while, and
/// returns whether Nubo's border changed.
fn border_comes_alive(console: &mut Console) -> bool {
    console.frame_of_shade(0);
    let still = console.sgb.bordered_frame().to_vec();
    console.sgb.idle_frames = attraction::IDLE_FRAMES - 1;
    for _ in 0..200 {
        console.frame_of_shade(0);
    }
    console.sgb.bordered_frame() != still
}

#[test]
fn nubos_border_comes_alive_when_the_player_leaves_the_controller_alone() {
    let mut console = Console::new();
    assert!(border_comes_alive(&mut console));
    let mut console = Console::new();
    console.sgb.set_border_time(SgbBorderTime::Day);
    assert!(border_comes_alive(&mut console));

    console.sgb.set_buttons_held(true);
    console.frame_of_shade(0);
    assert_eq!(console.sgb.idle_frames, 0);
    console.sgb.set_buttons_held(false);
    console.frame_of_shade(0);
    assert_eq!(console.sgb.idle_frames, 1);
}

#[test]
fn a_held_button_keeps_the_attraction_mode_away() {
    let mut console = Console::new();
    console.sgb.set_buttons_held(true);
    assert!(!border_comes_alive(&mut console));
}

#[test]
fn atrc_en_turns_the_attraction_mode_off() {
    let mut console = Console::new();
    console.send(&[header(command::ATRC_EN, 1), 0x01]);
    assert!(!border_comes_alive(&mut console));
    console.send(&[header(command::ATRC_EN, 1), 0x00]);
    assert!(border_comes_alive(&mut console));
}

#[test]
fn a_games_border_has_no_attraction_mode() {
    let mut console = Console::new();
    send_game_border(&mut console);
    assert!(!border_comes_alive(&mut console));
}

#[test]
fn save_states_keep_what_the_game_did_but_not_the_players_choices() {
    use crate::serializable::Serializable;

    let mut console = Console::new();
    console.send(&[header(command::PAL_PRI, 1), 0x01]);
    console.send(&[header(command::ATRC_EN, 1), 0x01]);
    console.sgb.set_player_palette(Some(PLAYER_PALETTE));
    console.sgb.set_border_choice(SgbBorder::Nubo);
    console.sgb.set_border_time(SgbBorderTime::Day);
    console.frame_of_shade(0);

    let mut bytes = Vec::new();
    console.sgb.serialize(&mut bytes).unwrap();
    let mut loaded = Console::new();
    loaded.sgb.set_player_palette(Some([1, 2, 3, 4]));
    loaded.sgb.deserialize(&mut bytes.as_slice()).unwrap();
    assert_eq!(loaded.sgb.player_palette(), Some([1, 2, 3, 4]));
    assert_eq!(loaded.sgb.border_choice(), SgbBorder::Game);
    assert_eq!(loaded.sgb.border_time(), SgbBorderTime::Night);
    assert!(loaded.sgb.game_palette_priority);
    assert!(loaded.sgb.attraction_disabled);
    assert_eq!(loaded.sgb.idle_frames, 1);
}

#[test]
fn picking_a_palette_after_the_game_took_its_colors_back_leaves_the_state_alone() {
    use crate::serializable::Serializable;

    let mut console = Console::new();
    console.send(&[header(command::PAL_PRI, 1), 0x01]);
    console.send(&pal01([0, 0x7C00, 0, 0, 0x7C00, 0, 0]));
    let mut before = Vec::new();
    console.sgb.serialize(&mut before).unwrap();

    console.sgb.set_player_palette(Some(PLAYER_PALETTE));
    assert_eq!(console.sgb.player_palette(), Some(PLAYER_PALETTE));
    let mut after = Vec::new();
    console.sgb.serialize(&mut after).unwrap();
    assert_eq!(before, after);

    // A player's pick also stays when a state loads, as with run-ahead
    // every frame, until the game takes its colors back again.
    console.sgb.deserialize(&mut before.as_slice()).unwrap();
    assert_eq!(console.sgb.player_palette(), Some(PLAYER_PALETTE));
    console.send(&pal01([0, 0x7C00, 0, 0, 0x7C00, 0, 0]));
    assert_eq!(console.sgb.player_palette(), None);
}

#[test]
fn a_redraw_shows_the_menus_settings_without_moving_time_on() {
    let mut console = Console::new();
    let mut game_screen = console.frame_of_shade(1);
    assert_eq!(pixel(&game_screen, 0, 0), DMG_SHADES[1]);
    let idle_frames = console.sgb.idle_frames;

    console.sgb.set_player_palette(Some(PLAYER_PALETTE));
    console.sgb.redraw(&mut game_screen);
    assert_eq!(pixel(&game_screen, 0, 0), [0xFF, 0x00, 0x00, 0xFF]);
    assert!(console.sgb.frame_colorized());
    assert_eq!(console.sgb.idle_frames, idle_frames);

    let mut redrawn = game_screen.clone();
    console.sgb.redraw(&mut redrawn);
    assert_eq!(redrawn, game_screen);
}

#[test]
fn a_redraw_leaves_the_intro_alone() {
    let mut console = Console::new();
    console.sgb = Sgb::new();
    let mut game_screen = console.frame_of_shade(3);
    let intro = game_screen.clone();
    console.sgb.redraw(&mut game_screen);
    assert_eq!(game_screen, intro);
}

#[test]
fn a_redraw_waits_for_a_frame_after_power_on_or_a_loaded_state() {
    let mut console = Console::new();
    let mut game_screen = vec![0x12; SCREEN_PIXELS * 4];
    console.sgb.set_player_palette(Some(PLAYER_PALETTE));
    console.sgb.redraw(&mut game_screen);
    assert!(game_screen.iter().all(|&byte| byte == 0x12));

    let mut game_screen = console.frame_of_shade(1);
    console.sgb.finish_loading_save_state();
    let before = game_screen.clone();
    console.sgb.set_player_palette(None);
    console.sgb.redraw(&mut game_screen);
    assert_eq!(game_screen, before);
}
