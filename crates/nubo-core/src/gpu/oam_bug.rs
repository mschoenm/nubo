/*
    The OAM corruption formulas are ported from SameBoy (https://github.com/LIJI32/SameBoy, Expat
    License, Copyright (c) 2015-2026 Lior Halphon; see the notice in apu.rs).
*/

//! The DMG OAM corruption bug.
//!
//! OAM (object attribute memory, 0xFE00-0xFE9F) holds 40 four-byte object entries. On DMG-family
//! hardware, when the CPU accesses OAM, or increments or decrements a 16-bit register that points
//! into it, while the PPU is reading OAM during mode 2 (OAM scan), the PPU's row gets mixed with
//! neighbouring rows. The corruption formulas, including the differences between DMG, MGB and
//! SGB2, follow SameBoy's OAM bug code. CGB hardware doesn't have the bug.

use crate::gpu::{Gpu, NO_OAM_SCAN_ROW};
use crate::model::Model;
use std::ops::Range;

/// The corruption mixes and copies OAM in rows of eight bytes (two objects).
const OAM_ROW_SIZE: usize = 8;

impl Gpu {
    /// Whether an access can corrupt OAM right now: the PPU must be scanning OAM on DMG-family
    /// hardware, at or past byte offset `min_row` (callers pass at least 8, so never in the first
    /// row).
    fn oam_bug_applies(&self, min_row: usize) -> bool {
        !self.is_cgb()
            && self.oam_scan_row != NO_OAM_SCAN_ROW
            && (self.oam_scan_row as usize) >= min_row
            && (self.oam_scan_row as usize) + 7 < self.object_attribute_memory.len()
    }

    /// Reads the little-endian 16-bit word at `offset` in OAM; the formulas mix words, not bytes.
    fn read_oam_word(&self, offset: usize) -> u16 {
        u16::from_le_bytes([
            self.object_attribute_memory[offset],
            self.object_attribute_memory[offset + 1],
        ])
    }

    /// Writes a little-endian 16-bit word at `offset` in OAM.
    fn write_oam_word(&mut self, offset: usize, value: u16) {
        let [low, high] = value.to_le_bytes();
        self.object_attribute_memory[offset] = low;
        self.object_attribute_memory[offset + 1] = high;
    }

    /// Copies the bytes in `byte_range` of the row starting at offset `from_row` to the same bytes
    /// of the row at `to_row`.
    fn copy_oam_bytes(&mut self, from_row: usize, to_row: usize, byte_range: Range<usize>) {
        self.object_attribute_memory.copy_within(
            from_row + byte_range.start..from_row + byte_range.end,
            to_row + byte_range.start,
        );
    }

    /// Corrupts OAM like a CPU write (or a 16-bit increment or decrement) during OAM scan.
    pub(crate) fn trigger_oam_bug_write(&mut self) {
        if self.oam_bug_applies(8) {
            let row = self.oam_scan_row as usize;
            let previous_row = row - OAM_ROW_SIZE;

            let a = self.read_oam_word(row);
            let b = self.read_oam_word(previous_row);
            let c = self.read_oam_word(previous_row + 4);

            let corrupted = ((a ^ c) & (b ^ c)) ^ c;
            self.write_oam_word(row, corrupted);
            self.copy_oam_bytes(previous_row, row, 2..8);
        }
    }

    /// Corrupts OAM like a CPU read during OAM scan.
    pub(crate) fn trigger_oam_bug_read(&mut self) {
        if !self.oam_bug_applies(8) {
            return;
        }

        let row = self.oam_scan_row as usize;
        // SameBoy limits the first two cases to offsets below 0x98, but the last row, 0x98, always
        // takes the third.
        match row & 0x18 {
            0x10 => self.secondary_read_corruption(row),
            0x00 => self.row_aligned_read_corruption(row),
            _ => {
                let corrupted = glitch_read(
                    self.read_oam_word(row),
                    self.read_oam_word(row - 8),
                    self.read_oam_word(row - 4),
                );
                self.write_oam_word(row - 8, corrupted);
                self.write_oam_word(row, corrupted);
            }
        }

        self.copy_oam_bytes(row - 8, row, 0..8);
        if row == 0x80 || (self.model == Model::Mgb && row == 0x40) {
            self.copy_oam_bytes(row, 0, 0..8);
        }
    }

    /// The read corruption when the PPU's byte offset has `row & 0x18 == 0x10`, which also
    /// overwrites the eight bytes 16 before it.
    fn secondary_read_corruption(&mut self, row: usize) {
        let corrupted = glitch_read_secondary(
            self.read_oam_word(row - 16),
            self.read_oam_word(row - 8),
            self.read_oam_word(row),
            self.read_oam_word(row - 4),
        );
        self.write_oam_word(row - 8, corrupted);
        self.copy_oam_bytes(row - 8, row - 16, 0..8);
    }

    /// The read corruption when the PPU's byte offset has `row & 0x18 == 0`, which mixes in words
    /// up to 32 bytes before it, also overwrites the eight bytes 16 and 32 before it, and differs
    /// between DMG, MGB and SGB2.
    fn row_aligned_read_corruption(&mut self, row: usize) {
        let corrupted = if self.model == Model::Mgb {
            self.tertiary_read_glitch(row, glitch_tertiary_read_3)
        } else if row == 0x40 {
            let glitch = if self.model == Model::Sgb2 {
                glitch_quaternary_read_sgb2
            } else {
                glitch_quaternary_read_dmg
            };
            glitch(
                self.read_oam_word(0),
                self.read_oam_word(row),
                self.read_oam_word(row - 4),
                self.read_oam_word(row - 6),
                self.read_oam_word(row - 8),
                self.read_oam_word(row - 14),
                self.read_oam_word(row - 16),
                self.read_oam_word(row - 32),
            )
        } else if self.model == Model::Sgb2 {
            self.tertiary_read_glitch(row, glitch_tertiary_read_2)
        } else {
            let glitch = match row {
                0x20 => glitch_tertiary_read_2,
                0x60 => glitch_tertiary_read_3,
                _ => glitch_tertiary_read_1,
            };
            self.tertiary_read_glitch(row, glitch)
        };
        self.write_oam_word(row - 8, corrupted);
        self.copy_oam_bytes(row - 8, row - 16, 0..8);
        self.copy_oam_bytes(row - 8, row - 32, 0..8);
    }

    /// Applies one of the five-word read formulas to the words at offset `row` and 4, 8, 16 and 32
    /// bytes before it.
    fn tertiary_read_glitch(&self, row: usize, glitch: fn(u16, u16, u16, u16, u16) -> u16) -> u16 {
        glitch(
            self.read_oam_word(row),
            self.read_oam_word(row - 4),
            self.read_oam_word(row - 8),
            self.read_oam_word(row - 16),
            self.read_oam_word(row - 32),
        )
    }

    /// Corrupts OAM like an instruction that both reads and increments or decrements a register
    /// pointing into OAM in the same cycle, such as `LD A, (HL+)`.
    pub(crate) fn trigger_oam_bug_mixed(&mut self) {
        if self.oam_bug_applies(32) {
            let row = self.oam_scan_row as usize;
            let previous_row = row - 8;
            let row_before_previous = row - 16;

            let a = self.read_oam_word(row_before_previous);
            let b = self.read_oam_word(previous_row);
            let c = self.read_oam_word(row);
            let d = self.read_oam_word(previous_row + 4);

            let corrupted = (b & (a | c | d)) | (a & c & d);
            self.write_oam_word(previous_row, corrupted);
            self.copy_oam_bytes(previous_row, row, 0..8);
            self.copy_oam_bytes(previous_row, row_before_previous, 0..8);

            self.trigger_oam_bug_read();
        }
    }
}

// The glitch formulas keep SameBoy's one-letter names for the OAM words they mix, so they can be
// compared with SameBoy's side by side. Each doc comment says which word each letter is, as a byte
// offset from the row the PPU is reading.

/// The plain read corruption: `a` is the row's word, `b` the word 8 bytes before it and `c` the
/// word 4 bytes before it.
fn glitch_read(a: u16, b: u16, c: u16) -> u16 {
    b | (a & c)
}

/// The secondary read corruption: `a` is the word 16 bytes before the row, `b` the word 8 bytes
/// before it, `c` the row's word and `d` the word 4 bytes before it.
fn glitch_read_secondary(a: u16, b: u16, c: u16, d: u16) -> u16 {
    (b & (a | c | d)) | (a & c & d)
}

/// The DMG's usual row-aligned read corruption: `a` is the row's word and `b`, `c`, `d` and `e`
/// the words 4, 8, 16 and 32 bytes before it (`tertiary_read_glitch`).
fn glitch_tertiary_read_1(a: u16, b: u16, c: u16, d: u16, e: u16) -> u16 {
    c | (a & b & d & e)
}

/// The row-aligned read corruption of the SGB2, and of the DMG at offset 0x20, with the same words
/// as `glitch_tertiary_read_1`.
fn glitch_tertiary_read_2(a: u16, b: u16, c: u16, d: u16, e: u16) -> u16 {
    (c & (a | b | d | e)) | (a & b & d & e)
}

/// The row-aligned read corruption of the MGB, and of the DMG at offset 0x60, with the same words
/// as `glitch_tertiary_read_1`.
fn glitch_tertiary_read_3(a: u16, b: u16, c: u16, d: u16, e: u16) -> u16 {
    (c & (a | b | d | e)) | (b & d & e)
}

/// The DMG's read corruption at offset 0x40: `a` is the first word of OAM (unused here), `b` the
/// row's word, and `c`, `d`, `e`, `f`, `g` and `h` the words 4, 6, 8, 14, 16 and 32 bytes before
/// it.
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors SameBoy's formula, which combines eight OAM words"
)]
fn glitch_quaternary_read_dmg(
    _a: u16,
    b: u16,
    c: u16,
    d: u16,
    e: u16,
    f: u16,
    g: u16,
    h: u16,
) -> u16 {
    (e & (h | g | (!d & f) | c | b)) | (c & g & h)
}

/// The SGB2's read corruption at offset 0x40, with the same words as `glitch_quaternary_read_dmg`
/// (`d` unused here).
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors SameBoy's formula, which combines eight OAM words"
)]
fn glitch_quaternary_read_sgb2(
    a: u16,
    b: u16,
    c: u16,
    _d: u16,
    e: u16,
    f: u16,
    g: u16,
    h: u16,
) -> u16 {
    (e & (h | g | c | (a & b))) | ((c & g & h) & (b | a | !f))
}
