//! The PPU (picture processing unit), which the code calls the GPU.
//!
//! The PPU draws the screen one line at a time. A frame has 154 lines of 456 dots each (a dot is
//! one tick of the 4 MHz clock; an M-cycle, the CPU's unit of time, is four dots). Lines 0-143 are
//! visible and go through three modes: mode 2 (OAM scan, finding the objects on the line), mode 3
//! (drawing pixels) and mode 0 (horizontal blank). Lines 144-153 are mode 1 (vertical blank).
//!
//! This module runs that timeline dot by dot, with the window's WY checks, the vblank interrupt,
//! switching the LCD on and off, and the PPU's registers. Its dot positions are calibrated against
//! the Mooneye PPU tests and an instrumented SameBoy. The rest is in its submodules: STAT and
//! LY=LYC with the STAT interrupt (`stat.rs`), when the CPU may access OAM and VRAM and how OAM
//! DMA and STOP affect the PPU (`access.rs`), the shortcuts that run many dots at once
//! (`steady.rs`), save states (`save_state.rs`), the palette registers and colors
//! (`palettes.rs`), the DMG OAM corruption bug (`oam_bug.rs`), LCDC's bits (`lcdc.rs`), the
//! screen's size (`constants.rs`), and the renderer that produces the pixels in mode 3
//! (`pipeline.rs`).
//!
//! Which timing rules apply depends on the model and speed (`TimingVariant`). The start of each
//! line (when STAT shows each mode, LY=LYC, the mode 0 and mode 2 interrupt sources, line 153)
//! and the window's WY checks follow Gambatte's timing at both speeds, measured by comparing the
//! CPU's register reads and writes and the interrupt requests in both emulators.

pub use crate::gpu::palettes::DMG_SHADES;
pub(crate) use crate::gpu::palettes::rgb555_to_rgba;

use crate::address_bus::MemoryMapped;
use crate::address_bus::hdma::HdmaState;
pub(crate) use crate::gpu::constants::{GB_SCREEN_HEIGHT, GB_SCREEN_WIDTH};
use crate::gpu::lcdc::{lcd_enabled, objects_enabled, window_enabled};
use crate::gpu::palettes::{Color, Palettes, WHITE};
use crate::gpu::pipeline::Pipeline;
use crate::gpu::save_state::ModelSgb;
use crate::model::Model;
use crate::serializable::{Serializable, read_name, unreadable_value, write_variant};
use crate::sgb::Sgb;
use crate::utils::is_bit_set;
use nubo_serializable_derive::Serializable;

/// Which part of the line the PPU is working on. The mode the CPU reads from STAT can differ
/// from it for a few dots, so that is tracked separately in `Gpu::visible_mode`. Save states
/// store a variant by its name. The order of the variants isn't the order of the mode numbers
/// STAT shows for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serializable)]
enum LineState {
    OamScan,
    Drawing,
    HBlank,
    VBlank,
}

/// Which of the PPU's timing rules apply. Many dots of the line differ between the DMG family
/// (DMG, MGB, SGB), the CGB (and AGB) at normal speed and the CGB in double speed, where the CPU
/// runs twice as fast and an M-cycle is two dots instead of four. The model's revision decides the
/// rest (`Gpu::model`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serializable)]
enum TimingVariant {
    Dmg,
    Cgb,
    CgbDoubleSpeed,
}

/// What the CGB's PPU can't read while the CPU is in STOP mode: whatever the CPU could access
/// when STOP started (SameBoy). It then reads 0xFF from OAM and VRAM, and draws black instead of
/// palette colors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serializable)]
pub(crate) struct StopBlocks {
    pub(crate) oam: bool,
    pub(crate) vram: bool,
    pub(crate) palettes: bool,
}

/// The PPU's countdowns: dots until a delayed effect of the line's timeline or of a register write
/// happens, each 0 while it isn't counting. `Gpu::tick_dot` counts them all down every dot (see
/// `count_down`), so every such countdown belongs here: `Gpu::has_steady_shape` only lets the dots
/// run in bulk while all of them are idle, which this way can't miss one. The countdowns that only
/// run in one part of the line are kept with it instead: `Gpu::mode_0_interrupt_delay` and
/// `Gpu::hdma_start_delay` in HBlank, `Gpu::fetcher_start_delay` in mode 3.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serializable)]
struct Countdowns {
    /// Dots until the mode 0 interrupt source starts while the last pixel is still being drawn.
    early_hblank: u8,
    /// Dots until a STAT interrupt raised by a delayed LYC write is requested.
    lyc_interrupt: u8,
    /// Dots until `Gpu::next_oam_dma_access` takes over, plus one.
    oam_dma_access: u8,
    /// Dots left before VRAM is blocked in mode 3 of the first line after the LCD is switched on.
    first_line_vram_grace: u8,
    /// Dots until palette RAM is blocked after mode 3 starts.
    palette_block: u8,
    /// Dots until palette RAM is unblocked after drawing ends.
    palette_unblock: u8,
    /// Dots until `Gpu::delayed_wy` takes the value of WY after a write.
    delayed_wy_update: u8,
    /// Dots the LY=LYC interrupt source stays active after an LYC write ended the match.
    lyc_source_hold: u8,
    /// Dots until `Gpu::compared_lyc` takes the value of LYC after a CGB write.
    lyc_write: u8,
    /// Dots until `Gpu::late_cleared_stat_sources` stop feeding the STAT interrupt line.
    stat_clear: u8,
}

impl Countdowns {
    /// Whether none of the countdowns is counting.
    fn is_idle(&self) -> bool {
        *self == Countdowns::default()
    }
}

/// Counts `countdown` down by one dot, unless it is idle, and returns whether it just ran out.
fn count_down(countdown: &mut u8) -> bool {
    if *countdown == 0 {
        return false;
    }
    *countdown -= 1;
    *countdown == 0
}

/// A byte OAM DMA copied, on its way to OAM (see `Gpu::write_oam_from_dma`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serializable)]
struct DmaOamWrite {
    /// The offset in OAM, below 0xA0.
    index: u16,
    value: u8,
}

/// How OAM DMA affects the PPU's own reads of OAM.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum OamDmaAccess {
    /// No transfer: the PPU reads OAM normally.
    #[default]
    Free,
    /// A transfer is requested or running. It holds OAM, so OAM scan reads nothing and the
    /// object attribute reads of mode 3 see the pair of bytes at `destination`, the next byte the
    /// transfer writes (when it is between 1 and 159).
    Running { destination: u8 },
    /// A transfer stands still because the CPU is halted.
    PausedByHalt,
}

/// Stored as the variant's name and the destination (0 when there is none), like a derived enum.
impl Serializable for OamDmaAccess {
    fn serialize(&self, writer: &mut Vec<u8>) -> std::io::Result<()> {
        let (variant, destination) = match *self {
            OamDmaAccess::Free => ("Free", 0u8),
            OamDmaAccess::Running { destination } => ("Running", destination),
            OamDmaAccess::PausedByHalt => ("PausedByHalt", 0),
        };
        write_variant(writer, variant, &["Free", "Running", "PausedByHalt"])?;
        destination.serialize(writer)
    }

    fn deserialize(&mut self, reader: &mut &[u8]) -> std::io::Result<()> {
        let variant = read_name(reader)?;
        let mut destination = 0u8;
        destination.deserialize(reader)?;
        *self = match variant {
            "Free" => OamDmaAccess::Free,
            "Running" => OamDmaAccess::Running { destination },
            "PausedByHalt" => OamDmaAccess::PausedByHalt,
            _ => {
                return Err(unreadable_value(format!(
                    "unknown OAM DMA access state {variant}"
                )));
            }
        };
        Ok(())
    }

    fn encoding() -> String {
        String::from("variant and u8")
    }
}

/// The PPU: its registers, VRAM, OAM, palettes, the line timeline and the frame buffers.
///
/// `Debug` is derived on purpose and shows everything, the big arrays included: `cargo test-roms
/// --verify-boot` compares the whole emulator's `Debug` output to check that starting a test from
/// a copy of the booted emulator matches a real boot, so a custom `Debug` that left out VRAM, OAM
/// or the frame buffers would weaken that check.
#[derive(Clone, Debug)]
pub(crate) struct Gpu {
    /// The line the PPU is on, 0 to 153. Unlike `ly`, it doesn't reset early on line 153.
    line: u8,

    /// The dot within the current line, 0 to 455.
    line_dot: u16,

    line_state: LineState,

    /// The mode the CPU reads from STAT bits 0-1: 0 = hblank, 1 = vblank, 2 = OAM scan,
    /// 3 = drawing.
    visible_mode: u8,

    /// Whether this is the first line after the LCD was switched on. That line skips OAM scan
    /// and starts a few dots late.
    first_line_after_lcd_on: bool,
    /// The frame being drawn is the first since the LCD was switched on. The screen doesn't show
    /// it, so it is published blank (little-things-gb's firstwhite test, checked on hardware).
    first_frame_after_lcd_on: bool,

    /// The LY=LYC coincidence flag, STAT bit 2.
    lyc_flag: bool,
    /// The LY=LYC flag as it was at the end of the previous dot. CGB-D and newer show this one in
    /// STAT, a dot behind the comparison that requests interrupts (Wilbert Pol's ly_lyc tests,
    /// verified on CGB and AGS).
    previous_lyc_flag: bool,

    /// The level of the internal STAT interrupt line. An interrupt is only requested when it
    /// rises, so overlapping sources don't trigger twice ("STAT blocking").
    stat_line_high: bool,

    /// Whether LY has matched WY (the window's top line) at one of the line's checks this frame,
    /// which allows the window to be drawn.
    window_y_triggered: bool,
    /// WY as mode 3 sees it when the window would start: the written value reaches it a few dots
    /// late. A match here starts the window on the current line only.
    delayed_wy: u8,

    /// Whether line 0's mode 2 interrupt was swallowed because the vblank and mode 2 interrupts
    /// were both enabled when it was due.
    line_0_oam_interrupt_blocked: bool,
    /// STAT's interrupt enable bits when the current line started (in double speed, as its second
    /// dot starts). Measured against Gambatte, a source that a STAT write enables at a line's start
    /// doesn't request an interrupt there, while one that was already enabled still blocks the
    /// others.
    stat_at_line_start: u8,
    /// On DMG and in double speed, the line the LY=LYC interrupt source compares LYC with after a
    /// write around a change of line (in a line's last dots or first two, or on line 153 before it
    /// compares with 0), until the comparison itself reaches that line (see
    /// `Gpu::adjust_lyc_source_after_write`).
    lyc_source_line: Option<u8>,

    /// Dots counted while the PPU doesn't draw (the LCD is off, or STOP stopped a monochrome
    /// model's PPU), used to keep producing frames at the normal rate.
    idle_dots: u32,
    /// The CPU is in STOP mode. Monochrome models stop the PPU and show a white screen; the CGB
    /// keeps drawing, cut off from what `stop_blocks` says.
    stopped: bool,
    stop_blocks: StopBlocks,

    /// Dots until HDMA (CGB's hblank-driven VRAM copy) learns that hblank started.
    hdma_start_delay: u8,
    /// The LCD was just switched off outside HBlank. STAT drops to mode 0, which HDMA takes as
    /// the start of an HBlank once the PPU next runs (SameBoy; Gambatte's dma hdma_disable_display
    /// tests).
    lcd_off_hblank_dma: bool,

    /// Dots between entering mode 3 and the renderer starting.
    fetcher_start_delay: u8,

    /// Dots after mode 3 ends before the hblank STAT interrupt source becomes active.
    mode_0_interrupt_delay: u8,
    /// The mode 0 interrupt source started before mode 3 ended.
    early_hblank_interrupt: bool,

    pipeline: Pipeline,

    lcdc: u8,

    /// LCDC as it was on the previous dot. Some renderer reads see a write one dot late.
    previous_lcdc: u8,

    scy: u8,

    scx: u8,

    wx: u8,

    wy: u8,

    ly: u8,

    lyc: u8,

    /// The LYC value the LY=LYC comparison uses. A CGB LYC write at normal speed reaches it up to
    /// `CGB_LYC_WRITE_DELAY_DOTS` after the register changes.
    compared_lyc: u8,

    stat: u8,
    /// STAT interrupt sources a CGB STAT write just disabled, which keep feeding the STAT
    /// interrupt line for `Countdowns::stat_clear` more dots.
    late_cleared_stat_sources: u8,

    palettes: Palettes,

    /// VBK (0xFF4F): bit 0 selects which of the two CGB VRAM banks the CPU sees.
    cgb_vbk: u8,

    /// OPRI (0xFF6C): bit 0 set gives objects DMG-style priority by x position instead of the
    /// CGB's priority by OAM index.
    cgb_opri: u8,

    /// KEY0 (0xFF4C): written by the CGB boot ROM; 0x04 selects DMG compatibility mode.
    key0: u8,

    /// The frame being drawn, one RGBA color per pixel.
    frame_buffer: Vec<Color>,

    /// Video RAM: 8 KiB on DMG, two 8 KiB banks on CGB.
    video_ram: [u8; 0x4000],

    /// OAM (object attribute memory): 40 four-byte entries describing the objects (sprites).
    object_attribute_memory: [u8; 0xa0],

    /// The hardware model; `timing` follows from it and the CPU's speed.
    model: Model,
    timing: TimingVariant,

    /// The CPU can't reach CGB palette RAM through BCPD and OCPD while the PPU draws: reads
    /// return 0xFF and writes are lost, though an auto-incrementing index still moves on.
    palette_ram_blocked: bool,

    countdowns: Countdowns,

    /// The last finished frame, copied from `frame_buffer` at the start of vblank. Frontends pull
    /// it when they are ready: `Emulator::frame_buffer` returns the latest frame, and
    /// `Emulator::take_frame` returns it only once per new frame (see `frame_ready`).
    completed_frame: Vec<u8>,

    /// Set when `completed_frame` holds a frame the frontend hasn't taken yet.
    frame_ready: bool,

    /// The STAT interrupt request flag (IF bit 1).
    stat_interrupt: bool,

    /// The vblank interrupt request flag (IF bit 0).
    vblank_interrupt: bool,

    /// How OAM DMA affects the PPU's OAM reads now. The address bus sets it every M-cycle.
    oam_dma_access: OamDmaAccess,
    /// The access that takes over when `Countdowns::oam_dma_access` runs out.
    next_oam_dma_access: OamDmaAccess,
    /// The OAM index and value OAM DMA copied in this M-cycle, until it reaches OAM.
    pending_dma_oam_write: Option<DmaOamWrite>,

    /// The object Y and X the OAM scan last read. While OAM DMA holds OAM the scan reads nothing
    /// and checks these stale values again instead (SameBoy).
    oam_scan_y_bus: u8,
    oam_scan_x_bus: u8,

    /// The OAM byte offset the PPU is reading during OAM scan, or `NO_OAM_SCAN_ROW`. The OAM
    /// corruption bug (`oam_bug.rs`) uses it.
    oam_scan_row: u8,

    /// The Super Nintendo around a Super Game Boy, which colors each finished frame and draws
    /// its border (`sgb.rs`). Other models have none.
    sgb: ModelSgb,
}

const HBLANK_MODE: u8 = 0;
const VBLANK_MODE: u8 = 1;
const OAM_MODE: u8 = 2;
const VRAM_MODE: u8 = 3;

/// Value of `oam_scan_row` when the PPU isn't reading OAM.
pub(super) const NO_OAM_SCAN_ROW: u8 = 0xFF;

const DOTS_PER_LINE: u16 = 456;
/// PPU dots in one frame, including the lines of VBlank.
pub(crate) const DOTS_PER_FRAME: u32 = DOTS_PER_LINE as u32 * FRAME_SCANLINE_COUNT as u32;
const FRAME_SCANLINE_COUNT: u8 = 154;
const VBLANK_START_LINE: u8 = GB_SCREEN_HEIGHT as u8;
const LAST_VISIBLE_LINE: u8 = VBLANK_START_LINE - 1;
const LAST_LINE: u8 = FRAME_SCANLINE_COUNT - 1;

/// The dot of lines 0-143 at which STAT starts showing mode 2, on every model and speed. Like the
/// other STAT and LY=LYC dots below, it is measured against Gambatte, which passes the lcd_offset
/// tests: tests that don't shift the LCD's phase only read STAT every four dots.
const MODE_2_STAT_DOT: u16 = 1;
/// At normal speed STAT shows mode 3 from this dot of OAM scan, before drawing starts.
const MODE_3_STAT_DOT: u16 = 81;
/// Line 0's mode 2 interrupt is only due at this dot, on every model and speed (measured against
/// Gambatte).
const LINE_0_OAM_SOURCE_DOT: u16 = 2;
/// In double speed, VRAM reads and writes are blocked from this dot of OAM scan, two dots before
/// drawing starts (AGE's stat-mode-ds and vram-read tests; Gambatte's vram_m3 _ds tests).
const DOUBLE_SPEED_VRAM_BLOCK_DOT: u16 = DRAWING_START_DOT - 2;
/// In double speed STAT shows mode 3 from this dot, a dot before VRAM is blocked, which only reads
/// that land on odd dots see (Gambatte's m3stat_count, m2int_m2stat _ds and lcd_offset
/// offset1_lyc99int_m3stat_count_ds tests).
const DOUBLE_SPEED_MODE_3_STAT_DOT: u16 = DOUBLE_SPEED_VRAM_BLOCK_DOT - 1;
/// In double speed a line samples STAT's interrupt enables as this dot starts, so a write landing
/// in its first dot still counts: enabling the mode 2 source there keeps line 144's STAT line high
/// from an LY=LYC match into mode 1, while enabling it a dot later doesn't (measured against
/// Gambatte; Gambatte's m1irq_m2enable_lyc_ds and ly143_late_m2enable_ds_lcdoffset1 tests).
const DOUBLE_SPEED_STAT_SAMPLE_DOT: u16 = 1;
/// The dot of line 144 at which STAT shows mode 1 and the vblank interrupt is requested.
const VBLANK_START_DOT: u16 = 2;
const OAM_SCAN_END_DOT: u16 = 80;
const DRAWING_START_DOT: u16 = 84;
const FETCHER_START_DELAY_DOTS: u8 = 2;
/// On the first line after the LCD is switched on, the renderer starts two dots later than on
/// other lines, so mode 3 ends two dots later too (AGE's stat-mode, oam and vram tests;
/// Gambatte's enable_display tests; GBMicrotest's int_hblank tests).
const LCD_ON_FETCHER_START_DELAY_DOTS: u8 = 4;
// `Gpu::enter_mode_3` sets one of these delays and leaves starting the renderer to
// `Gpu::tick_mode_3`, which only starts it as the delay runs out.
const _: () = assert!(FETCHER_START_DELAY_DOTS > 0 && LCD_ON_FETCHER_START_DELAY_DOTS > 0);
const MODE_0_INTERRUPT_DELAY_DOTS: u8 = 1;
/// The PPU blocks CGB palette RAM from the first dot of mode 3 until these dots after drawing
/// ends, fitted to Gambatte's cgbpal_m3 tests (SameBoy has the same blocking with slightly
/// different timing).
const PALETTE_UNBLOCK_DOTS: u8 = 4;
/// In double speed palette RAM is already blocked a dot before mode 3 starts, which only accesses
/// that land on odd dots see (Gambatte's cgbpal_m3 m3start_ds_lcdoffset1 tests).
const DOUBLE_SPEED_PALETTE_BLOCK_DOT: u16 = DRAWING_START_DOT - 1;
const DOUBLE_SPEED_PALETTE_UNBLOCK_DOTS: u8 = 3;
/// How far into mode 3 of the first line after LCD on the CGB blocks palette RAM and VRAM.
const FIRST_LINE_PALETTE_BLOCK_DOTS: u8 = 2;
const FIRST_LINE_VRAM_BLOCK_DOTS: u8 = 3;
/// Dots after a write to WY until mode 3 sees the new value (`delayed_wy`).
const DMG_DELAYED_WY_DOTS: u8 = 2;
const CGB_DELAYED_WY_DOTS: u8 = 5;
const DOUBLE_SPEED_DELAYED_WY_DOTS: u8 = 1;
/// The dots at which the PPU compares WY with LY and remembers a match for the rest of the frame,
/// matching what Gambatte does: line 0 starts a new frame's check at the first constant, lines
/// 1-143 check their own number at the second, and lines 0-142 check it again near their end at
/// the third (the hardware tests of Gambatte's window late_wy and late_enable_afterVblank
/// families). The dots differ between models and speeds because WY and LCDC writes land at
/// different points.
const DMG_WINDOW_Y_CHECK_DOTS: (u16, u16, u16) = (4, 1, 453);
const CGB_WINDOW_Y_CHECK_DOTS: (u16, u16, u16) = (4, 0, 452);
const DOUBLE_SPEED_WINDOW_Y_CHECK_DOTS: (u16, u16, u16) = (5, 1, 453);
/// The dot the first line starts at after the LCD is switched on.
const DMG_LCD_ON_START_DOT: u16 = 3;
const CGB_LCD_ON_START_DOT: u16 = 4;
const LAST_LINE_ZERO_COMPARE_DOT: u16 = 10;
const HDMA_START_DELAY_DOTS: u8 = 2;
/// An hblank HDMA started from this dot of the line on waits for the next line's hblank
/// (Gambatte's dma hdma_late_enable lcdoffset tests).
const HBLANK_DMA_START_END_DOT: u16 = 453;
const DOUBLE_SPEED_HBLANK_DMA_START_END_DOT: u16 = 455;

/// How many more dots the STAT interrupt sources a CGB STAT write at normal speed disables stay
/// active (see `Gpu::cgb_late_cleared_stat_sources`).
pub(super) const CGB_STAT_CLEAR_DELAY_DOTS: u8 = 2;

const LYC_STAT_SOURCE_BIT: u8 = 6;
const OAM_MODE_STAT_SOURCE_BIT: u8 = 5;
const VBLANK_MODE_STAT_SOURCE_BIT: u8 = 4;
const HBLANK_MODE_STAT_SOURCE_BIT: u8 = 3;
const STAT_WRITABLE_BITS: u8 = 0b0111_1000;

/// `dots`, but at most `limit`.
fn dots_up_to(dots: u32, limit: u16) -> u16 {
    u16::try_from(dots.min(u32::from(limit))).unwrap_or(limit)
}

/// A white frame to draw into.
fn blank_pixels() -> Vec<Color> {
    vec![WHITE; (GB_SCREEN_WIDTH * GB_SCREEN_HEIGHT) as usize]
}

impl Gpu {
    pub(super) fn new() -> Self {
        Gpu {
            line: 0,
            line_dot: 0,
            line_state: LineState::HBlank,
            visible_mode: HBLANK_MODE,
            palette_ram_blocked: false,
            countdowns: Countdowns::default(),
            first_line_after_lcd_on: false,
            first_frame_after_lcd_on: false,
            lyc_flag: false,
            previous_lyc_flag: false,
            stat_line_high: false,
            window_y_triggered: false,
            delayed_wy: 0,
            line_0_oam_interrupt_blocked: false,
            stat_at_line_start: 0,
            lyc_source_line: None,
            idle_dots: 0,
            stopped: false,
            stop_blocks: StopBlocks::default(),
            hdma_start_delay: 0,
            lcd_off_hblank_dma: false,
            fetcher_start_delay: 0,
            mode_0_interrupt_delay: 0,
            early_hblank_interrupt: false,
            pipeline: Pipeline::default(),
            lcdc: 0,
            previous_lcdc: 0,
            scy: 0,
            scx: 0,
            wx: 0,
            wy: 0,
            ly: 0,
            lyc: 0,
            compared_lyc: 0,
            stat: 0,
            late_cleared_stat_sources: 0,
            palettes: Palettes::default(),
            cgb_vbk: 0,
            cgb_opri: 0,
            key0: 0,
            frame_buffer: blank_pixels(),
            video_ram: [0; 0x4000],
            object_attribute_memory: [0; 0xa0],
            model: Model::Dmg,
            timing: TimingVariant::Dmg,
            completed_frame: blank_pixels().into_flattened(),
            frame_ready: false,
            stat_interrupt: false,
            vblank_interrupt: false,
            oam_dma_access: OamDmaAccess::Free,
            next_oam_dma_access: OamDmaAccess::Free,
            pending_dma_oam_write: None,
            oam_scan_y_bus: 0,
            oam_scan_x_bus: 0,
            oam_scan_row: NO_OAM_SCAN_ROW,
            sgb: ModelSgb::default(),
        }
    }

    /// Selects the hardware model, which decides the PPU's timing along with the CPU's speed.
    pub(super) fn set_model(&mut self, model: Model) {
        self.model = model;
        self.sgb = ModelSgb(model.is_sgb().then(|| Box::new(Sgb::new())));
        self.timing = if model.is_cgb() {
            TimingVariant::Cgb
        } else {
            TimingVariant::Dmg
        };
    }

    /// Called when the CGB's CPU switches between normal and double speed.
    pub(super) fn set_cgb_double_speed(&mut self, value: bool) {
        debug_assert!(
            self.model.is_cgb() || !value,
            "only the CGB has a double speed"
        );
        self.timing = if value {
            TimingVariant::CgbDoubleSpeed
        } else if self.model.is_cgb() {
            TimingVariant::Cgb
        } else {
            TimingVariant::Dmg
        };
    }

    /// Whether the PPU's timing follows the CPU's speed, as it always does. A loaded save state
    /// where it doesn't is damaged.
    pub(super) fn matches_speed(&self, cgb_double_speed: bool) -> bool {
        self.in_double_speed() == cgb_double_speed
    }

    /// Whether this is a CGB's PPU (the AGB's included), with VRAM banks, palette RAM and its own
    /// timing, also while it runs a DMG game.
    fn is_cgb(&self) -> bool {
        self.model.is_cgb()
    }

    fn in_double_speed(&self) -> bool {
        self.timing == TimingVariant::CgbDoubleSpeed
    }

    pub(super) fn palettes_mut(&mut self) -> &mut Palettes {
        &mut self.palettes
    }

    /// The last finished frame, four RGBA bytes per pixel.
    pub(crate) fn completed_frame(&self) -> &[u8] {
        &self.completed_frame
    }

    /// The Super Game Boy's side of the console, on the Super Game Boy models.
    pub(crate) fn sgb(&self) -> Option<&Sgb> {
        self.sgb.0.as_deref()
    }

    pub(crate) fn sgb_mut(&mut self) -> Option<&mut Sgb> {
        self.sgb.0.as_deref_mut()
    }

    /// On a Super Game Boy, draws the last picture again with the menu's settings as they are now
    /// (`Sgb::redraw`).
    pub(crate) fn redraw_sgb_picture(&mut self) {
        if let Some(sgb) = &mut self.sgb.0 {
            sgb.redraw(&mut self.completed_frame);
        }
    }

    /// Whether `completed_frame` holds a frame the frontend hasn't taken yet.
    pub(crate) fn frame_ready(&self) -> bool {
        self.frame_ready
    }

    pub(crate) fn set_frame_ready(&mut self, value: bool) {
        self.frame_ready = value;
    }

    /// The STAT interrupt request flag (IF bit 1).
    pub(super) fn stat_interrupt(&self) -> bool {
        self.stat_interrupt
    }

    pub(super) fn set_stat_interrupt(&mut self, value: bool) {
        self.stat_interrupt = value;
    }

    /// The vblank interrupt request flag (IF bit 0).
    pub(super) fn vblank_interrupt(&self) -> bool {
        self.vblank_interrupt
    }

    pub(super) fn set_vblank_interrupt(&mut self, value: bool) {
        self.vblank_interrupt = value;
    }

    fn lcd_enabled(&self) -> bool {
        lcd_enabled(self.lcdc)
    }

    /// Whether STAT shows HBlank (mode 0) and the HBlank DMA trigger, which comes a couple of dots
    /// after the mode change, has passed.
    pub(super) fn hblank_dma_triggered(&self) -> bool {
        self.mode() == HBLANK_MODE && self.hdma_start_delay == 0
    }

    /// After a write to WY, mode 3 sees the new value a few dots later (`delayed_wy`; the delays
    /// are fitted to Gambatte's window late_wy tests).
    fn schedule_delayed_wy_update(&mut self) {
        if self.countdowns.delayed_wy_update == 0 {
            let delay = match self.timing {
                TimingVariant::Dmg => DMG_DELAYED_WY_DOTS,
                TimingVariant::Cgb => CGB_DELAYED_WY_DOTS,
                TimingVariant::CgbDoubleSpeed => DOUBLE_SPEED_DELAYED_WY_DOTS,
            };
            self.countdowns.delayed_wy_update = delay + 1;
        }
    }

    /// Remembers for the rest of the frame that LY matched WY while the window was enabled.
    fn update_window_y_trigger(&mut self) {
        if window_enabled(self.lcdc) && self.ly == self.wy {
            self.window_y_triggered = true;
        }
    }

    /// Compares WY with LY at the dots of the line where the PPU does (`*_WINDOW_Y_CHECK_DOTS`).
    /// Line 0 starts the frame over, and only then resets the window's line counter, so a DMG
    /// window enabled during VBlank with a WX 166 start pending doesn't carry a counted line into
    /// the new frame (Gambatte's window on_screen wxA6_weoff_at_xposA6 test).
    fn check_window_y(&mut self) {
        let (line_0_dot, line_start_dot, line_end_dot) = self.window_y_check_dots();
        if self.line == 0 && self.line_dot == line_0_dot {
            self.window_y_triggered = false;
            self.pipeline.reset_window_line();
            self.update_window_y_trigger();
        } else if (1..VBLANK_START_LINE).contains(&self.line) && self.line_dot == line_start_dot {
            self.update_window_y_trigger();
        }
        if self.line < LAST_VISIBLE_LINE && self.line_dot == line_end_dot {
            self.update_window_y_trigger();
        }
    }

    /// The dots of the window's WY checks for this model and speed (see `*_WINDOW_Y_CHECK_DOTS`).
    fn window_y_check_dots(&self) -> (u16, u16, u16) {
        match self.timing {
            TimingVariant::Dmg => DMG_WINDOW_Y_CHECK_DOTS,
            TimingVariant::Cgb => CGB_WINDOW_Y_CHECK_DOTS,
            TimingVariant::CgbDoubleSpeed => DOUBLE_SPEED_WINDOW_Y_CHECK_DOTS,
        }
    }

    /// Whether the window can start on this line: LY matched WY at one of this frame's checks, or
    /// matches the delayed WY right now.
    pub(super) fn window_y_active(&self) -> bool {
        self.window_y_triggered || (window_enabled(self.lcdc) && self.delayed_wy == self.ly)
    }

    /// Moves to dot 0 of the next line. Visible lines start with OAM scan; line 144 starts vblank
    /// and publishes the finished frame.
    fn start_next_line(&mut self) {
        self.stat_at_line_start = self.stat;
        self.line = (self.line + 1) % FRAME_SCANLINE_COUNT;
        self.line_dot = 0;
        self.first_line_after_lcd_on = false;

        if self.line == 0 {
            self.window_y_triggered = false;
        } else {
            self.ly = self.line;
        }

        if self.line < VBLANK_START_LINE {
            self.line_state = LineState::OamScan;
            self.oam_scan_row = 0;
            self.clear_visible_objects();
        } else if self.line == VBLANK_START_LINE {
            self.line_state = LineState::VBlank;
            let blank = std::mem::take(&mut self.first_frame_after_lcd_on);
            self.publish_frame(blank, true);
        }
    }

    /// Hands the frame in `frame_buffer` to the frontend, or a white frame when `blank` is set.
    /// `drawn` says whether the PPU drew it, rather than counting a frame's time with the LCD off
    /// or stopped.
    ///
    /// On a Super Game Boy the Super Nintendo shows the frame instead, colored and with its
    /// border. It is sent the pixels the PPU drew whether or not the LCD would show them (the
    /// first frame after the LCD is switched on), and keeps showing the last one while none are
    /// drawn (SameBoy).
    fn publish_frame(&mut self, blank: bool, drawn: bool) {
        if let Some(sgb) = &mut self.sgb.0 {
            let frame = drawn.then_some(self.frame_buffer.as_flattened());
            sgb.finish_frame(frame, &mut self.completed_frame);
        } else if blank {
            self.completed_frame.as_chunks_mut().0.fill(WHITE);
        } else {
            self.completed_frame
                .copy_from_slice(self.frame_buffer.as_flattened());
        }
        self.frame_ready = true;
    }

    /// Advances mode 2 by one dot. Every two dots one OAM entry is checked for the line, and mode 3
    /// starts at `DRAWING_START_DOT`.
    fn tick_oam_scan(&mut self) {
        if self.first_line_after_lcd_on {
            if self.line_dot == DRAWING_START_DOT {
                self.enter_mode_3();
            }
            return;
        }

        if self.line_dot == MODE_2_STAT_DOT {
            self.visible_mode = OAM_MODE;
        }
        if self.line == 0 && self.line_dot == LINE_0_OAM_SOURCE_DOT {
            // Gambatte's m2enable ly0 tests: with both interrupts enabled, line 0's mode 2
            // interrupt is lost even though STAT already left mode 1, and a later STAT write
            // doesn't bring it back.
            // A CGB at normal speed only sees the vblank enable from the start of the line here,
            // but the mode 2 enable as it is now (Gambatte's late_enable_m1disable_ly0 tests).
            let vblank_enables = if self.timing == TimingVariant::Cgb {
                self.stat_at_line_start
            } else {
                self.stat
            };
            self.line_0_oam_interrupt_blocked =
                is_bit_set(vblank_enables, VBLANK_MODE_STAT_SOURCE_BIT)
                    && is_bit_set(self.stat, OAM_MODE_STAT_SOURCE_BIT);
        }

        if self.line_dot.is_multiple_of(2) && self.line_dot <= OAM_SCAN_END_DOT {
            if self.line_dot > 0 {
                self.scan_oam_entry(self.line_dot / 2 - 1);
            }
            self.oam_scan_row = if self.line_dot < OAM_SCAN_END_DOT {
                (self.line_dot * 2) as u8
            } else {
                NO_OAM_SCAN_ROW
            };
        }

        let mode_3_stat_dot = if self.in_double_speed() {
            DOUBLE_SPEED_MODE_3_STAT_DOT
        } else {
            MODE_3_STAT_DOT
        };
        if self.line_dot == mode_3_stat_dot {
            self.visible_mode = VRAM_MODE;
        }
        if self.in_double_speed() && self.line_dot == DOUBLE_SPEED_PALETTE_BLOCK_DOT {
            self.palette_ram_blocked = true;
        }
        if self.line_dot == DRAWING_START_DOT {
            self.enter_mode_3();
        }
    }

    /// Switches to mode 3. The renderer starts after `fetcher_start_delay` dots.
    fn enter_mode_3(&mut self) {
        if self.is_cgb() && self.first_line_after_lcd_on {
            // The first line after the LCD is switched on blocks palette RAM and, at normal speed,
            // VRAM a few dots into mode 3 (SameBoy; the dots are fitted to Gambatte's
            // enable_display ly0_late tests).
            self.countdowns.palette_block = FIRST_LINE_PALETTE_BLOCK_DOTS;
            if !self.in_double_speed() {
                self.countdowns.first_line_vram_grace = FIRST_LINE_VRAM_BLOCK_DOTS;
            }
        } else if self.is_cgb() {
            self.palette_ram_blocked = true;
        }
        self.line_state = LineState::Drawing;
        self.early_hblank_interrupt = false;
        self.countdowns.early_hblank = 0;
        self.visible_mode = VRAM_MODE;
        self.oam_scan_row = NO_OAM_SCAN_ROW;
        self.fetcher_start_delay = if self.first_line_after_lcd_on {
            LCD_ON_FETCHER_START_DELAY_DOTS
        } else {
            FETCHER_START_DELAY_DOTS
        };
    }

    /// Advances mode 3 by one dot and switches to hblank once the renderer finishes the line.
    fn tick_mode_3(&mut self) {
        if self.fetcher_start_delay > 0 {
            self.fetcher_start_delay -= 1;
            if self.fetcher_start_delay > 0 {
                return;
            }
            self.start_drawing();
        }

        if !self.tick_drawing() {
            return;
        }

        self.end_drawing();
        self.line_state = LineState::HBlank;
        self.visible_mode = if self.in_double_speed() {
            VRAM_MODE
        } else {
            HBLANK_MODE
        };
        self.mode_0_interrupt_delay = MODE_0_INTERRUPT_DELAY_DOTS;
        self.hdma_start_delay = HDMA_START_DELAY_DOTS;
        self.countdowns.palette_unblock = match self.timing {
            TimingVariant::Dmg => 0,
            TimingVariant::Cgb => PALETTE_UNBLOCK_DOTS,
            TimingVariant::CgbDoubleSpeed => DOUBLE_SPEED_PALETTE_UNBLOCK_DOTS,
        };
    }

    /// Shows mode 1 and requests the vblank interrupt early on line 144, and resets LY early on
    /// line 153.
    fn tick_vblank(&mut self) {
        if self.line == VBLANK_START_LINE && self.line_dot == VBLANK_START_DOT {
            self.visible_mode = VBLANK_MODE;
            self.vblank_interrupt = true;
        }

        if self.line == LAST_LINE && self.line_dot == self.last_line_ly_reset_dot() {
            self.ly = 0;
        }
    }

    /// Advances the PPU by one dot while the LCD is on.
    fn tick_dot(&mut self, hdma: &mut HdmaState) {
        if count_down(&mut self.countdowns.early_hblank) {
            self.early_hblank_interrupt = true;
        }
        self.previous_lyc_flag = self.lyc_flag;
        if count_down(&mut self.countdowns.lyc_interrupt) {
            self.stat_interrupt = true;
        }
        if count_down(&mut self.countdowns.oam_dma_access) {
            self.oam_dma_access = self.next_oam_dma_access;
            self.apply_dma_oam_write();
        }
        count_down(&mut self.countdowns.first_line_vram_grace);
        if count_down(&mut self.countdowns.palette_block) {
            self.palette_ram_blocked = true;
        }
        if count_down(&mut self.countdowns.palette_unblock) {
            self.palette_ram_blocked = false;
        }
        if count_down(&mut self.countdowns.delayed_wy_update) {
            self.delayed_wy = self.wy;
        }
        self.line_dot += 1;
        if self.line_dot == DOTS_PER_LINE {
            self.start_next_line();
        }
        if self.in_double_speed() && self.line_dot == DOUBLE_SPEED_STAT_SAMPLE_DOT {
            self.stat_at_line_start = self.stat;
        }
        self.check_window_y();

        match self.line_state {
            LineState::OamScan => self.tick_oam_scan(),
            LineState::Drawing => self.tick_mode_3(),
            LineState::HBlank => {
                self.visible_mode = HBLANK_MODE;
                if self.mode_0_interrupt_delay > 0 {
                    self.mode_0_interrupt_delay -= 1;
                }
                if self.hdma_start_delay > 0 {
                    self.hdma_start_delay -= 1;
                    if self.hdma_start_delay == 0 {
                        hdma.set_hblank_started(true);
                    }
                }
            }
            LineState::VBlank => self.tick_vblank(),
        }

        self.update_lyc_flag();
        self.update_stat_line();
        self.finish_late_register_writes();
        self.previous_lcdc = self.lcdc;
    }

    /// Counts up to `dots` dots in which the PPU doesn't draw (the LCD is off, or STOP stopped a
    /// monochrome model's PPU) and returns how many it counted: it stops early when a frame is
    /// due, which it then delivers, blank when `blank` is set or else whatever the frame buffer
    /// holds.
    fn count_idle_dots(&mut self, dots: u32, blank: bool) -> u32 {
        let counted = dots.min(self.idle_dots_until_frame());
        self.idle_dots += counted;
        if self.idle_dots == DOTS_PER_FRAME {
            self.idle_dots = 0;
            self.publish_frame(blank, false);
        }
        counted
    }

    /// How many more dots the PPU counts while it doesn't draw before it delivers a frame.
    fn idle_dots_until_frame(&self) -> u32 {
        if self.idle_dots < DOTS_PER_FRAME {
            DOTS_PER_FRAME - self.idle_dots
        } else {
            u32::MAX
        }
    }

    /// Advances the PPU by the `dots` dots of (part of) an M-cycle. `hdma` is told when hblank
    /// starts.
    pub(crate) fn step_dots(&mut self, dots: u8, hdma: &mut HdmaState) {
        self.run_dots(u32::from(dots), hdma, true);
    }

    /// Advances the PPU by `dots` dots, with `hdma` told when hblank starts, and returns how many
    /// of the dots after them the PPU can be left to run later (`quiet_dots`).
    /// `stat_interrupt_enabled` says whether IE enables the STAT interrupt.
    pub(crate) fn run_dots(
        &mut self,
        dots: u32,
        hdma: &mut HdmaState,
        stat_interrupt_enabled: bool,
    ) -> u32 {
        if std::mem::take(&mut self.lcd_off_hblank_dma) {
            hdma.set_hblank_started(true);
        }
        let mut remaining = dots;
        // Whether the LY=LYC flag and the STAT line are known to be up to date (`stat_settled`):
        // every dot the LCD draws leaves them so, so only the first needs checking.
        let mut settled = false;
        while remaining > 0 {
            if !self.lcd_enabled() {
                remaining -= self.count_idle_dots(remaining, false);
            } else if self.stopped && !self.is_cgb() {
                // A monochrome model's PPU stands still in STOP mode and shows white (SameBoy;
                // daid's stop_instr test).
                remaining -= self.count_idle_dots(remaining, true);
            } else if self.has_steady_shape() && (settled || self.stat_settled()) {
                remaining -= self.run_steady_dots(remaining);
                debug_assert!(self.stat_settled(), "a steady stretch leaves STAT settled");
                settled = true;
            } else if self.has_quiet_line_start() {
                remaining -= self.run_line_start(remaining);
                debug_assert!(self.stat_settled(), "a line's start leaves STAT settled");
                settled = true;
            } else if self.has_quiet_hblank_start() {
                remaining -= self.run_hblank_start(remaining, hdma);
                debug_assert!(self.stat_settled(), "HBlank's start leaves STAT settled");
                settled = true;
            } else {
                self.tick_dot(hdma);
                remaining -= 1;
                settled = true;
            }
        }
        self.quiet_dots(settled, hdma.in_progress(), stat_interrupt_enabled)
    }

    pub(crate) fn lcdc(&self) -> u8 {
        self.lcdc
    }

    /// Whether a DMG LCDC write clearing the object enable bit now would land during an object
    /// fetch, or at the start of the line when `includes_line_start` is set. Used by
    /// `write_timing.rs`.
    pub(crate) fn object_fetch_conflicts_with_lcdc_write(&self, includes_line_start: bool) -> bool {
        (includes_line_start && self.pipeline.position_in_line == 0)
            || self.pipeline.during_object_fetch
    }

    pub(crate) fn window_is_being_fetched(&self) -> bool {
        self.pipeline.window_is_being_fetched
    }

    /// Suppresses the DMG glitch that inserts an extra background pixel when the window is switched
    /// off just as it would start.
    pub(crate) fn disable_window_pixel_insertion_glitch(&mut self) {
        self.pipeline.disable_window_pixel_insertion_glitch = true;
    }

    /// During a split LCDC write, the background enable bit the renderer uses if it draws the
    /// line's first pixel. `None` uses LCDC.
    pub(crate) fn set_written_bg_enable(&mut self, value: Option<bool>) {
        self.pipeline.written_bg_enable = value;
    }

    /// Marks the dot right after a DMG WX write, during which the window can't start one pixel
    /// early.
    pub(crate) fn set_wx_just_changed(&mut self, value: bool) {
        self.pipeline.wx_just_changed = value;
    }

    /// Marks the dot during which an LCDC write changing the tile data area (bit 4) makes the
    /// fetcher read glitched tile data.
    pub(crate) fn set_tile_sel_glitch(&mut self, value: bool) {
        self.pipeline.tile_sel_glitch = value;
    }

    /// Marks the dot in which a CGB LCDC write changing the object size lands, during which OAM
    /// scan sees either size.
    pub(crate) fn set_tall_objects_glitch(&mut self, value: bool) {
        self.pipeline.tall_objects_glitch = value;
    }

    /// Resets the line timeline and blanks the screen when LCDC bit 7 is cleared. An HBlank DMA
    /// trigger or mode 0 interrupt source still on its way (`hdma_start_delay`,
    /// `mode_0_interrupt_delay`) is dropped, so HDMA hears of no HBlank and sees a plain mode 0
    /// while the LCD is off and on the first line after it is turned on again, where a CPU that
    /// halts counts as halted inside HBlank (SameBoy: `GB_lcd_off` restarts `GB_display_run`'s
    /// line, dropping its HDMA trigger, and `halt` lets HDMA start on wake only after halting
    /// outside mode 0).
    fn turn_lcd_off(&mut self) {
        self.line = 0;
        self.ly = 0;
        self.line_dot = 0;
        self.line_state = LineState::HBlank;
        self.visible_mode = HBLANK_MODE;
        self.hdma_start_delay = 0;
        self.mode_0_interrupt_delay = 0;
        self.first_line_after_lcd_on = false;
        self.palette_ram_blocked = false;
        self.countdowns.palette_block = 0;
        self.countdowns.palette_unblock = 0;
        self.countdowns.first_line_vram_grace = 0;
        self.compared_lyc = self.lyc;
        self.countdowns.lyc_write = 0;
        if std::mem::take(&mut self.countdowns.lyc_interrupt) > 0 {
            self.stat_interrupt = true;
        }
        self.late_cleared_stat_sources = 0;
        self.countdowns.stat_clear = 0;
        self.window_y_triggered = false;
        self.idle_dots = 0;
        self.oam_scan_row = NO_OAM_SCAN_ROW;
        self.frame_buffer.fill(WHITE);
    }

    /// Starts the special first line when LCDC bit 7 is set: it skips OAM scan and starts a few
    /// dots in.
    fn turn_lcd_on(&mut self) {
        self.first_frame_after_lcd_on = true;
        self.line = 0;
        self.ly = 0;
        self.line_dot = if self.is_cgb() {
            CGB_LCD_ON_START_DOT
        } else {
            DMG_LCD_ON_START_DOT
        };
        self.line_state = LineState::OamScan;
        self.visible_mode = HBLANK_MODE;
        self.first_line_after_lcd_on = true;
        self.window_y_triggered = false;
        self.pipeline.reset_window_line();
        self.clear_visible_objects();
        self.update_window_y_trigger();
    }

    /// Writes LCDC, switching the LCD on or off when bit 7 changes. On DMG, clearing the object
    /// enable bit during an object fetch aborts that fetch.
    pub(super) fn set_lcdc(&mut self, value: u8) {
        let was_enabled = self.lcd_enabled();
        let mode_before = self.mode();
        if !self.is_cgb()
            && objects_enabled(self.lcdc)
            && !objects_enabled(value)
            && self.pipeline.during_object_fetch
        {
            self.pipeline.object_fetch_aborted = true;
        }
        let window_disabled = window_enabled(self.lcdc) && !window_enabled(value);
        let window_enabling = !window_enabled(self.lcdc) && window_enabled(value);
        self.lcdc = value;
        if window_disabled {
            self.cancel_window_start();
        }
        if window_enabling {
            self.window_enabled_with_continue_pending();
        }

        match (was_enabled, self.lcd_enabled()) {
            (true, false) => {
                self.lcd_off_hblank_dma = mode_before != HBLANK_MODE;
                self.turn_lcd_off();
            }
            (false, true) => self.turn_lcd_on(),
            _ => {}
        }

        self.update_lyc_flag();
        self.update_stat_line();
    }

    /// Places the line timeline at `dot` of `line`, to match where the boot ROM leaves the PPU.
    /// Only for positions where no line is being drawn and no OAM scan has started: VBlank, the
    /// first dot of OAM scan (dot 0), or HBlank once mode 3 would be over (dot 252 or later). It
    /// doesn't set the renderer up for a line in progress, so tests that place it elsewhere don't
    /// draw.
    pub(super) fn set_frame_position(&mut self, line: u8, dot: u16) {
        self.line = line;
        self.line_dot = dot;
        self.first_line_after_lcd_on = false;
        self.ly = if line == LAST_LINE && dot >= self.last_line_ly_reset_dot() {
            0
        } else {
            line
        };

        let (state, mode) = if line >= VBLANK_START_LINE {
            (
                LineState::VBlank,
                if line == VBLANK_START_LINE && dot < VBLANK_START_DOT {
                    HBLANK_MODE
                } else {
                    VBLANK_MODE
                },
            )
        } else if dot < DRAWING_START_DOT {
            (
                LineState::OamScan,
                if dot < MODE_2_STAT_DOT {
                    HBLANK_MODE
                } else if dot < MODE_3_STAT_DOT {
                    OAM_MODE
                } else {
                    VRAM_MODE
                },
            )
        } else {
            (LineState::HBlank, HBLANK_MODE)
        };

        self.line_state = state;
        self.visible_mode = mode;
        self.update_lyc_flag();
        self.stat_line_high = self.stat_line_level(self.stat);
    }

    /// Whether the PPU is in hblank or the LCD is off. An hblank HDMA started now copies its first
    /// block right away; one started in the last dots of the line waits for the next hblank.
    pub(crate) fn in_hblank_or_off(&self) -> bool {
        let hblank_dma_end_dot = if self.in_double_speed() {
            DOUBLE_SPEED_HBLANK_DMA_START_END_DOT
        } else {
            HBLANK_DMA_START_END_DOT
        };
        !self.lcd_enabled()
            || (self.line_state == LineState::HBlank && self.line_dot < hblank_dma_end_dot)
    }

    /// Whether the CGB runs a DMG game, as selected by the boot ROM through KEY0.
    pub(super) fn in_dmg_compatibility_mode(&self) -> bool {
        self.key0 == 0x04
    }

    #[cfg(test)]
    pub(super) fn set_mode(&mut self, value: u8) {
        self.visible_mode = value;
        self.line_state = match value {
            HBLANK_MODE => LineState::HBlank,
            VBLANK_MODE => LineState::VBlank,
            OAM_MODE => LineState::OamScan,
            _ => LineState::Drawing,
        };
    }
}

impl MemoryMapped for Gpu {
    fn read_byte(&self, address: u16) -> u8 {
        match address {
            0x8000..=0x9FFF => self.video_ram_byte(address & 0x1FFF),
            0xFE00..=0xFE9F => self.object_attribute_memory_byte(address & 0xFF),
            0xFF40 => self.lcdc,
            0xFF41 => self.stat_value(),
            0xFF42 => self.scy,
            0xFF43 => self.scx,
            0xFF44 => self.ly_value(),
            0xFF45 => self.lyc,
            0xFF47 => self.palettes.bgp(),
            0xFF48 => self.palettes.obp0(),
            0xFF49 => self.palettes.obp1(),
            0xFF4A => self.wy,
            0xFF4B => self.wx,
            0xFF4C => self.key0,
            0xFF4F => self.cgb_vbk(),
            0xFF68 => self.palettes.cgb_bcps(),
            0xFF69 if self.palette_ram_blocked => 0xFF,
            0xFF69 => self.palettes.cgb_bcpd(),
            0xFF6A => self.palettes.cgb_ocps(),
            0xFF6B if self.palette_ram_blocked => 0xFF,
            0xFF6B => self.palettes.cgb_ocpd(),
            0xFF6C => self.cgb_opri(),
            _ => {
                unreachable!("the address bus only routes GPU addresses here, got 0x{address:04X}")
            }
        }
    }

    fn write_byte(&mut self, address: u16, value: u8) {
        match address {
            0x8000..=0x9FFF => self.set_video_ram_byte(address & 0x1FFF, value),
            0xFE00..=0xFE9F => self.set_object_attribute_memory_byte(address & 0xFF, value),
            0xFF40 => self.set_lcdc(value),
            0xFF41 => self.write_stat(value),
            0xFF42 => self.scy = value,
            0xFF43 => self.scx = value,
            0xFF44 => {}
            0xFF45 => self.write_lyc(value),
            0xFF47 => self.palettes.set_bgp(value),
            0xFF48 => self.palettes.set_obp0(value),
            0xFF49 => self.palettes.set_obp1(value),
            0xFF4A => {
                self.wy = value;
                self.schedule_delayed_wy_update();
            }
            0xFF4B => self.wx = value,
            0xFF4C => self.key0 = value,
            0xFF4F => self.set_cgb_vbk(value),
            0xFF68 => self.palettes.set_cgb_bcps(value),
            0xFF69 if self.palette_ram_blocked => self.palettes.skip_cgb_bcpd_write(),
            0xFF69 => self.palettes.set_cgb_bcpd(value),
            0xFF6A => self.palettes.set_cgb_ocps(value),
            0xFF6B if self.palette_ram_blocked => self.palettes.skip_cgb_ocpd_write(),
            0xFF6B => self.palettes.set_cgb_ocpd(value),
            0xFF6C => self.set_cgb_opri(value),
            _ => {
                unreachable!("the address bus only routes GPU addresses here, got 0x{address:04X}")
            }
        }
    }
}

#[cfg(test)]
mod tests;

mod access;
mod constants;
mod lcdc;
mod oam_bug;
mod palettes;
mod pipeline;
mod save_state;
mod stat;
mod steady;
