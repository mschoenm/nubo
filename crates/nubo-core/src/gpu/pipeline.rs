/*
    The mode 3 pixel pipeline (background/window fetcher, pixel FIFOs, object fetches and the window
    quirks) is ported from SameBoy's display code (https://github.com/LIJI32/SameBoy, Expat License,
    Copyright (c) 2015-2026 Lior Halphon; see the notice in apu.rs).
*/

//! The mode 3 renderer: the pixel fetcher, the pixel FIFOs and object fetches.
//!
//! The PPU doesn't draw whole tiles at once. Each dot, the background fetcher takes one step
//! (see [`FetcherState`]) towards reading the next 8-pixel row of a tile: first the tile number
//! from the tile map, then the low and high bytes of that row from the tile data. When the
//! background FIFO (a first-in, first-out queue of pixels) is empty, the fetcher pushes the 8
//! pixels into it. Meanwhile, every dot in which the background FIFO has pixels, one pixel is
//! shifted out, combined with the matching pixel of the object FIFO and written to the frame
//! buffer.
//!
//! When an object starts at the current x position, shifting stops while the object's tile row is
//! fetched (see [`Phase`]) and mixed into the object FIFO. When the window starts, the background
//! FIFO is cleared and the fetcher restarts on the window's tile map. Both stalls make mode 3
//! longer, which is why its length varies from line to line.
//!
//! Registers such as SCX, LCDC and the palettes are read on the same dots as in SameBoy, so a
//! write in the middle of mode 3 takes effect in the middle of the line, like on hardware.

use crate::gpu::constants::{GB_SCREEN_HEIGHT, GB_SCREEN_WIDTH};
use crate::gpu::lcdc::{
    bg_and_window_enabled, bg_uses_high_tile_map, objects_enabled, uses_8000_addressing,
    uses_tall_objects, window_enabled, window_uses_high_tile_map,
};
use crate::gpu::palettes::{BLACK, Color, WHITE};
use crate::gpu::{Gpu, OamDmaAccess, TimingVariant};
use crate::model::Model;
use crate::serializable::{FieldWriter, Serializable, array_encoding, read_fields};
use crate::utils::is_bit_set;
use serializable_derive::Serializable;
use std::io::{self, Read, Write};

const FIFO_LENGTH: usize = 8;
const MAX_VISIBLE_OBJECTS: usize = 10;
const VRAM_BANK_SIZE: u16 = 0x2000;
/// The VRAM offsets of the two tile maps, at 0x9800 and 0x9C00.
const LOW_TILE_MAP: u16 = 0x1800;
const HIGH_TILE_MAP: u16 = 0x1C00;
/// Each tile takes 16 bytes of VRAM: two bytes (the low and high bitplanes) per row.
const TILE_BYTES: u16 = 16;
/// With the $8800 method, signed tile numbers count from tile 0 at this VRAM offset (0x9000).
const SIGNED_TILE_DATA_CENTER: u16 = 0x1000;

/// Bits of a CGB background tile's attributes (read from VRAM bank 1) and of an object's OAM
/// flags, which share their layout: the CGB palette, the VRAM bank of the tile data, the DMG
/// palette (objects only: OBP0 or OBP1), x and y flip, and the background's priority over objects.
const PALETTE_BITS: u8 = 0b0000_0111;
const VRAM_BANK_BIT: u8 = 3;
const DMG_PALETTE_BIT: u8 = 4;
const X_FLIP_BIT: u8 = 5;
const Y_FLIP_BIT: u8 = 6;
const PRIORITY_BIT: u8 = 7;
/// The number of objects OAM describes, which OAM scan checks one after the other.
const OAM_ENTRY_COUNT: u16 = 40;
/// Mode 3 only draws on lines 0-143, with `lcd_x` below 160 (save states are checked for this,
/// see `Pipeline::is_consistent`), so its pixels are always in the frame buffer. The renderer
/// still skips a pixel outside it instead of panicking, and debug builds stop there with this.
const FRAME_BUFFER_BOUNDS: &str = "mode 3 draws within the frame buffer";
/// The CGB has eight background and eight object palettes.
const CGB_PALETTE_COUNT: usize = 8;
/// A tile map is 32 tiles wide.
const TILE_MAP_WIDTH: u8 = 32;
/// `position_in_line` at the start of a line: -16 as a signed byte.
const POSITION_LINE_START: u8 = 0xF0;
/// `position_in_line` once the fine scroll pixels have been skipped: -8 as a signed byte.
const POSITION_AFTER_FINE_SCROLL: u8 = 0xF8;
/// `position_in_line` at which the window starts for WX 0: -7 as a signed byte.
const POSITION_OF_WX_0: u8 = POSITION_AFTER_FINE_SCROLL + 1;
/// WX is the window's screen x plus 7.
const WX_OFFSET: u8 = 7;
/// The WX of a window starting at the last pixel of the line, 166.
const LAST_PIXEL_WX: u8 = GB_SCREEN_WIDTH as u8 - 1 + WX_OFFSET;
/// The WX of the end of the line, 167, one past the last pixel.
const LINE_END_WX: u8 = LAST_PIXEL_WX + 1;
/// An object's OAM x is its screen x plus 8.
const OBJECT_X_OFFSET: u8 = 8;

/// The steps of the background fetcher, one per dot. Each memory read takes two dots: on the
/// first ("T1") the fetcher works out the address and on the second ("T2") it reads VRAM.
///
/// The variants are in the order the fetcher goes through them, and that order matters: the
/// object fetch compares states (`Phase::WaitForFetcher` waits while the state is before
/// `GetTileDataHighT2`), and `Gpu::take_tile_steps` follows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serializable)]
enum FetcherState {
    /// Picks the tile map (background or window) and computes the tile map address.
    #[default]
    GetTileT1,
    /// Reads the tile number, and on CGB the tile's attributes from VRAM bank 1.
    GetTileT2,
    /// Computes the address of the low byte of the tile row.
    GetTileDataLowT1,
    /// Reads the low byte (bit 0 of each pixel's color ID).
    GetTileDataLowT2,
    /// Computes the address of the high byte of the tile row.
    GetTileDataHighT1,
    /// Reads the high byte (bit 1 of each pixel's color ID), then tries to push.
    GetTileDataHighT2,
    /// Waits until the background FIFO is empty, then pushes the row and starts over.
    Push,
}

/// The background state the window start replaced, kept on CGB until the first window pixel so a
/// cleared window enable bit can cancel the start.
#[derive(Clone, Debug, Default, Serializable)]
struct WindowStartBackup {
    bg_fifo: BackgroundFifo,
    fetcher_state: FetcherState,
    current_tile: u8,
    current_tile_attributes: u8,
    current_tile_data: [u8; 2],
}

/// One pixel waiting in a FIFO.
#[derive(Debug, Clone, Copy, Default, Serializable)]
struct FifoItem {
    /// The 2-bit color ID from the tile data. 0 means transparent for objects.
    color_id: u8,
    /// The palette number: a CGB palette 0-7, or OBP0/OBP1 (0/1) for DMG objects.
    palette: u8,
    /// For CGB objects, the OAM index: when objects overlap, the lower index wins.
    priority: u8,
    /// The background-over-object flag: from the CGB tile attributes for background pixels, or
    /// from the object's flags for object pixels.
    bg_priority: bool,
}

/// The background FIFO. The fetcher only pushes into it when it is empty, a whole tile row at a
/// time, so it holds one row: its two bitplanes, which shift out from the left one pixel per dot,
/// and the attributes all its pixels share.
#[derive(Clone, Copy, Debug, Default)]
struct BackgroundFifo {
    /// The low bitplane (bit 0 of each color ID), next pixel in bit 7.
    lower: u8,
    /// The high bitplane (bit 1 of each color ID), next pixel in bit 7.
    upper: u8,
    palette: u8,
    bg_priority: bool,
    size: usize,
}

impl BackgroundFifo {
    fn clear(&mut self) {
        self.size = 0;
    }

    /// Fills the empty FIFO with one tile row, the leftmost pixel in bit 7 of `lower` and `upper`
    /// unless `flip_x` is set.
    fn push_row(&mut self, lower: u8, upper: u8, palette: u8, bg_priority: bool, flip_x: bool) {
        let (lower, upper) = if flip_x {
            (lower.reverse_bits(), upper.reverse_bits())
        } else {
            (lower, upper)
        };
        *self = BackgroundFifo {
            lower,
            upper,
            palette,
            bg_priority,
            size: FIFO_LENGTH,
        };
    }

    /// Fills the empty FIFO with a single pixel of color 0.
    fn push_blank_pixel(&mut self) {
        *self = BackgroundFifo {
            size: 1,
            ..BackgroundFifo::default()
        };
    }

    /// Shifts out the oldest pixel. The caller checks that the FIFO isn't empty.
    fn pop(&mut self) -> FifoItem {
        let color_id = (self.lower >> 7) | ((self.upper >> 7) << 1);
        self.lower <<= 1;
        self.upper <<= 1;
        self.size -= 1;
        FifoItem {
            color_id,
            palette: self.palette,
            priority: 0,
            bg_priority: self.bg_priority,
        }
    }
}

impl Serializable for BackgroundFifo {
    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        let mut fields = FieldWriter::new(writer);
        fields.field("lower", &self.lower)?;
        fields.field("upper", &self.upper)?;
        fields.field("palette", &self.palette)?;
        fields.field("bg_priority", &self.bg_priority)?;
        fields.field_with("size", u8::encoding, |writer| {
            write_fifo_index(self.size, writer)
        })?;
        fields.finish()
    }

    fn deserialize(&mut self, reader: &mut dyn Read) -> io::Result<()> {
        read_fields(reader, |name, bytes| {
            match name {
                "lower" => self.lower.deserialize(bytes)?,
                "upper" => self.upper.deserialize(bytes)?,
                "palette" => self.palette.deserialize(bytes)?,
                "bg_priority" => self.bg_priority.deserialize(bytes)?,
                "size" => self.size = read_fifo_index(bytes, FIFO_LENGTH)?,
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        if usize::from(self.palette) >= CGB_PALETTE_COUNT {
            return Err(invalid_renderer_state());
        }
        Ok(())
    }
}

/// Writes a FIFO's size or head, which the FIFOs keep as a `usize` for indexing, as a byte.
fn write_fifo_index(index: usize, writer: &mut dyn Write) -> io::Result<()> {
    u8::try_from(index)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "FIFO index out of range"))?
        .serialize(writer)
}

/// Reads a FIFO's size or head written by `write_fifo_index`, rejecting one above `max`.
fn read_fifo_index(reader: &mut dyn Read, max: usize) -> io::Result<usize> {
    let mut index = 0u8;
    index.deserialize(reader)?;
    if usize::from(index) > max {
        return Err(invalid_renderer_state());
    }
    Ok(usize::from(index))
}

/// The error for a loaded save state whose renderer state the emulator never produces.
fn invalid_renderer_state() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "renderer state out of range")
}

/// The object FIFO: an 8-pixel ring buffer that object rows are mixed into, shifted out one pixel
/// per dot.
#[derive(Clone, Debug, Default)]
struct Fifo {
    items: [FifoItem; FIFO_LENGTH],
    /// The index of the next pixel to shift out.
    head: usize,
    size: usize,
}

impl Serializable for Fifo {
    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        let mut fields = FieldWriter::new(writer);
        let items_encoding = || array_encoding::<FifoItem>(self.items.len());
        fields.field_with("items", items_encoding, |writer| {
            self.items
                .iter()
                .try_for_each(|item| item.serialize(writer))
        })?;
        fields.field_with("head", u8::encoding, |writer| {
            write_fifo_index(self.head, writer)
        })?;
        fields.field_with("size", u8::encoding, |writer| {
            write_fifo_index(self.size, writer)
        })?;
        fields.finish()
    }

    fn deserialize(&mut self, reader: &mut dyn Read) -> io::Result<()> {
        read_fields(reader, |name, bytes| {
            match name {
                "items" => self
                    .items
                    .iter_mut()
                    .try_for_each(|item| item.deserialize(bytes))?,
                "head" => self.head = read_fifo_index(bytes, FIFO_LENGTH - 1)?,
                "size" => self.size = read_fifo_index(bytes, FIFO_LENGTH)?,
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        if self
            .items
            .iter()
            .any(|item| item.color_id > 0b11 || usize::from(item.palette) >= CGB_PALETTE_COUNT)
        {
            return Err(invalid_renderer_state());
        }
        Ok(())
    }
}

impl Fifo {
    fn clear(&mut self) {
        self.head = 0;
        self.size = 0;
    }

    /// Shifts out the oldest pixel. The caller checks that the FIFO isn't empty.
    fn pop(&mut self) -> FifoItem {
        let item = self.items[self.head];
        self.head = (self.head + 1) & (FIFO_LENGTH - 1);
        self.size -= 1;
        item
    }

    /// Mixes an object's tile row into the object FIFO. An object pixel only replaces a pixel
    /// that is transparent or belongs to an object with a higher `priority` value, so on DMG the
    /// object fetched first stays in front.
    fn overlay_object_row(
        &mut self,
        lower: u8,
        upper: u8,
        palette: u8,
        bg_priority: bool,
        priority: u8,
        flip_x: bool,
    ) {
        while self.size < FIFO_LENGTH {
            self.items[(self.head + self.size) & (FIFO_LENGTH - 1)] = FifoItem::default();
            self.size += 1;
        }

        let flip_xor = if flip_x { 0 } else { 7 };
        for bit in (0..FIFO_LENGTH).rev() {
            let color_id = ((lower >> bit) & 1) | (((upper >> bit) & 1) << 1);
            let target = &mut self.items[(self.head + (bit ^ flip_xor)) & (FIFO_LENGTH - 1)];
            if color_id != 0 && (target.color_id == 0 || target.priority > priority) {
                *target = FifoItem {
                    color_id,
                    palette,
                    priority,
                    bg_priority,
                };
            }
        }
    }
}

/// Where pixels get their colors from. Nothing the PPU does while it runs changes it.
///
/// `Gpu::draw_plain_pixels` passes each variant as a constant to `draw_plain_pixels_in`, and from
/// there it goes to `draw_background_tile`, `draw_plain_pixel`, `mixed_pixel_color`,
/// `background_color_in` and `object_color_in`. All of them are `#[inline(always)]`, which is what
/// gives each variant a copy of the drawing loop in which the matches on the colors fold away:
/// without it (as clippy's pedantic lints suggest), the choice would quietly come back in every
/// pixel. A trait with a type per variant would make the copies certain, but it measured 1% more
/// instructions than this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PixelColors {
    /// STOP cut the CGB's PPU off from palette RAM: every pixel is black.
    Blocked,
    /// CGB palette RAM, picked by the tile attributes and object flags.
    Cgb,
    /// A DMG game on a CGB: the DMG palette registers pick colors from CGB palettes.
    Compatibility,
    /// The DMG's gray shades.
    Dmg,
}

/// Where the renderer is within the current dot. Phases that end a dot return from
/// `Gpu::tick_drawing`; the others fall through to the next phase in the same dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serializable)]
enum Phase {
    /// Checks whether the window starts at this position. Starting it can stall for a dot.
    #[default]
    LineStart,
    /// Continues after the one-dot stall of a window start.
    AfterWindowStall,
    /// Drops objects left of the current position and starts fetching one that begins here.
    ObjectCheck,
    /// Keeps the background fetcher going until it has a row ready, before the object fetch.
    WaitForFetcher,
    /// One more background fetcher step.
    ExtraFetcherStep,
    /// Reads the object's tile number and flags from OAM.
    ReadObjectAttributes,
    ObjectAttributesWait,
    /// Reads the low byte of the object's tile row.
    ReadObjectLow,
    ObjectLowWait,
    /// Reads the high byte of the object's tile row.
    ReadObjectHigh,
    /// Mixes the fetched row into the object FIFO, then checks for another object here.
    OverlayObject,
    /// Shifts out a pixel if possible and advances the background fetcher.
    Render,
    /// All 160 pixels of the line have been drawn.
    Done,
}

impl Phase {
    fn is_object_fetch(self) -> bool {
        matches!(
            self,
            Phase::WaitForFetcher
                | Phase::ExtraFetcherStep
                | Phase::ReadObjectAttributes
                | Phase::ObjectAttributesWait
                | Phase::ReadObjectLow
                | Phase::ObjectLowWait
        )
    }
}

/// The renderer's state for the current line. Field names mostly follow SameBoy's, to make the
/// two easy to compare.
///
/// Save states hold all of it but the one-dot flags `write_timing.rs` sets around a register
/// write, which are always clear between the emulator's steps. `Gpu`'s save state checks the
/// loaded values (`Pipeline::is_consistent`).
#[derive(Clone, Debug, Default, Serializable)]
pub(super) struct Pipeline {
    bg_fifo: BackgroundFifo,
    oam_fifo: Fifo,
    phase: Phase,
    /// The x position of the next pixel shifted out of the background FIFO. It starts at -16
    /// (0xF0) so that the fine scroll pixels (SCX & 7) and the first 8 pixels can be dropped
    /// before x = 0. The line ends at 160. As in SameBoy it is a byte that holds -16 to -1 as
    /// 0xF0 to 0xFF (it can't be an `i8`: 160 doesn't fit), so an unsigned comparison such as
    /// `position_in_line < 158` also leaves out the positions before the screen; `is_on_screen`
    /// and `signed_position` say so where it matters.
    pub(super) position_in_line: u8,
    /// The x position in the frame buffer of the next pixel written.
    lcd_x: u8,
    line_has_fractional_scrolling: bool,
    fetcher_state: FetcherState,
    current_tile: u8,
    /// The CGB attributes of the current tile: palette (bits 0-2), VRAM bank (bit 3), x flip
    /// (bit 5), y flip (bit 6) and background priority (bit 7).
    current_tile_attributes: u8,
    /// The low and high bytes of the tile row the fetcher read.
    current_tile_data: [u8; 2],
    last_tile_index_address: u16,
    last_tile_data_address: u16,
    /// Whether the last tile data address used the $8000 method (LCDC bit 4).
    last_tileset: bool,
    /// The tile map y the fetcher latched on its first step, on revisions that do that.
    cached_fetcher_y: u8,
    /// The value an LCDC tile data area write glitch reads instead of tile data.
    data_for_sel_glitch: u8,
    last_unsigned_fetch_data: u8,
    /// Whether the fetcher is drawing the window instead of the background.
    wx_triggered: bool,
    window_tile_x: u8,
    /// The window's line counter, incremented each line the window starts.
    window_y: u8,
    /// Set from the window start until the first window pixel is shifted out.
    pub(super) window_is_being_fetched: bool,
    window_start_backup: Option<WindowStartBackup>,
    /// DMG: the window reached WX 166 on the last pixel, so it starts the next line if the
    /// window enable bit is set when that line starts drawing.
    window_continue_pending: bool,
    /// Set by the DMG glitch that inserts one extra background pixel when the window starts.
    insert_bg_pixel: bool,
    /// The fields from here to `tall_objects_glitch` are set by `write_timing.rs` around register
    /// writes that land mid-dot; see the matching setters on `Gpu`.
    pub(super) disable_window_pixel_insertion_glitch: bool,
    #[serializable(skip)]
    pub(super) wx_just_changed: bool,
    #[serializable(skip)]
    pub(super) written_bg_enable: Option<bool>,
    #[serializable(skip)]
    pub(super) tile_sel_glitch: bool,
    /// CGB: set for the dot in which an LCDC write that changes the object size lands, during
    /// which OAM scan finds objects as if they were 16 pixels tall.
    #[serializable(skip)]
    pub(super) tall_objects_glitch: bool,
    /// CGB: the fetcher reuses the previous tile data because the window starts mid-fetch.
    cgb_wx_glitch: bool,
    /// Whether the renderer is checking for or fetching objects rather than shifting out pixels.
    pub(super) during_object_fetch: bool,
    /// Set when a DMG LCDC write disables objects during an object fetch, ending the fetch.
    pub(super) object_fetch_aborted: bool,
    /// The objects found by OAM scan, sorted by descending x, so the next object to draw is at
    /// the end.
    visible_object_indices: [u8; MAX_VISIBLE_OBJECTS],
    visible_object_x: [u8; MAX_VISIBLE_OBJECTS],
    visible_object_y: [u8; MAX_VISIBLE_OBJECTS],
    visible_object_count: u8,
    object_tile: u8,
    /// The object's OAM flags: CGB palette (bits 0-2), CGB VRAM bank (bit 3), DMG palette
    /// (bit 4), x flip (bit 5), y flip (bit 6) and background priority (bit 7).
    object_flags: u8,
    object_tile_data: [u8; 2],
}

impl Pipeline {
    /// Makes the window line counter wrap to 0 the next time the window starts.
    pub(super) fn reset_window_line(&mut self) {
        self.window_y = 0xFF;
    }

    /// `position_in_line` as the number it stands for, -16 to 160.
    fn signed_position(&self) -> i16 {
        if self.position_in_line >= POSITION_LINE_START {
            i16::from(self.position_in_line.cast_signed())
        } else {
            i16::from(self.position_in_line)
        }
    }

    /// Whether the next pixel is one of the 160 the screen shows, not one of the pixels dropped
    /// before x = 0.
    fn is_on_screen(&self) -> bool {
        self.position_in_line < GB_SCREEN_WIDTH as u8
    }

    /// Whether the fine scroll pixels (SCX & 7) are still being dropped at the start of the line:
    /// positions -16 to -9.
    fn in_fine_scroll(&self) -> bool {
        (POSITION_LINE_START..POSITION_AFTER_FINE_SCROLL).contains(&self.position_in_line)
    }

    /// The WX at which the window starts at the current position.
    fn wx_here(&self) -> u8 {
        self.position_in_line.wrapping_add(WX_OFFSET)
    }

    /// The OAM x coordinate (screen x + 8) of an object that starts at the current position. The
    /// positions before -8 match objects at OAM x 0.
    fn x_for_object_match(&self) -> u8 {
        if self.in_fine_scroll() {
            0
        } else {
            self.position_in_line.wrapping_add(OBJECT_X_OFFSET)
        }
    }

    /// The slot of the next object to draw. The caller checks that there is one.
    fn next_object(&self) -> usize {
        usize::from(self.visible_object_count) - 1
    }

    /// Whether a loaded save state left the renderer with values that fit together and that it
    /// can use as indexes: positions within the line, at most ten objects found by OAM scan (each
    /// one of the 40 in OAM), and tile addresses within VRAM. The FIFOs check their own contents
    /// as they load. `drawing_line` is the line being drawn once the renderer has started on it
    /// (`start_drawing`), if any: pixels are only written then, at `lcd_x`, which matches the
    /// position.
    pub(super) fn is_consistent(&self, drawing_line: Option<u8>) -> bool {
        let position_valid = self.position_in_line <= GB_SCREEN_WIDTH as u8
            || self.position_in_line >= POSITION_LINE_START;
        let lcd_x_valid = match drawing_line {
            Some(line) => {
                u32::from(line) < GB_SCREEN_HEIGHT
                    && self.lcd_x
                        == if self.position_in_line >= POSITION_LINE_START {
                            0
                        } else {
                            self.position_in_line
                        }
            }
            None => u32::from(self.lcd_x) <= GB_SCREEN_WIDTH,
        };
        let count = usize::from(self.visible_object_count);
        let objects_valid = count <= MAX_VISIBLE_OBJECTS
            && self.visible_object_indices[..count]
                .iter()
                .all(|&index| u16::from(index) < OAM_ENTRY_COUNT);
        position_valid
            && lcd_x_valid
            && objects_valid
            && self.last_tile_index_address < VRAM_BANK_SIZE
            && self.last_tile_data_address < 2 * VRAM_BANK_SIZE
            && self.window_tile_x < TILE_MAP_WIDTH
    }

    /// How many dots of mode 3 surely pass before the second to last pixel of the line is drawn,
    /// which starts the countdown to the mode 0 interrupt source. `position_in_line` moves on at
    /// most one pixel per dot, except while the fine scroll pixels are skipped at the start of the
    /// line, where it can jump to -7.
    pub(super) fn dots_before_last_pixels(&self) -> u16 {
        let position = if self.in_fine_scroll() {
            -8
        } else {
            self.signed_position()
        };
        (GB_SCREEN_WIDTH as i16 - 1 - position)
            .max(0)
            .cast_unsigned()
    }

    /// `dots_before_last_pixels` at the start of a line.
    pub(super) const DOTS_BEFORE_LAST_PIXELS_OF_LINE: u16 = GB_SCREEN_WIDTH as u16 - 1 + 8;
}

impl Gpu {
    /// Whether colors, tile attributes and object priority work the CGB way, which is the case
    /// on CGB hardware outside DMG compatibility mode.
    fn uses_cgb_rendering(&self) -> bool {
        self.is_cgb() && !self.in_dmg_compatibility_mode()
    }

    pub(super) fn clear_visible_objects(&mut self) {
        self.pipeline.visible_object_count = 0;
    }

    /// OAM scan for one entry: remembers object `index` if it covers the current line and fewer
    /// than ten objects were found so far, keeping the list sorted by x. While OAM DMA runs the
    /// scan can't read OAM and checks the Y and X it read last again; while DMA is paused by HALT
    /// it reads them, but models before CGB-E find no objects (SameBoy).
    pub(super) fn scan_oam_entry(&mut self, index: u16) {
        if self.stop_blocks.oam {
            self.oam_scan_y_bus = 0xFF;
            self.oam_scan_x_bus = 0xFF;
        } else if !matches!(self.oam_dma_access, OamDmaAccess::Running { .. }) {
            self.oam_scan_y_bus = self.object_attribute_memory[index as usize * 4];
            self.oam_scan_x_bus = self.object_attribute_memory[index as usize * 4 + 1];
        }
        if usize::from(self.pipeline.visible_object_count) == MAX_VISIBLE_OBJECTS
            || (self.oam_dma_access == OamDmaAccess::PausedByHalt && self.model < Model::CgbE)
        {
            return;
        }

        let oam_y = self.oam_scan_y_bus;
        let oam_x = self.oam_scan_x_bus;
        let height = if uses_tall_objects(self.lcdc) || self.pipeline.tall_objects_glitch {
            16
        } else {
            8
        };
        let y = i16::from(oam_y) - 16;
        let line = i16::from(self.ly);
        if y > line || y + height <= line {
            return;
        }
        self.add_visible_object(index, oam_x, oam_y);
    }

    /// `scan_oam_entry` for each entry in `entries`. While nothing keeps the scan from reading
    /// OAM, what decides whether an object is found is the same for all of them, so it is worked
    /// out once.
    pub(super) fn scan_oam_entries(&mut self, entries: std::ops::Range<u16>) {
        if self.stop_blocks.oam || !matches!(self.oam_dma_access, OamDmaAccess::Free) {
            for index in entries {
                self.scan_oam_entry(index);
            }
            return;
        }
        let height = if uses_tall_objects(self.lcdc) || self.pipeline.tall_objects_glitch {
            16
        } else {
            8
        };
        let line = i16::from(self.ly);
        for index in entries {
            let oam_y = self.object_attribute_memory[usize::from(index) * 4];
            let oam_x = self.object_attribute_memory[usize::from(index) * 4 + 1];
            self.oam_scan_y_bus = oam_y;
            self.oam_scan_x_bus = oam_x;
            let y = i16::from(oam_y) - 16;
            if usize::from(self.pipeline.visible_object_count) < MAX_VISIBLE_OBJECTS
                && y <= line
                && y + height > line
            {
                self.add_visible_object(index, oam_x, oam_y);
            }
        }
    }

    /// Adds OAM entry `index` to the objects found on the line, keeping them sorted by x.
    fn add_visible_object(&mut self, index: u16, oam_x: u8, oam_y: u8) {
        let pipeline = &mut self.pipeline;
        let count = usize::from(pipeline.visible_object_count);
        let insert_at = (0..count)
            .find(|&j| pipeline.visible_object_x[j] <= oam_x)
            .unwrap_or(count);
        pipeline
            .visible_object_indices
            .copy_within(insert_at..count, insert_at + 1);
        pipeline
            .visible_object_x
            .copy_within(insert_at..count, insert_at + 1);
        pipeline
            .visible_object_y
            .copy_within(insert_at..count, insert_at + 1);
        pipeline.visible_object_indices[insert_at] = index as u8;
        pipeline.visible_object_x[insert_at] = oam_x;
        pipeline.visible_object_y[insert_at] = oam_y;
        pipeline.visible_object_count += 1;
    }

    /// A PPU read of OAM in mode 3. While OAM DMA copies, the PPU sees the pair of bytes the
    /// transfer writes next instead (SameBoy).
    fn ppu_oam_read(&self, address: usize) -> u8 {
        if self.stop_blocks.oam {
            return 0xFF;
        }
        match self.oam_dma_access {
            OamDmaAccess::Running { destination }
                if destination > 0
                    && usize::from(destination) < self.object_attribute_memory.len() =>
            {
                self.object_attribute_memory[usize::from(destination & !1) | (address & 1)]
            }
            _ => self.object_attribute_memory[address],
        }
    }

    /// A PPU read of VRAM, which reads 0xFF while STOP cuts the PPU off from VRAM.
    fn ppu_vram_read(&self, address: usize) -> u8 {
        if self.stop_blocks.vram {
            0xFF
        } else {
            self.video_ram[address]
        }
    }

    /// Resets the renderer for a new line. The background FIFO starts with 8 dummy pixels.
    pub(super) fn start_drawing(&mut self) {
        let pipeline = &mut self.pipeline;
        pipeline.disable_window_pixel_insertion_glitch = false;
        pipeline.bg_fifo.clear();
        pipeline.oam_fifo.clear();
        pipeline.bg_fifo.push_row(0, 0, 0, false, false);
        pipeline.lcd_x = 0;
        pipeline.position_in_line = POSITION_LINE_START;
        pipeline.line_has_fractional_scrolling = false;
        pipeline.fetcher_state = FetcherState::GetTileT1;
        pipeline.phase = Phase::LineStart;
        if std::mem::take(&mut pipeline.window_continue_pending) && window_enabled(self.lcdc) {
            pipeline.wx_triggered = true;
            pipeline.window_tile_x = 1;
            pipeline.window_y = pipeline.window_y.wrapping_add(1);
        }
    }

    /// Finishes the line after the last pixel: fills any pixels the renderer skipped and carries
    /// a DMG window start at WX 166 over to the next line.
    pub(super) fn end_drawing(&mut self) {
        self.fill_rest_of_desynced_line();
        let is_cgb = self.is_cgb();
        // The request is made even with the window enable bit clear; it only has to be set
        // when the next line starts (Gambatte's window on_screen wxA6 tests).
        let window_continues = !is_cgb && self.window_y_triggered && self.wx == LAST_PIXEL_WX;
        let pipeline = &mut self.pipeline;
        pipeline.position_in_line = POSITION_LINE_START;
        pipeline.line_has_fractional_scrolling = false;
        if matches!(
            pipeline.fetcher_state,
            FetcherState::GetTileDataHighT1 | FetcherState::GetTileDataHighT2
        ) {
            // The line ended before the high byte was read, so the high byte slot keeps the last
            // byte the fetcher did read, as in SameBoy. (SameBoy also tried setting
            // data_for_sel_glitch here and left that out because it glitched Tesserae.)
            pipeline.current_tile_data[1] = pipeline.current_tile_data[0];
        }

        pipeline.window_continue_pending = window_continues;
        pipeline.wx_triggered = false;
    }

    /// Fills the rest of the line by repeating the last pixel written, for lines where the
    /// renderer ended before writing all 160 pixels.
    fn fill_rest_of_desynced_line(&mut self) {
        let line_start = usize::from(self.ly) * GB_SCREEN_WIDTH as usize;
        if line_start >= self.frame_buffer.len() {
            return;
        }
        while u32::from(self.pipeline.lcd_x) < GB_SCREEN_WIDTH {
            let pixel_index = line_start + usize::from(self.pipeline.lcd_x);
            self.frame_buffer[pixel_index] = if self.pipeline.lcd_x == 0 {
                self.background_color(0, 0)
            } else {
                self.frame_buffer[pixel_index - 1]
            };
            self.pipeline.lcd_x += 1;
        }
    }

    /// Advances the renderer by one dot. Returns true once the last pixel of the line is drawn.
    pub(super) fn tick_drawing(&mut self) -> bool {
        if self.pipeline.object_fetch_aborted {
            self.pipeline.object_fetch_aborted = false;
            if self.pipeline.phase.is_object_fetch() {
                self.pipeline.phase = Phase::Render;
            }
        }

        loop {
            match self.pipeline.phase {
                Phase::LineStart => {
                    if self.check_window_activation() {
                        self.pipeline.phase = Phase::AfterWindowStall;
                        return false;
                    }
                    self.after_window_check();
                }
                Phase::AfterWindowStall => self.after_window_check(),
                Phase::ObjectCheck => {
                    let match_x = self.pipeline.x_for_object_match();
                    let pipeline = &mut self.pipeline;
                    while pipeline.visible_object_count != 0
                        && pipeline.visible_object_x[pipeline.next_object()] < match_x
                    {
                        pipeline.visible_object_count -= 1;
                    }
                    let fetching_objects = objects_enabled(self.lcdc) || self.is_cgb();
                    let pipeline = &mut self.pipeline;
                    if fetching_objects
                        && pipeline.visible_object_count != 0
                        && pipeline.visible_object_x[pipeline.next_object()] == match_x
                    {
                        pipeline.phase = Phase::WaitForFetcher;
                    } else {
                        pipeline.phase = Phase::Render;
                    }
                }
                Phase::WaitForFetcher => {
                    if self.pipeline.fetcher_state < FetcherState::GetTileDataHighT2
                        || self.pipeline.bg_fifo.size == 0
                    {
                        self.advance_fetcher();
                        return false;
                    }
                    self.pipeline.phase = Phase::ExtraFetcherStep;
                }
                Phase::ExtraFetcherStep => {
                    self.advance_fetcher();
                    self.pipeline.phase = Phase::ReadObjectAttributes;
                    return false;
                }
                Phase::ReadObjectAttributes => {
                    self.advance_fetcher();
                    let object = usize::from(
                        self.pipeline.visible_object_indices[self.pipeline.next_object()],
                    );
                    self.pipeline.object_tile = self.ppu_oam_read(object * 4 + 2);
                    self.pipeline.object_flags = self.ppu_oam_read(object * 4 + 3);
                    // The tile number comes over the same OAM bus latch that OAM scan reads Y
                    // through, so a scan that can't read OAM later sees it (SameBoy's
                    // mode2_y_bus).
                    self.oam_scan_y_bus = self.pipeline.object_tile;
                    self.pipeline.phase = Phase::ObjectAttributesWait;
                    return false;
                }
                Phase::ObjectAttributesWait => {
                    self.pipeline.phase = Phase::ReadObjectLow;
                    return false;
                }
                Phase::ReadObjectLow => {
                    // On CGB the low bitplane read sees an object size change a dot late, so a
                    // change between the two reads mixes rows of different tiles (Mealybug's
                    // m3_lcdc_obj_size_change_scx on CGB-C and CGB-D).
                    let lcdc = if self.is_cgb() {
                        self.previous_lcdc
                    } else {
                        self.lcdc
                    };
                    let address = self.object_line_address_with(lcdc);
                    self.pipeline.object_tile_data[0] = self.ppu_vram_read(address as usize);
                    self.pipeline.last_unsigned_fetch_data = self.pipeline.object_tile_data[0];
                    self.pipeline.phase = Phase::ObjectLowWait;
                    return false;
                }
                Phase::ObjectLowWait => {
                    self.pipeline.phase = Phase::ReadObjectHigh;
                    return false;
                }
                Phase::ReadObjectHigh => {
                    self.pipeline.during_object_fetch = false;
                    let address = self.object_line_address();
                    self.pipeline.object_tile_data[1] = self.ppu_vram_read(address as usize + 1);
                    self.pipeline.last_unsigned_fetch_data = self.pipeline.object_tile_data[1];
                    self.pipeline.phase = Phase::OverlayObject;
                    return false;
                }
                Phase::OverlayObject => {
                    self.overlay_current_object();
                    self.pipeline.during_object_fetch = true;
                    self.pipeline.phase = Phase::ObjectCheck;
                }
                Phase::Render => {
                    self.pipeline.during_object_fetch = false;
                    let position_before = self.pipeline.position_in_line;
                    self.render_pixel_if_possible();
                    if position_before != self.pipeline.position_in_line
                        && self.pipeline.position_in_line == GB_SCREEN_WIDTH as u8 - 1
                    {
                        self.start_hblank_interrupt_countdown();
                    }
                    self.advance_fetcher();
                    if self.pipeline.position_in_line == GB_SCREEN_WIDTH as u8 {
                        self.pipeline.phase = Phase::Done;
                        return true;
                    }
                    self.pipeline.phase = Phase::LineStart;
                    return false;
                }
                Phase::Done => return false,
            }
        }
    }

    /// Arms the DMG glitch that inserts an extra background pixel when the window starts right
    /// after the fetcher pushed a row, then moves on to the object check.
    fn after_window_check(&mut self) {
        let wx = self.wx;
        let pipeline = &mut self.pipeline;
        if wx == pipeline.wx_here()
            && pipeline.wx_triggered
            && !pipeline.window_is_being_fetched
            && pipeline.fetcher_state == FetcherState::GetTileT1
            && pipeline.bg_fifo.size == FIFO_LENGTH
        {
            pipeline.insert_bg_pixel = true;
        }
        pipeline.during_object_fetch = true;
        pipeline.phase = Phase::ObjectCheck;
    }

    /// Starts the window when WX (the window's screen x plus 7) matches the current position,
    /// including the DMG and WX = 0 edge cases. Returns true when the start stalls for a dot.
    fn check_window_activation(&mut self) -> bool {
        if !self.window_can_start() {
            return false;
        }
        let is_cgb = self.is_cgb();
        let wx = self.wx;
        let scx_fine = self.scx & 7;

        let pipeline = &mut self.pipeline;
        let position = pipeline.position_in_line;
        let should_activate = if wx == 0 {
            position == POSITION_OF_WX_0
                || (position == POSITION_LINE_START && scx_fine != 0)
                || (POSITION_LINE_START + 1..=POSITION_AFTER_FINE_SCROLL).contains(&position)
        } else if wx < LAST_PIXEL_WX || (is_cgb && wx == LAST_PIXEL_WX) {
            wx == pipeline.wx_here()
                || (!is_cgb
                    && wx == pipeline.wx_here().wrapping_sub(1)
                    && !pipeline.wx_just_changed)
        } else {
            false
        };

        if should_activate {
            pipeline.window_start_backup = is_cgb.then_some(WindowStartBackup {
                bg_fifo: pipeline.bg_fifo,
                fetcher_state: pipeline.fetcher_state,
                current_tile: pipeline.current_tile,
                current_tile_attributes: pipeline.current_tile_attributes,
                current_tile_data: pipeline.current_tile_data,
            });
            pipeline.window_y = pipeline.window_y.wrapping_add(1);
            pipeline.window_tile_x = 0;
            pipeline.bg_fifo.clear();
            pipeline.wx_triggered = true;
            pipeline.fetcher_state = FetcherState::GetTileT1;
            pipeline.window_is_being_fetched = true;
            return wx == 0 && scx_fine != 0 && !is_cgb;
        }

        if !is_cgb && wx == LAST_PIXEL_WX && wx == pipeline.wx_here() {
            pipeline.window_y = pipeline.window_y.wrapping_add(1);
        }
        false
    }

    /// Whether the window may start on this dot, if WX matches: it hasn't started yet, LY has
    /// reached WY this frame, and the window enable bit is set as the window's start sees it. That
    /// sees changes to the bit a dot late in one direction: on CGB at normal speed setting the bit
    /// only lets the window start a dot later while clearing it stops the window at once
    /// (Gambatte's window late_reenable tests), and on DMG it's the other way round, so a disable
    /// landing as the window's position is reached is too late (Gambatte's window late_disable
    /// tests). In double speed the write itself lands late.
    pub(super) fn window_can_start(&self) -> bool {
        let enabled_now = window_enabled(self.lcdc);
        let enabled_before = window_enabled(self.previous_lcdc);
        let window_enable_seen = match self.timing {
            TimingVariant::Dmg => enabled_now || enabled_before,
            TimingVariant::Cgb => enabled_now && enabled_before,
            TimingVariant::CgbDoubleSpeed => enabled_now,
        };
        !self.pipeline.wx_triggered && window_enable_seen && self.window_y_active()
    }

    /// CGB only: works out whether the window starts during this fetch, in which case the
    /// fetcher reuses the previous tile data instead of reading new data.
    fn update_wx_glitch(&mut self) {
        if !self.is_cgb() {
            return;
        }
        let window_active = window_enabled(self.lcdc) && self.window_y_active();
        let wx = self.wx;
        let pipeline = &mut self.pipeline;
        if !window_active {
            pipeline.cgb_wx_glitch = false;
            return;
        }
        if wx == 0 {
            pipeline.cgb_wx_glitch = (POSITION_LINE_START..=POSITION_AFTER_FINE_SCROLL)
                .contains(&pipeline.position_in_line)
                || (pipeline.position_in_line == POSITION_OF_WX_0
                    && pipeline.line_has_fractional_scrolling);
            return;
        }
        pipeline.cgb_wx_glitch = pipeline
            .wx_here()
            .wrapping_add(u8::from(pipeline.window_is_being_fetched))
            == wx;
    }

    /// Whether the fetcher latches the tile map y on its first step. CGB-D and later do; DMG and
    /// older CGBs recompute it for each tile data read, so a mid-fetch SCY write shows up.
    fn caches_fetcher_y(&self) -> bool {
        self.model > Model::CgbC
    }

    /// The y coordinate within the background or window tile map that the fetcher reads.
    fn current_fetcher_y(&self) -> u8 {
        if self.pipeline.wx_triggered {
            self.pipeline.window_y
        } else {
            self.ly.wrapping_add(self.scy)
        }
    }

    /// The VRAM address of the low (`byte_offset` 0) or high (1) byte of the current tile row.
    fn tile_data_address(&mut self, byte_offset: u16) -> u16 {
        let y = if self.caches_fetcher_y() {
            self.pipeline.cached_fetcher_y
        } else {
            self.current_fetcher_y()
        };
        let last_tileset = uses_8000_addressing(self.lcdc);
        let pipeline = &mut self.pipeline;
        pipeline.last_tileset = last_tileset;
        let mut tile_address = if last_tileset {
            u16::from(pipeline.current_tile) * TILE_BYTES
        } else {
            SIGNED_TILE_DATA_CENTER.wrapping_add_signed(
                i16::from(pipeline.current_tile.cast_signed()) * TILE_BYTES.cast_signed(),
            )
        };
        if is_bit_set(pipeline.current_tile_attributes, VRAM_BANK_BIT) {
            tile_address += VRAM_BANK_SIZE;
        }
        let y_flip = if is_bit_set(pipeline.current_tile_attributes, Y_FLIP_BIT) {
            7
        } else {
            0
        };
        tile_address + u16::from((y & 7) ^ y_flip) * 2 + byte_offset
    }

    /// Reads tile data at `last_tile_data_address`, except for a CGB glitch right after a write
    /// switches LCDC to the $8000 method, which returns the previous unsigned fetch instead.
    fn read_tile_data_byte(&mut self) -> u8 {
        let tile_sel_just_set =
            !uses_8000_addressing(self.previous_lcdc) && uses_8000_addressing(self.lcdc);
        if self.timing == TimingVariant::Cgb
            && tile_sel_just_set
            && self.pipeline.last_tile_data_address & 0x800 == 0
        {
            return self.pipeline.last_unsigned_fetch_data;
        }
        let data = self.ppu_vram_read(self.pipeline.last_tile_data_address as usize);
        if self.pipeline.last_tileset {
            self.pipeline.last_unsigned_fetch_data = data;
        }
        data
    }

    /// During the LCDC tile data area write glitch: whether the fetcher reads the glitched value,
    /// and that value.
    fn tile_sel_glitch_data(&self) -> (bool, u8) {
        let pipeline = &self.pipeline;
        if pipeline.last_tileset {
            (pipeline.current_tile & 0x80 == 0, pipeline.current_tile)
        } else {
            (true, pipeline.data_for_sel_glitch)
        }
    }

    /// Runs one step of the background fetcher.
    fn advance_fetcher(&mut self) {
        match self.pipeline.fetcher_state {
            FetcherState::GetTileT1 => self.find_tile_number(),
            FetcherState::GetTileT2 => self.read_tile_number(),
            FetcherState::GetTileDataLowT1 => self.find_tile_data_low(),
            FetcherState::GetTileDataLowT2 => self.read_tile_data_low(),
            FetcherState::GetTileDataHighT1 => self.find_tile_data_high(),
            FetcherState::GetTileDataHighT2 => self.read_tile_data_high(),
            FetcherState::Push => self.fetcher_push(),
        }
    }

    /// `FetcherState::GetTileT1`: picks the background or window tile map and works out the
    /// address of the tile number.
    #[inline]
    fn find_tile_number(&mut self) {
        self.update_wx_glitch();
        if !window_enabled(self.lcdc) {
            self.pipeline.wx_triggered = false;
        }

        let wx_triggered = self.pipeline.wx_triggered;
        let high_map = (bg_uses_high_tile_map(self.lcdc) && !wx_triggered)
            || (window_uses_high_tile_map(self.lcdc) && wx_triggered);
        let map = if high_map {
            HIGH_TILE_MAP
        } else {
            LOW_TILE_MAP
        };

        let y = self.current_fetcher_y();
        let position = self.pipeline.position_in_line;
        let x = if wx_triggered {
            self.pipeline.window_tile_x
        } else if self.pipeline.in_fine_scroll() {
            self.scx >> 3
        } else {
            // A position before the screen (-8 to -1) counts 256 too many here, which is a whole
            // number of rows of the 32-tile map, so the tile column still comes out right.
            let object_adjust = u32::from(self.is_cgb() && !self.pipeline.during_object_fetch);
            (((u32::from(self.scx) + u32::from(position) + 8).wrapping_sub(object_adjust) / 8)
                & 0x1F) as u8
        };
        if self.caches_fetcher_y() {
            self.pipeline.cached_fetcher_y = y;
        }
        self.pipeline.last_tile_index_address = map + u16::from(x) + u16::from(y / 8) * 32;
        self.pipeline.fetcher_state = FetcherState::GetTileT2;
    }

    /// `FetcherState::GetTileT2`: reads the tile number, and on CGB its attributes.
    #[inline]
    fn read_tile_number(&mut self) {
        if !self.pipeline.cgb_wx_glitch {
            let address = self.pipeline.last_tile_index_address as usize;
            self.pipeline.current_tile = self.ppu_vram_read(address);
            if self.is_cgb() {
                self.pipeline.current_tile_attributes =
                    self.ppu_vram_read(address + VRAM_BANK_SIZE as usize);
            }
        }
        self.pipeline.fetcher_state = FetcherState::GetTileDataLowT1;
    }

    /// `FetcherState::GetTileDataLowT1`: works out the address of the tile row's low byte.
    #[inline]
    fn find_tile_data_low(&mut self) {
        self.update_wx_glitch();
        self.pipeline.last_tile_data_address = self.tile_data_address(0);
        self.pipeline.fetcher_state = FetcherState::GetTileDataLowT2;
    }

    /// `FetcherState::GetTileDataLowT2`: reads the tile row's low byte.
    #[inline]
    fn read_tile_data_low(&mut self) {
        if self.pipeline.cgb_wx_glitch {
            self.pipeline.current_tile_data[0] = self.pipeline.current_tile_data[1];
        } else {
            let address = self.pipeline.last_tile_data_address as usize;
            let mut use_glitched = false;
            if self.pipeline.tile_sel_glitch {
                let (should_use, data) = self.tile_sel_glitch_data();
                use_glitched = should_use;
                self.pipeline.current_tile_data[0] = data;
            }
            if !use_glitched {
                self.pipeline.current_tile_data[0] = self.read_tile_data_byte();
            }
            if self.pipeline.last_tileset && self.pipeline.tile_sel_glitch {
                self.pipeline.data_for_sel_glitch = self.ppu_vram_read(address);
            }
        }
        self.pipeline.fetcher_state = FetcherState::GetTileDataHighT1;
    }

    /// `FetcherState::GetTileDataHighT1`: works out the address of the tile row's high byte.
    #[inline]
    fn find_tile_data_high(&mut self) {
        self.update_wx_glitch();
        self.pipeline.last_tile_data_address = self.tile_data_address(1);
        self.pipeline.fetcher_state = FetcherState::GetTileDataHighT2;
    }

    /// `FetcherState::GetTileDataHighT2`: reads the tile row's high byte and tries to push the row.
    #[inline]
    fn read_tile_data_high(&mut self) {
        if self.pipeline.cgb_wx_glitch {
            self.pipeline.current_tile_data[1] = self.pipeline.current_tile_data[0];
            self.pipeline.fetcher_state = FetcherState::Push;
            if self.pipeline.wx_triggered {
                self.pipeline.window_tile_x = (self.pipeline.window_tile_x + 1) % TILE_MAP_WIDTH;
            }
            return;
        }

        let address = self.pipeline.last_tile_data_address as usize;
        let mut use_glitched = false;
        if self.pipeline.tile_sel_glitch {
            let (should_use, data) = self.tile_sel_glitch_data();
            use_glitched = should_use;
            self.pipeline.current_tile_data[1] = data;
        }
        if !use_glitched {
            let data = self.read_tile_data_byte();
            self.pipeline.current_tile_data[1] = data;
            self.pipeline.data_for_sel_glitch = data;
        }
        if self.pipeline.last_tileset && self.pipeline.tile_sel_glitch {
            self.pipeline.data_for_sel_glitch = self.ppu_vram_read(address);
        }
        if self.pipeline.wx_triggered {
            self.pipeline.window_tile_x = (self.pipeline.window_tile_x + 1) % TILE_MAP_WIDTH;
        }
        self.fetcher_push();
    }

    /// Pushes the fetched row into the background FIFO once it is empty, and restarts the
    /// fetcher. Until then the fetcher stays in `FetcherState::Push`.
    fn fetcher_push(&mut self) {
        self.pipeline.fetcher_state = FetcherState::Push;
        if self.pipeline.bg_fifo.size > 0 {
            return;
        }

        if self.window_y_triggered
            && !window_enabled(self.lcdc)
            && !self.is_cgb()
            && !self.pipeline.disable_window_pixel_insertion_glitch
        {
            // Positions before -7 wrap around to WX values above the end of the line.
            let mut logical_position = self.pipeline.wx_here();
            if logical_position > LINE_END_WX {
                logical_position = 0;
            }
            if self.wx == logical_position {
                self.pipeline.bg_fifo.push_blank_pixel();
                return;
            }
        }

        let pipeline = &mut self.pipeline;
        let attributes = pipeline.current_tile_attributes;
        pipeline.bg_fifo.push_row(
            pipeline.current_tile_data[0],
            pipeline.current_tile_data[1],
            attributes & PALETTE_BITS,
            is_bit_set(attributes, PRIORITY_BIT),
            is_bit_set(attributes, X_FLIP_BIT),
        );
        pipeline.fetcher_state = FetcherState::GetTileT1;
    }

    /// The VRAM address of the low byte of the current object's row on this line, taking 8x16
    /// objects, y flip and the CGB VRAM bank into account.
    fn object_line_address(&self) -> u16 {
        self.object_line_address_with(self.lcdc)
    }

    /// Like `object_line_address`, with the object size taken from `lcdc`.
    fn object_line_address_with(&self, lcdc: u8) -> u16 {
        let pipeline = &self.pipeline;
        let tall_objects = uses_tall_objects(lcdc);
        let mask = if tall_objects { 0xF } else { 7 };
        let object_y = pipeline.visible_object_y[pipeline.next_object()];
        let mut tile_y = self.ly.wrapping_sub(object_y) & mask;
        if is_bit_set(pipeline.object_flags, Y_FLIP_BIT) {
            tile_y ^= mask;
        }
        let tile = if tall_objects {
            pipeline.object_tile & 0xFE
        } else {
            pipeline.object_tile
        };
        let mut address = u16::from(tile) * TILE_BYTES + u16::from(tile_y) * 2;
        if self.uses_cgb_rendering() && is_bit_set(pipeline.object_flags, VRAM_BANK_BIT) {
            address += VRAM_BANK_SIZE;
        }
        address
    }

    /// Mixes the fetched object row into the object FIFO and moves on to the next object.
    fn overlay_current_object(&mut self) {
        let cgb_rendering = self.uses_cgb_rendering();
        let index_priority = cgb_rendering && !is_bit_set(self.cgb_opri, 0);
        let data_for_sel_glitch = self.ppu_vram_read(self.object_line_address() as usize + 1);
        let pipeline = &mut self.pipeline;
        let flags = pipeline.object_flags;
        let palette = if cgb_rendering {
            flags & PALETTE_BITS
        } else {
            u8::from(is_bit_set(flags, DMG_PALETTE_BIT))
        };
        let priority = if index_priority {
            pipeline.visible_object_indices[pipeline.next_object()]
        } else {
            0
        };
        pipeline.oam_fifo.overlay_object_row(
            pipeline.object_tile_data[0],
            pipeline.object_tile_data[1],
            palette,
            is_bit_set(flags, PRIORITY_BIT),
            priority,
            is_bit_set(flags, X_FLIP_BIT),
        );
        pipeline.data_for_sel_glitch = data_for_sel_glitch;
        pipeline.visible_object_count -= 1;
    }

    /// Shifts one pixel out of the FIFOs and, once past the scrolled-out pixels, writes it to the
    /// frame buffer. Does nothing while the background FIFO is empty or an object at x = 0 still
    /// has to be fetched.
    fn render_pixel_if_possible(&mut self) {
        let lcdc = self.lcdc;
        let object_at_zero_pending = self.pipeline.visible_object_count != 0
            && (objects_enabled(lcdc) || self.is_cgb())
            && self.pipeline.visible_object_x[self.pipeline.next_object()] == 0;
        if object_at_zero_pending || self.pipeline.bg_fifo.size == 0 {
            return;
        }

        let scx_fine = self.scx & 7;
        let pipeline = &mut self.pipeline;
        let bg_item = if pipeline.insert_bg_pixel {
            pipeline.insert_bg_pixel = false;
            FifoItem::default()
        } else {
            pipeline.bg_fifo.pop()
        };
        let mut bg_priority = bg_item.bg_priority;

        let mut object_item = None;
        if pipeline.oam_fifo.size > 0 {
            let item = pipeline.oam_fifo.pop();
            if item.color_id != 0 && objects_enabled(lcdc) {
                bg_priority |= item.bg_priority;
                object_item = Some(item);
            }
        }

        if pipeline.in_fine_scroll() {
            let fine_scroll_reached = pipeline.position_in_line & 7 == scx_fine
                || (pipeline.window_is_being_fetched
                    && pipeline.position_in_line & 7 == 6
                    && scx_fine == 7);
            if fine_scroll_reached {
                pipeline.position_in_line = POSITION_AFTER_FINE_SCROLL;
            } else if pipeline.position_in_line == POSITION_AFTER_FINE_SCROLL - 1 {
                pipeline.position_in_line = POSITION_LINE_START;
                return;
            } else {
                pipeline.line_has_fractional_scrolling = true;
            }
        }

        pipeline.window_is_being_fetched = false;
        pipeline.window_start_backup = None;

        if !pipeline.is_on_screen() {
            pipeline.position_in_line = pipeline.position_in_line.wrapping_add(1);
            return;
        }

        self.draw_pixel(bg_item, object_item, bg_priority);
    }

    /// Mixes a background pixel with the object pixel over it (if any), writes the result to the
    /// frame buffer and moves on to the next position. `bg_priority` is set when the background
    /// pixel is drawn over the object, unless it has color 0.
    fn draw_pixel(&mut self, bg_item: FifoItem, object_item: Option<FifoItem>, bg_priority: bool) {
        let color = self.mixed_pixel_color(
            self.pixel_colors(),
            self.uses_cgb_rendering(),
            self.bg_window_enabled(),
            bg_item,
            object_item,
            bg_priority,
        );
        self.put_pixel(color);
    }

    /// Whether the background and window are drawn (LCDC bit 0) at the current position.
    fn bg_window_enabled(&self) -> bool {
        match self.pipeline.written_bg_enable {
            Some(written) if self.pipeline.lcd_x == 0 => written,
            _ => {
                bg_and_window_enabled(self.lcdc)
                    && (self.is_cgb() || bg_and_window_enabled(self.previous_lcdc))
            }
        }
    }

    /// The color of the pixel that wins between a background pixel and the object pixel over it
    /// (if any), with the colors from `colors`. `cgb_rendering` is `uses_cgb_rendering` and
    /// `bg_window_enabled` is `bg_window_enabled`.
    /// Always inlined so that `colors` is a constant in it (see `PixelColors`).
    #[inline(always)]
    fn mixed_pixel_color(
        &self,
        colors: PixelColors,
        cgb_rendering: bool,
        bg_window_enabled: bool,
        bg_item: FifoItem,
        mut object_item: Option<FifoItem>,
        mut bg_priority: bool,
    ) -> Color {
        let mut bg_enabled = true;
        if !bg_window_enabled {
            if cgb_rendering {
                bg_priority = false;
            } else {
                bg_enabled = false;
            }
        }

        let bg_color_id = if bg_enabled { bg_item.color_id } else { 0 };
        if bg_color_id != 0 && bg_priority {
            object_item = None;
        }

        match object_item {
            Some(item) => self.object_color_in(colors, item),
            None => self.background_color_in(colors, bg_item.palette, bg_color_id),
        }
    }

    /// Writes a pixel to the frame buffer at the current position and moves on to the next one.
    #[inline(always)]
    fn put_pixel(&mut self, color: Color) {
        let pixel_index =
            usize::from(self.ly) * GB_SCREEN_WIDTH as usize + usize::from(self.pipeline.lcd_x);
        let pixel = self.frame_buffer.get_mut(pixel_index);
        debug_assert!(pixel.is_some(), "{FRAME_BUFFER_BOUNDS}");
        if let Some(pixel) = pixel {
            *pixel = color;
        }

        self.pipeline.position_in_line += 1;
        self.pipeline.lcd_x += 1;
    }

    /// Where pixels get their colors from now.
    fn pixel_colors(&self) -> PixelColors {
        if self.stop_blocks.palettes {
            PixelColors::Blocked
        } else if self.uses_cgb_rendering() {
            PixelColors::Cgb
        } else if self.is_cgb() {
            PixelColors::Compatibility
        } else {
            PixelColors::Dmg
        }
    }

    /// The color of a background or window pixel.
    fn background_color(&self, palette: u8, color_id: u8) -> Color {
        self.background_color_in(self.pixel_colors(), palette, color_id)
    }

    /// The color of a background or window pixel, with the colors from `colors`.
    /// Always inlined so that `colors` is a constant in it (see `PixelColors`).
    #[inline(always)]
    fn background_color_in(&self, colors: PixelColors, palette: u8, color_id: u8) -> Color {
        match colors {
            PixelColors::Blocked => BLACK,
            PixelColors::Cgb => self.palettes.cgb_bg_color(palette, color_id, false),
            PixelColors::Compatibility => self.palettes.cgb_bg_color(0, color_id, true),
            PixelColors::Dmg => self.palettes.dmg_bg_color(color_id),
        }
    }

    /// The color of an object pixel, with the colors from `colors`.
    /// Always inlined so that `colors` is a constant in it (see `PixelColors`).
    #[inline(always)]
    fn object_color_in(&self, colors: PixelColors, item: FifoItem) -> Color {
        let color = match colors {
            PixelColors::Blocked => return BLACK,
            PixelColors::Cgb => self
                .palettes
                .cgb_object_color(item.palette, item.color_id, false),
            PixelColors::Compatibility => {
                self.palettes
                    .cgb_object_color(item.palette, item.color_id, true)
            }
            PixelColors::Dmg => self.palettes.dmg_object_color(item.palette, item.color_id),
        };
        color.unwrap_or(WHITE)
    }

    /// Test helper: runs OAM scan and mode 3 for line `ly` in one go.
    #[cfg(test)]
    pub(super) fn write_scanline(&mut self) {
        self.previous_lcdc = self.lcdc;
        self.clear_visible_objects();
        self.scan_oam_entries(0..OAM_ENTRY_COUNT);
        self.window_y_triggered = self.window_y_triggered || self.ly >= self.wy;
        self.delayed_wy = self.wy;
        self.pipeline.reset_window_line();
        self.start_drawing();
        while !self.tick_drawing() {}
        self.end_drawing();
    }
}

impl Gpu {
    /// DMG: setting the window enable bit while a WX 166 start is pending for the next line
    /// already counts a window line, and the start counts another one (Gambatte's window
    /// on_screen wxA6_late_we_reenable tests).
    pub(super) fn window_enabled_with_continue_pending(&mut self) {
        if self.pipeline.window_continue_pending {
            self.pipeline.window_y = self.pipeline.window_y.wrapping_add(1);
        }
    }

    /// CGB: a window enable bit cleared before the first window pixel cancels the window start,
    /// and the background carries on from where it was (Gambatte's window late_disable tests).
    pub(super) fn cancel_window_start(&mut self) {
        let pipeline = &mut self.pipeline;
        if !pipeline.window_is_being_fetched {
            return;
        }
        if let Some(backup) = pipeline.window_start_backup.take() {
            pipeline.bg_fifo = backup.bg_fifo;
            pipeline.fetcher_state = backup.fetcher_state;
            pipeline.current_tile = backup.current_tile;
            pipeline.current_tile_attributes = backup.current_tile_attributes;
            pipeline.current_tile_data = backup.current_tile_data;
            pipeline.wx_triggered = false;
            pipeline.window_is_being_fetched = false;
        }
    }
}

#[cfg(test)]
mod tests;

mod plain_pixels;
