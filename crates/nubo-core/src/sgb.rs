/*
    The commands, the colorization of the picture, the VRAM transfers and the border (with its
    fade when a game sends a new one) are ported from SameBoy's sgb.c
    (https://github.com/LIJI32/SameBoy, Expat License, Copyright (c) 2015-2026 Lior Halphon; see
    the notice in apu.rs).
*/

//! The Super Game Boy: a cartridge for the Super Nintendo (SNES) with Game
//! Boy hardware inside, which shows Game Boy games on a TV.
//!
//! The Game Boy part is an ordinary DMG (`Model::Sgb`, `Model::Sgb2`). The
//! Super Nintendo around it adds what this module emulates:
//!
//! - **Commands.** A game sends commands to the Super Nintendo through the
//!   joypad register, one bit at a time (`packets.rs`). Only games whose
//!   cartridge header says they support the Super Game Boy can (`Sgb::start`).
//! - **Colors.** The Game Boy draws in four shades of gray. The Super Nintendo
//!   colors them with four palettes of four colors (`PAL01` to `PAL12`,
//!   `PAL_SET`), picking a palette for each 8×8 square of the screen from the
//!   attribute map (`ATTR_BLK`, `ATTR_LIN`, `ATTR_DIV`, `ATTR_CHR`,
//!   `ATTR_SET`). Colors are RGB555: five bits each of red, green and blue.
//! - **Transfers.** Larger data, such as 512 palettes to pick from or the
//!   border's pictures, travels as a picture: the game draws it on the screen
//!   and sends a `_TRN` command, and a few frames later the Super Nintendo
//!   reads the screen back as tile data (`Sgb::finish_transfer`).
//! - **The border.** The TV picture is 256×224 pixels with the Game Boy's
//!   160×144 screen in the middle; a game can fill the rest with a border of
//!   its own (`CHR_TRN`, `PCT_TRN`). Until it does, Nubo draws its own
//!   (`border.rs`): the Super Game Boy's built-in border is Nintendo's
//!   artwork and isn't included.
//! - **Masking.** A game can freeze the picture or blank it while it prepares
//!   the next one (`MASK_EN`).
//! - **Multiplayer.** A game can ask for up to four controllers (`MLT_REQ`),
//!   which the joypad then reads one after another (`joypad.rs`).
//! - **The startup.** When the Super Nintendo is switched on, it holds the
//!   Game Boy in reset while it plays its intro, and shows the Game Boy's
//!   picture only once the intro is over, so a game starts later than on a
//!   Game Boy. Nubo plays an intro of its own (`intro.rs`), as long as
//!   SameBoy's, and holds the Game Boy as long as SameBoy does
//!   (`Sgb::holds_game_boy_in_reset`); neither length is measured on
//!   hardware yet. The intro is silent.
//!
//! - **The menu.** The player can recolor the game with a palette of their
//!   own and pick Nubo's border over the game's (a real Super Game Boy has a
//!   menu for this, opened with the L and R buttons; Nubo's apps draw their
//!   own). A game can take the palettes away (`ICON_EN`), take its colors
//!   back whenever it sets new ones (`PAL_PRI`), and stop all further
//!   commands (`ICON_EN`). Left alone for seven minutes, Nubo's border plays
//!   a screensaver (`attraction.rs`), unless the game turned it off
//!   (`ATRC_EN`). Save states keep what the game did but not the player's
//!   choices, which stay as they are when a state loads, like the buttons
//!   held: a loaded state shows the player's palette and border too, unless
//!   the game had taken its colors back by then.
//!
//! Not emulated: the Super Nintendo's sound (`SOUND`, `SOU_TRN`), programs a
//! game sends to the Super Nintendo's own processor (`DATA_SND`, `DATA_TRN`,
//! `JUMP`), and the built-in palettes and borders, which are Nintendo's.
//! `OBJ_TRN` and `TEST_EN` do nothing on a retail Super Game Boy, so they do
//! nothing here either.
//!
//! Until a game sets colors of its own, the picture keeps the Game Boy's
//! grays (`DMG_SHADES`), so frontends show it in the player's palette like a
//! DMG game (`Sgb::frame_colorized`), as a Super Game Boy shows such games in
//! the palette the player picks from its menu.
//!
//! The PPU owns the Super Game Boy and hands it each frame when it finishes
//! (`Gpu::publish_frame`), which is exactly when the Super Nintendo sees it:
//! the PPU is never left behind past the end of a frame. The address bus hands
//! it every JOYP write.

use crate::gpu::{DMG_SHADES, rgb555_to_rgba};
use crate::sgb::border::{Border, BorderCache, BorderSource, NuboBorder};
use crate::sgb::intro::Intro;
use crate::sgb::packets::{MAX_COMMAND_SIZE, PacketReceiver, is_header_packet};
use nubo_serializable_derive::Serializable;

/// The Game Boy screen's width in pixels.
const SCREEN_WIDTH: usize = 160;
/// The Game Boy screen's height in pixels.
const SCREEN_HEIGHT: usize = 144;
/// The pixels on the Game Boy screen.
const SCREEN_PIXELS: usize = SCREEN_WIDTH * SCREEN_HEIGHT;
/// The width of the picture with the border, in pixels.
pub(crate) const BORDERED_WIDTH: usize = 256;
/// The height of the picture with the border, in pixels.
pub(crate) const BORDERED_HEIGHT: usize = 224;
/// Where the Game Boy screen's top-left corner is in the picture with the
/// border.
pub(crate) const SCREEN_OFFSET: (usize, usize) = (48, 40);
/// The attribute map's width: one palette for each 8×8 square.
const ATTRIBUTE_COLUMNS: usize = SCREEN_WIDTH / 8;
/// The attribute map's height.
const ATTRIBUTE_ROWS: usize = SCREEN_HEIGHT / 8;
/// The squares in the attribute map.
const ATTRIBUTE_SQUARES: usize = ATTRIBUTE_COLUMNS * ATTRIBUTE_ROWS;
/// The bytes of one attribute file: the attribute map, four squares a byte.
const ATTRIBUTE_FILE_SIZE: usize = ATTRIBUTE_SQUARES / 4;
/// How many attribute files `ATTR_TRN` sends.
const ATTRIBUTE_FILE_COUNT: usize = 45;
/// The bytes `ATTR_TRN` sends: 254 tiles of 16 bytes, which hold the 45
/// attribute files and some unused bytes after them.
const ATTRIBUTE_TRANSFER_SIZE: usize = 0xFE * 16;
/// The bytes of the palettes `PAL_TRN` sends: 512 palettes of four RGB555
/// colors, two bytes each.
const SYSTEM_PALETTES_SIZE: usize = 512 * 4 * 2;
/// How many frames after a `_TRN` command the Super Nintendo reads the
/// screen (SameBoy).
const TRANSFER_DELAY_FRAMES: u8 = 3;
/// The border's fade when a game sends a new one, in frames: the old border
/// stays for 41 frames, fades out over 32, and the new one fades in over 32.
/// Measured on an SGB2 by SameBoy, and maybe off by two frames.
const BORDER_FADE_FRAMES: u8 = 105;
/// How many frames the intro lasts, from power-on, before the Game Boy's
/// picture shows (SameBoy: 210).
const INTRO_FRAMES: u16 = 210;
/// How many frames from power-on the Game Boy is held in reset (SameBoy: 106,
/// until "the boot animation is almost done").
const RESET_HOLD_FRAMES: u16 = 106;
/// The four grays of `DMG_SHADES` as RGB555, the colors a palette has until a
/// game sets it.
const GRAY_PALETTE: [u16; 4] = [0x7FFF, 0x56B5, 0x294A, 0x0000];

/// The Super Game Boy's commands Nubo carries out, by number (the top five
/// bits of a command's first byte).
mod command {
    pub(super) const PAL01: u8 = 0x00;
    pub(super) const PAL23: u8 = 0x01;
    pub(super) const PAL03: u8 = 0x02;
    pub(super) const PAL12: u8 = 0x03;
    pub(super) const ATTR_BLK: u8 = 0x04;
    pub(super) const ATTR_LIN: u8 = 0x05;
    pub(super) const ATTR_DIV: u8 = 0x06;
    pub(super) const ATTR_CHR: u8 = 0x07;
    pub(super) const PAL_SET: u8 = 0x0A;
    pub(super) const PAL_TRN: u8 = 0x0B;
    pub(super) const ATRC_EN: u8 = 0x0C;
    pub(super) const ICON_EN: u8 = 0x0E;
    pub(super) const MLT_REQ: u8 = 0x11;
    pub(super) const CHR_TRN: u8 = 0x13;
    pub(super) const PCT_TRN: u8 = 0x14;
    pub(super) const ATTR_TRN: u8 = 0x15;
    pub(super) const ATTR_SET: u8 = 0x16;
    pub(super) const MASK_EN: u8 = 0x17;
    pub(super) const PAL_PRI: u8 = 0x19;
}

/// Which border a Super Game Boy shows, as the player picks it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SgbBorder {
    /// The game's own border, or Nubo's until the game sends one.
    #[default]
    Game,
    /// Nubo's border, even when the game sent one of its own.
    Nubo,
}

/// What `MASK_EN` does to the picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serializable)]
enum Mask {
    /// The picture shows what the Game Boy draws.
    #[default]
    Off,
    /// The picture keeps showing the last frame before the mask.
    Freeze,
    /// The picture is black.
    Black,
    /// The picture is filled with color 0.
    Color0,
}

/// Where the screen a `_TRN` command sends goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serializable)]
enum TransferDestination {
    /// The first 128 of the border's 256 tiles (`CHR_TRN`).
    #[default]
    LowBorderTiles,
    /// The other 128 border tiles.
    HighBorderTiles,
    /// The border's tile map and its palettes (`PCT_TRN`).
    BorderMap,
    /// The 512 palettes `PAL_SET` picks from (`PAL_TRN`).
    SystemPalettes,
    /// The 45 attribute maps `ATTR_SET` picks from (`ATTR_TRN`).
    AttributeFiles,
}

/// A `_TRN` command waiting for the frame in which the Super Nintendo reads
/// the screen.
#[derive(Clone, Copy, Debug, Default, Serializable)]
struct Transfer {
    /// Frames left until the screen is read.
    frames_left: u8,
    /// Where the screen goes.
    destination: TransferDestination,
}

/// The Super Game Boy's side of the console: everything the Super Nintendo
/// keeps for the Game Boy game (see the module docs).
///
/// Save states keep all of it but the finished pictures, which are the
/// frontend's side of the hand-over like `Gpu::completed_frame`, the
/// caches, and the player's choices from the menu and the buttons held.
#[derive(Clone, Debug, Serializable)]
pub(crate) struct Sgb {
    /// The command arriving through JOYP.
    receiver: PacketReceiver,
    /// Whether the cartridge header says the game supports the Super Game
    /// Boy. Other games' commands are ignored.
    commands_enabled: bool,
    /// Whether a game has set colors. Until then the picture keeps the
    /// Game Boy's grays.
    colorized: bool,
    /// The four palettes the picture is colored with, as little-endian
    /// RGB555. Color 0 is shared: the first palette's color 0 is used for
    /// all four.
    palettes: [u8; 32],
    /// The 512 palettes `PAL_SET` picks from, which `PAL_TRN` sends.
    system_palettes: [u8; SYSTEM_PALETTES_SIZE],
    /// Which palette each 8×8 square of the screen uses, row by row.
    attribute_map: [u8; ATTRIBUTE_SQUARES],
    /// The attribute maps `ATTR_SET` picks from, which `ATTR_TRN` sends.
    attribute_files: [u8; ATTRIBUTE_TRANSFER_SIZE],
    mask: Mask,
    transfer: Option<Transfer>,
    /// The border on the picture.
    border: Border,
    /// The border `CHR_TRN` and `PCT_TRN` are sending, which replaces
    /// `border` halfway through the fade.
    pending_border: Border,
    /// Frames left in the fade to a new border, 0 when there is none.
    border_fade: u8,
    /// The shades (0 to 3) of the last frame the Game Boy drew, which the
    /// transfers read.
    screen: [u8; SCREEN_PIXELS],
    /// The shades the picture shows: `screen`, unless the mask froze it.
    shown_screen: [u8; SCREEN_PIXELS],
    /// How many frames of the intro have been shown since power-on, up to
    /// `INTRO_FRAMES`.
    intro_frames: u16,
    /// Whether the game took away the palettes the player can pick from the
    /// menu (`ICON_EN` bit 0).
    player_palettes_disabled: bool,
    /// Whether the game takes its colors back from the player's palette
    /// whenever it sets new ones (`PAL_PRI`).
    game_palette_priority: bool,
    /// Whether the game turned the attraction mode off (`ATRC_EN`).
    attraction_disabled: bool,
    /// Whether the game took its colors back from the player's palette
    /// (`PAL_PRI`, or `ICON_EN` taking the palettes away), until the player
    /// picks a palette again.
    palette_taken_back: bool,
    /// The palette the player picked from the menu, four RGB555 colors from
    /// the lightest shade to the darkest, or `None` for the game's colors.
    /// Save states leave it out, like `border_choice`: the player's choices
    /// stay as they are when a state loads, as the buttons held do.
    #[serializable(skip)]
    player_palette: Option<[u16; 4]>,
    /// The border the player picked from the menu.
    #[serializable(skip)]
    border_choice: SgbBorder,
    /// How many frames since the intro the player has held no button. The
    /// attraction mode starts at `attraction::IDLE_FRAMES`.
    idle_frames: u32,
    /// Whether the player holds a button now, which the address bus passes
    /// on. Save states leave it out, as they leave out the buttons in the
    /// joypad: they belong to the player rather than the console.
    #[serializable(skip)]
    buttons_held: bool,
    /// Nubo's intro, drawn once.
    #[serializable(skip)]
    intro: Intro,
    /// Nubo's own border, drawn once.
    #[serializable(skip)]
    nubo_border: NuboBorder,
    /// The border as last drawn, so it is only drawn again when it changes.
    #[serializable(skip)]
    border_cache: BorderCache,
    /// Whether the game screen in the last picture uses colors other than
    /// the four `DMG_SHADES`: the intro's, a game's own, or the palette the
    /// player picked from the menu.
    #[serializable(skip)]
    frame_colorized: bool,
    /// Whether the last picture is a frame of the game that `redraw` can
    /// draw again: not during the intro, and not until a frame was drawn
    /// since power-on or since a save state was loaded, whose picture this
    /// one's fields don't describe.
    #[serializable(skip)]
    redrawable: bool,
    /// How much darker the border's fade made the last picture's border, in
    /// steps of 1/31 of full brightness (`next_border_fade`).
    #[serializable(skip)]
    shown_border_fade: u8,
    /// The last picture with its border, 256×224 RGBA.
    #[serializable(skip)]
    bordered_frame: Vec<u8>,
}

impl Default for Sgb {
    fn default() -> Self {
        Sgb::new()
    }
}

impl Sgb {
    /// A Super Game Boy as it is switched on, showing Nubo's border.
    pub(crate) fn new() -> Self {
        let mut palettes = [0; 32];
        for (index, color) in palettes.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            *color = GRAY_PALETTE[index % 4].to_le_bytes();
        }
        Sgb {
            receiver: PacketReceiver::new(),
            commands_enabled: false,
            colorized: false,
            palettes,
            system_palettes: [0; SYSTEM_PALETTES_SIZE],
            attribute_map: [0; ATTRIBUTE_SQUARES],
            attribute_files: [0; ATTRIBUTE_TRANSFER_SIZE],
            mask: Mask::Off,
            transfer: None,
            border: Border::default(),
            pending_border: Border::default(),
            border_fade: 0,
            screen: [0; SCREEN_PIXELS],
            shown_screen: [0; SCREEN_PIXELS],
            intro_frames: 0,
            player_palettes_disabled: false,
            game_palette_priority: false,
            attraction_disabled: false,
            palette_taken_back: false,
            player_palette: None,
            border_choice: SgbBorder::Game,
            idle_frames: 0,
            buttons_held: false,
            intro: Intro::new(),
            nubo_border: NuboBorder::new(),
            border_cache: BorderCache::default(),
            frame_colorized: false,
            redrawable: false,
            shown_border_fade: 0,
            bordered_frame: vec![0; BORDERED_WIDTH * BORDERED_HEIGHT * 4],
        }
    }

    /// Called when the boot ROM hands over to the game, after the real one
    /// has sent the cartridge header to the Super Nintendo. Commands only
    /// work for games whose header says they support the Super Game Boy: an
    /// SGB flag (0x0146) of 0x03 and an old licensee code (0x014B) of 0x33.
    pub(crate) fn start(&mut self, sgb_flag: u8, old_licensee_code: u8) {
        self.commands_enabled = sgb_flag == 0x03 && old_licensee_code == 0x33;
    }

    /// Whether the Super Nintendo still holds the Game Boy in reset: its CPU
    /// and timers stand still, and only its (switched off) PPU and APU count
    /// time.
    pub(crate) fn holds_game_boy_in_reset(&self) -> bool {
        self.intro_frames < RESET_HOLD_FRAMES
    }

    /// Called after a save state was loaded: the last picture is the old
    /// run's, so `redraw` waits for the next frame.
    pub(crate) fn finish_loading_save_state(&mut self) {
        self.redrawable = false;
    }

    /// Ends the intro, for a save state made by a version without one, of a
    /// game already past the boot ROM: its state lacks the intro's frames,
    /// which would otherwise start again from power-on.
    pub(crate) fn finish_intro(&mut self) {
        self.intro_frames = INTRO_FRAMES;
    }

    /// Whether the intro has shown all its frames.
    #[cfg(test)]
    pub(crate) fn intro_over(&self) -> bool {
        self.intro_frames >= INTRO_FRAMES
    }

    /// Starts the intro again, as a state without its frames would.
    #[cfg(test)]
    pub(crate) fn restart_intro_for_tests(&mut self) {
        self.intro_frames = 0;
    }

    /// The palette the player picked, while it colors the screen: not
    /// while the game has taken the palettes away or its colors back.
    pub(crate) fn player_palette(&self) -> Option<[u16; 4]> {
        self.player_palette
            .filter(|_| !self.palette_taken_back && !self.player_palettes_disabled)
    }

    /// The player picks `palette`, four RGB555 colors from lightest to
    /// darkest shade, to color the whole screen with, or the game's colors
    /// with `None`. Picking a palette takes it back from a game that took
    /// its colors back, unless the game has taken the palettes away
    /// (`player_palettes_allowed`).
    pub(crate) fn set_player_palette(&mut self, palette: Option<[u16; 4]>) {
        self.player_palette = palette;
        if palette.is_some() && !self.player_palettes_disabled {
            self.palette_taken_back = false;
        }
    }

    /// Whether the game lets the player pick a palette (`ICON_EN`).
    pub(crate) fn player_palettes_allowed(&self) -> bool {
        !self.player_palettes_disabled
    }

    /// The border the player picked.
    pub(crate) fn border_choice(&self) -> SgbBorder {
        self.border_choice
    }

    /// Picks the border to show.
    pub(crate) fn set_border_choice(&mut self, border: SgbBorder) {
        self.border_choice = border;
    }

    /// Tells whether the player holds any button now. A press ends the
    /// attraction mode and starts the wait for it again.
    ///
    /// Only the end of a frame reads what this sets, and the PPU, which hands
    /// over the frames, is never left behind past one, so a lazily run PPU
    /// sees the same as one run M-cycle by M-cycle.
    pub(crate) fn set_buttons_held(&mut self, held: bool) {
        self.buttons_held = held;
        if held {
            self.idle_frames = 0;
        }
    }

    /// Whether the player holds a button, as `set_buttons_held` last said.
    #[cfg(test)]
    pub(crate) fn buttons_held_for_tests(&self) -> bool {
        self.buttons_held
    }

    /// Whether the game screen in the last picture uses colors other than
    /// the four `DMG_SHADES` (apart from where a game's border covers it):
    /// during the intro, once a game sets colors, or while the player's
    /// palette from the menu applies.
    pub(crate) fn frame_colorized(&self) -> bool {
        self.frame_colorized
    }

    /// The last picture with its border: 256×224 RGBA pixels, row by row.
    pub(crate) fn bordered_frame(&self) -> &[u8] {
        &self.bordered_frame
    }

    /// Sees a write of `value` to JOYP, whose group select bits were
    /// `previous_select`. Returns the number of players an `MLT_REQ` command
    /// finished by this write asks for, which the joypad then reads.
    pub(crate) fn write_joyp(&mut self, previous_select: u8, value: u8) -> Option<u8> {
        if !self.commands_enabled {
            return None;
        }
        let command = self.receiver.receive(previous_select, value)?;
        self.run_command(&command)
    }

    /// Carries out a command that has arrived. Returns the number of players
    /// `MLT_REQ` asks for.
    fn run_command(&mut self, command: &[u8; MAX_COMMAND_SIZE]) -> Option<u8> {
        if is_header_packet(command[0]) || command[0] & 0x07 == 0 {
            return None;
        }
        match command[0] >> 3 {
            command::PAL01 => self.set_two_palettes(command, 0, 1),
            command::PAL23 => self.set_two_palettes(command, 2, 3),
            command::PAL03 => self.set_two_palettes(command, 0, 3),
            command::PAL12 => self.set_two_palettes(command, 1, 2),
            command::ATTR_BLK => self.attribute_blocks(command),
            command::ATTR_LIN => self.attribute_lines(command),
            command::ATTR_DIV => self.attribute_division(command),
            command::ATTR_CHR => self.attribute_squares(command),
            command::PAL_SET => self.set_palettes_from_system(command),
            command::PAL_TRN => self.start_transfer(TransferDestination::SystemPalettes),
            command::ATRC_EN => self.attraction_disabled = command[1] & 0x01 != 0,
            command::ICON_EN => {
                self.player_palettes_disabled = command[1] & 0x01 != 0;
                if self.player_palettes_disabled {
                    self.palette_taken_back = true;
                }
                // Bit 1 bars the menu's controller setup, which Nubo's menu
                // doesn't have: the apps' own controls settings serve
                // instead. Bit 2 ignores every command after this one.
                if command[1] & 0x04 != 0 {
                    self.commands_enabled = false;
                }
            }
            command::MLT_REQ => return Some((command[1] & 0x03) + 1),
            command::CHR_TRN => self.start_transfer(if command[1] & 0x01 == 0 {
                TransferDestination::LowBorderTiles
            } else {
                TransferDestination::HighBorderTiles
            }),
            command::PCT_TRN => self.start_transfer(TransferDestination::BorderMap),
            command::ATTR_TRN => self.start_transfer(TransferDestination::AttributeFiles),
            command::ATTR_SET => {
                self.load_attribute_file(command[1] & 0x3F);
                if command[1] & 0x40 != 0 {
                    self.mask = Mask::Off;
                }
            }
            command::MASK_EN => {
                self.mask = match command[1] & 0x03 {
                    0 => Mask::Off,
                    1 => Mask::Freeze,
                    2 => Mask::Black,
                    _ => Mask::Color0,
                };
            }
            command::PAL_PRI => self.game_palette_priority = command[1] & 0x01 != 0,
            _ => {}
        }
        None
    }

    /// The color at `index` (palette × 4 + color) of `palettes`.
    fn palette_color(&self, index: usize) -> u16 {
        u16::from_le_bytes([self.palettes[index * 2], self.palettes[index * 2 + 1]])
    }

    /// Called when the game sets new colors: with `PAL_PRI` on, they replace
    /// the player's palette.
    fn game_sets_colors(&mut self) {
        self.colorized = true;
        if self.game_palette_priority {
            self.palette_taken_back = true;
        }
    }

    /// Sets the color at `index` (palette × 4 + color) of `palettes`.
    fn set_palette_color(&mut self, index: usize, color: u16) {
        self.palettes[index * 2..index * 2 + 2].copy_from_slice(&color.to_le_bytes());
    }

    /// `PAL01`, `PAL23`, `PAL03` and `PAL12`: sets the shared color 0 and
    /// colors 1 to 3 of two palettes.
    fn set_two_palettes(&mut self, command: &[u8; MAX_COMMAND_SIZE], first: usize, second: usize) {
        self.game_sets_colors();
        let color = |offset: usize| u16::from_le_bytes([command[offset], command[offset + 1]]);
        for palette in 0..4 {
            self.set_palette_color(palette * 4, color(1));
        }
        for index in 0..3 {
            self.set_palette_color(first * 4 + index + 1, color(3 + index * 2));
            self.set_palette_color(second * 4 + index + 1, color(9 + index * 2));
        }
    }

    /// `PAL_SET`: copies four of the 512 system palettes into the palettes,
    /// and then maybe loads an attribute file and lifts the mask.
    fn set_palettes_from_system(&mut self, command: &[u8; MAX_COMMAND_SIZE]) {
        self.game_sets_colors();
        for palette in 0..4 {
            let number = usize::from(command[1 + palette * 2])
                | usize::from(command[2 + palette * 2] & 0x01) << 8;
            let start = number * 8;
            self.palettes[palette * 8..palette * 8 + 8]
                .copy_from_slice(&self.system_palettes[start..start + 8]);
        }
        let color_0 = self.palette_color(0);
        for palette in 1..4 {
            self.set_palette_color(palette * 4, color_0);
        }
        if command[9] & 0x80 != 0 {
            self.load_attribute_file(command[9] & 0x3F);
        }
        if command[9] & 0x40 != 0 {
            self.mask = Mask::Off;
        }
    }

    /// Sets the palette of the square at `column`, `row`.
    fn set_attribute(&mut self, column: usize, row: usize, palette: u8) {
        self.attribute_map[column + row * ATTRIBUTE_COLUMNS] = palette & 0x03;
    }

    /// `ATTR_BLK`: colors up to 18 rectangles of squares. Each sets the
    /// squares inside it, on its edge and outside it to their own palettes,
    /// or leaves them. Setting only the inside or only the outside sets the
    /// edge too.
    fn attribute_blocks(&mut self, command: &[u8; MAX_COMMAND_SIZE]) {
        let count = usize::from(command[1]);
        if count > 0x12 {
            return;
        }
        for block in command[2..].as_chunks::<6>().0.iter().take(count) {
            let (inside, mut edge, outside) = (
                block[0] & 0x01 != 0,
                block[0] & 0x02 != 0,
                block[0] & 0x04 != 0,
            );
            let inside_palette = block[1] & 0x03;
            let mut edge_palette = (block[1] >> 2) & 0x03;
            let outside_palette = (block[1] >> 4) & 0x03;
            if inside && !edge && !outside {
                edge = true;
                edge_palette = inside_palette;
            } else if outside && !edge && !inside {
                edge = true;
                edge_palette = outside_palette;
            }
            let [left, top, right, bottom] =
                [block[2], block[3], block[4], block[5]].map(|value| usize::from(value & 0x1F));

            for row in 0..ATTRIBUTE_ROWS {
                for column in 0..ATTRIBUTE_COLUMNS {
                    if column < left || column > right || row < top || row > bottom {
                        if outside {
                            self.set_attribute(column, row, outside_palette);
                        }
                    } else if column > left && column < right && row > top && row < bottom {
                        if inside {
                            self.set_attribute(column, row, inside_palette);
                        }
                    } else if edge {
                        self.set_attribute(column, row, edge_palette);
                    }
                }
            }
        }
    }

    /// `ATTR_LIN`: sets whole rows or columns of squares to a palette.
    fn attribute_lines(&mut self, command: &[u8; MAX_COMMAND_SIZE]) {
        let count = usize::from(command[1]);
        if count > MAX_COMMAND_SIZE - 2 {
            return;
        }
        for &line in &command[2..2 + count] {
            let palette = (line >> 5) & 0x03;
            let number = usize::from(line & 0x1F);
            if line & 0x80 != 0 {
                if number < ATTRIBUTE_ROWS {
                    for column in 0..ATTRIBUTE_COLUMNS {
                        self.set_attribute(column, number, palette);
                    }
                }
            } else if number < ATTRIBUTE_COLUMNS {
                for row in 0..ATTRIBUTE_ROWS {
                    self.set_attribute(number, row, palette);
                }
            }
        }
    }

    /// `ATTR_DIV`: divides the screen at a row or column of squares, with one
    /// palette before it, one on it and one after it.
    fn attribute_division(&mut self, command: &[u8; MAX_COMMAND_SIZE]) {
        let after_palette = command[1] & 0x03;
        let before_palette = (command[1] >> 2) & 0x03;
        let on_palette = (command[1] >> 4) & 0x03;
        let horizontal = command[1] & 0x40 != 0;
        let line = usize::from(command[2] & 0x1F);

        for row in 0..ATTRIBUTE_ROWS {
            for column in 0..ATTRIBUTE_COLUMNS {
                let position = if horizontal { row } else { column };
                let palette = match position.cmp(&line) {
                    std::cmp::Ordering::Less => before_palette,
                    std::cmp::Ordering::Equal => on_palette,
                    std::cmp::Ordering::Greater => after_palette,
                };
                self.set_attribute(column, row, palette);
            }
        }
    }

    /// `ATTR_CHR`: sets squares one after another, from a starting square
    /// along rows or down columns, two bits each.
    fn attribute_squares(&mut self, command: &[u8; MAX_COMMAND_SIZE]) {
        let (mut column, mut row) = (usize::from(command[1]), usize::from(command[2]));
        let count = usize::from(u16::from_le_bytes([command[3], command[4]]));
        let down_columns = command[5] != 0;
        if column >= ATTRIBUTE_COLUMNS || row >= ATTRIBUTE_ROWS {
            return;
        }
        let data = &command[6..];
        for index in 0..count.min(data.len() * 4) {
            let palette = data[index / 4] >> (6 - (index % 4) * 2);
            self.set_attribute(column, row, palette);
            if down_columns {
                row += 1;
                if row == ATTRIBUTE_ROWS {
                    row = 0;
                    column += 1;
                    if column == ATTRIBUTE_COLUMNS {
                        break;
                    }
                }
            } else {
                column += 1;
                if column == ATTRIBUTE_COLUMNS {
                    column = 0;
                    row += 1;
                    if row == ATTRIBUTE_ROWS {
                        break;
                    }
                }
            }
        }
    }

    /// Copies attribute file `number` into the attribute map. Numbers past
    /// the 45 files do nothing.
    fn load_attribute_file(&mut self, number: u8) {
        let number = usize::from(number);
        if number >= ATTRIBUTE_FILE_COUNT {
            return;
        }
        let file =
            &self.attribute_files[number * ATTRIBUTE_FILE_SIZE..(number + 1) * ATTRIBUTE_FILE_SIZE];
        for (index, square) in self.attribute_map.iter_mut().enumerate() {
            *square = (file[index / 4] >> (6 - (index % 4) * 2)) & 0x03;
        }
    }

    /// Starts a `_TRN` command: the screen is read a few frames from now.
    fn start_transfer(&mut self, destination: TransferDestination) {
        self.transfer = Some(Transfer {
            frames_left: TRANSFER_DELAY_FRAMES,
            destination,
        });
    }

    /// Called at the start of vblank, when the Game Boy has finished a frame:
    /// `frame` is the frame as RGBA in `DMG_SHADES`, or `None` when the LCD
    /// didn't draw one (it was off, or the CPU stopped it), in which case the
    /// Super Nintendo keeps showing the last one. Moves transfers and the
    /// border's fade on, and draws the picture: the game screen into
    /// `game_screen` (160×144 RGBA) and the picture with its border into
    /// `bordered_frame`.
    pub(crate) fn finish_frame(&mut self, frame: Option<&[u8]>, game_screen: &mut [u8]) {
        if let Some(frame) = frame {
            for (shade, pixel) in self.screen.iter_mut().zip(frame.as_chunks::<4>().0) {
                *shade = shade_of(*pixel);
            }
        }

        if let Some(transfer) = &mut self.transfer {
            transfer.frames_left = transfer.frames_left.saturating_sub(1);
            if transfer.frames_left == 0 {
                let destination = transfer.destination;
                self.transfer = None;
                self.finish_transfer(destination);
            }
        }

        if self.mask != Mask::Freeze && frame.is_some() {
            self.shown_screen = self.screen;
        }

        let in_intro = self.intro_frames < INTRO_FRAMES;
        self.redrawable = !in_intro;
        let colors = self.colors();
        if in_intro {
            self.intro_frames += 1;
            self.intro.draw(self.intro_frames, game_screen);
            self.frame_colorized = true;
        } else {
            self.idle_frames = if self.buttons_held {
                0
            } else {
                self.idle_frames.saturating_add(1)
            };
            self.draw_game_screen(&colors, game_screen);
            self.frame_colorized = self.colorized || self.player_palette().is_some();
        }
        self.shown_border_fade = self.next_border_fade();
        self.draw_border(&colors, game_screen, in_intro);
    }

    /// Draws the last picture again with the menu's settings as they are
    /// now, without moving time on, so a change made from the menu shows
    /// while the game is paused. The intro, and a picture from before a
    /// save state was loaded, are left as they are.
    pub(crate) fn redraw(&mut self, game_screen: &mut [u8]) {
        if !self.redrawable {
            return;
        }
        let colors = self.colors();
        self.draw_game_screen(&colors, game_screen);
        self.frame_colorized = self.colorized || self.player_palette().is_some();
        self.draw_border(&colors, game_screen, false);
    }

    /// Reads the screen as tile data into the transfer's destination: each
    /// 8×8 square, row by row, becomes a tile of 16 bytes in the Game Boy's
    /// own tile format, two bytes for each row of pixels.
    fn finish_transfer(&mut self, destination: TransferDestination) {
        let destination: &mut [u8] = match destination {
            TransferDestination::LowBorderTiles => &mut self.pending_border.tiles[..0x1000],
            TransferDestination::HighBorderTiles => &mut self.pending_border.tiles[0x1000..],
            TransferDestination::BorderMap => {
                self.pending_border.from_game = true;
                self.border_fade = BORDER_FADE_FRAMES;
                &mut self.pending_border.map
            }
            TransferDestination::SystemPalettes => &mut self.system_palettes,
            TransferDestination::AttributeFiles => &mut self.attribute_files,
        };
        for (tile, bytes) in destination.as_chunks_mut::<16>().0.iter_mut().enumerate() {
            let (left, top) = (
                (tile % ATTRIBUTE_COLUMNS) * 8,
                (tile / ATTRIBUTE_COLUMNS) * 8,
            );
            for (row, row_bytes) in bytes.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                let start = left + (top + row) * SCREEN_WIDTH;
                let (mut low, mut high) = (0, 0);
                for (x, &shade) in self.screen[start..start + 8].iter().enumerate() {
                    low |= (shade & 0x01) << (7 - x);
                    high |= ((shade >> 1) & 0x01) << (7 - x);
                }
                *row_bytes = [low, high];
            }
        }
    }

    /// The 16 colors of the four palettes as RGBA: the player's palette in
    /// all four while it is active, and otherwise the game's. Until a game
    /// sets colors they are the `DMG_SHADES`.
    fn colors(&self) -> [[u8; 4]; 16] {
        if let Some(palette) = self.player_palette() {
            std::array::from_fn(|index| rgb555_to_rgba(palette[index % 4]))
        } else if self.colorized {
            std::array::from_fn(|index| rgb555_to_rgba(self.palette_color(index)))
        } else {
            std::array::from_fn(|index| DMG_SHADES[index % 4])
        }
    }

    /// Draws the game screen into `game_screen`, colored or masked.
    fn draw_game_screen(&self, colors: &[[u8; 4]; 16], game_screen: &mut [u8]) {
        let pixels = game_screen.as_chunks_mut::<4>().0;
        match self.mask {
            Mask::Off | Mask::Freeze => {
                let rows = pixels
                    .as_chunks_mut::<SCREEN_WIDTH>()
                    .0
                    .iter_mut()
                    .zip(self.shown_screen.as_chunks::<SCREEN_WIDTH>().0);
                for (y, (row, shades)) in rows.enumerate() {
                    let palettes = &self.attribute_map[y / 8 * ATTRIBUTE_COLUMNS..];
                    let squares = row.as_chunks_mut::<8>().0.iter_mut();
                    for ((square, shades), &palette) in
                        squares.zip(shades.as_chunks::<8>().0).zip(palettes)
                    {
                        let palette_colors = &colors[usize::from(palette & 0x03) * 4..];
                        for (pixel, &shade) in square.iter_mut().zip(shades) {
                            *pixel = palette_colors[usize::from(shade & 0x03)];
                        }
                    }
                }
            }
            Mask::Black => pixels.fill(rgb555_to_rgba(0)),
            Mask::Color0 => pixels.fill(colors[0]),
        }
    }

    /// Draws the picture with its border into `bordered_frame`: the border,
    /// with the game screen showing through it in the middle and color 0
    /// where the border has none. The border may also cover parts of the game
    /// screen, which `game_screen` then shows too.
    ///
    /// The border only changes with the fade, a new border or a new color 0,
    /// so only then is it drawn again (`BorderCache`); otherwise only the game
    /// screen is copied in.
    fn draw_border(&mut self, colors: &[[u8; 4]; 16], game_screen: &mut [u8], in_intro: bool) {
        // The intro shows the border at full brightness while a fade runs on
        // (SameBoy), and so does Nubo's border when the player picked it over
        // the game's.
        let fade = if in_intro || self.border_choice == SgbBorder::Nubo {
            0
        } else {
            self.shown_border_fade
        };
        let source = if self.border.from_game && self.border_choice == SgbBorder::Game {
            BorderSource::Game(&self.border)
        } else {
            let attraction = !in_intro
                && !self.attraction_disabled
                && self.idle_frames >= attraction::IDLE_FRAMES;
            BorderSource::Nubo {
                attraction_frame: attraction.then(|| self.idle_frames - attraction::IDLE_FRAMES),
            }
        };
        let cache = &mut self.border_cache;
        cache.update(source, &self.nubo_border);

        let screen_pixels = game_screen.as_chunks_mut::<4>().0;
        for &(index, color) in &cache.screen_cover {
            screen_pixels[usize::from(index)] = rgb555_to_rgba(faded(color, fade));
        }

        let output = self.bordered_frame.as_chunks_mut::<4>().0;
        if cache.drawn_with != Some((fade, colors[0])) {
            for (pixel, color) in output.iter_mut().zip(cache.picture.pixels()) {
                *pixel = match color {
                    Some(color) => rgb555_to_rgba(faded(color, fade)),
                    None => colors[0],
                };
            }
            cache.drawn_with = Some((fade, colors[0]));
        }

        let (screen_left, screen_top) = SCREEN_OFFSET;
        for (y, row) in screen_pixels
            .as_chunks::<SCREEN_WIDTH>()
            .0
            .iter()
            .enumerate()
        {
            let start = screen_left + (screen_top + y) * BORDERED_WIDTH;
            output[start..start + SCREEN_WIDTH].copy_from_slice(row);
        }
    }

    /// Moves the border's fade on by a frame and returns how much darker the
    /// border is drawn this frame, in steps of 1/31 of full brightness.
    /// Halfway through, the new border replaces the old one.
    fn next_border_fade(&mut self) -> u8 {
        let fade = match self.border_fade {
            0 => 0,
            65.. => {
                self.border_fade -= 1;
                0
            }
            33..=64 => {
                self.border_fade -= 1;
                64 - self.border_fade
            }
            1..=32 => {
                self.border_fade -= 1;
                self.border_fade
            }
        };
        if self.border_fade == 32 {
            self.border = self.pending_border.clone();
        }
        fade
    }
}

/// The shade (0 to 3) of a pixel the PPU drew in `DMG_SHADES`.
fn shade_of(pixel: [u8; 4]) -> u8 {
    match pixel[0] {
        0xFF => 0,
        0xAA => 1,
        0x55 => 2,
        _ => 3,
    }
}

/// Darkens an RGB555 color by `fade` steps in each channel, as the Super
/// Nintendo's screen brightness does.
fn faded(color: u16, fade: u8) -> u16 {
    if fade == 0 {
        return color;
    }
    let channel = |shift: u16| ((color >> shift) & 0x1F).saturating_sub(u16::from(fade));
    channel(0) | channel(5) << 5 | channel(10) << 10
}

mod attraction;
mod border;
mod intro;
mod packets;
#[cfg(test)]
mod tests;
