//! Randomized tests of the PPU on random scenes: random VRAM, OAM, palettes and registers on DMG,
//! CGB and CGB in double speed, with register writes at random dots.
//!
//! They check that the bulk runs (`Gpu::run_dots` with its steady stretches and plain pixels) give
//! exactly what running dot by dot does, down to every byte of a save state after each stretch,
//! often ended near a line's timing events, that the quiet dots a bulk run promises (the ones the
//! address bus may leave the PPU behind for) hold no interrupt, frame or HBlank signal the CPU
//! could see, and that a save state taken at any dot carries on
//! exactly like the PPU it came from and can't make the PPU panic when corrupted. Now and then the
//! comparison also switches the LCD off for a while, enters STOP mode or runs an OAM DMA transfer
//! (see `Event`), so that the bulk runs' paths for those are compared too.
//!
//! `NUBO_RANDOM_SCENES` sets how many scenes the comparison of bulk runs with running dot by dot
//! tries (18 by default, a few seconds in a debug build), and `NUBO_RANDOM_SEED` the seed it starts
//! from, which a failure reports. The nightly run tries thousands with a new seed each night.

use crate::address_bus::MemoryMapped;
use crate::address_bus::hdma::HdmaState;
use crate::clock::dots_per_m_cycle;
use crate::gpu::{DOTS_PER_FRAME, DOTS_PER_LINE, Gpu, OamDmaAccess};
use crate::model::Model;
use crate::serializable::Serializable;
use std::collections::VecDeque;

/// A small pseudo-random number generator (xorshift64), so that the tests are repeatable.
struct TestRandom(u64);

impl TestRandom {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn byte(&mut self) -> u8 {
        self.next().to_le_bytes()[3]
    }

    /// A number from 0 to `limit - 1`.
    fn below(&mut self, limit: u32) -> u32 {
        u32::try_from(self.next() % u64::from(limit)).expect("below a u32")
    }

    /// A byte from 0 to `limit - 1`.
    fn byte_below(&mut self, limit: u8) -> u8 {
        u8::try_from(self.below(u32::from(limit))).expect("below a u8")
    }

    fn chance(&mut self, percent: u32) -> bool {
        self.below(100) < percent
    }
}

/// How many scenes `bulk_runs_match_running_dot_by_dot` tries, and the seed it starts from: from
/// `NUBO_RANDOM_SCENES` and `NUBO_RANDOM_SEED` when they are set (see the module docs).
fn scene_count_and_seed() -> (usize, u64) {
    let read = |name: &str| {
        std::env::var(name)
            .ok()
            .map(|value| value.parse::<u64>().expect("a whole number"))
    };
    let count = read("NUBO_RANDOM_SCENES").map_or(18, |count| {
        usize::try_from(count).expect("a scene count that fits")
    });
    // xorshift64 never leaves 0, so a seed of 0 becomes 1.
    let seed = read("NUBO_RANDOM_SEED")
        .unwrap_or(0x2545_F491_4F6C_DD1D)
        .max(1);
    (count, seed)
}

/// The three timing variants (DMG, CGB, CGB in double speed), as a model and a speed, and a CGB
/// from before CGB-D, whose fetcher reads the tile map's y again for each read
/// (`caches_fetcher_y`).
const TIMINGS: [(Model, bool); 4] = [
    (Model::Dmg, false),
    (Model::CgbE, false),
    (Model::CgbE, true),
    (Model::CgbC, false),
];

/// A PPU with random VRAM, OAM, palettes and registers that has just switched the LCD on.
/// Objects are placed where they can show, and the window often is.
fn random_scene(random: &mut TestRandom, model: Model, double_speed: bool) -> Gpu {
    let mut gpu = Gpu::new();
    gpu.set_model(model);
    gpu.set_cgb_double_speed(double_speed);
    for byte in &mut gpu.video_ram {
        *byte = random.byte();
    }
    for object in gpu.object_attribute_memory.as_chunks_mut::<4>().0 {
        object[0] = random.byte_below(170);
        object[1] = random.byte_below(176);
        object[2] = random.byte();
        object[3] = random.byte();
    }
    for address in 0xFF47..=0xFF49 {
        gpu.write_byte(address, random.byte());
    }
    if model.is_cgb() {
        for (index_register, data_register) in [(0xFF68, 0xFF69), (0xFF6A, 0xFF6B)] {
            gpu.write_byte(index_register, 0x80);
            for _ in 0..64 {
                gpu.write_byte(data_register, random.byte());
            }
        }
        gpu.write_byte(0xFF4C, if random.chance(25) { 0x04 } else { 0 });
        gpu.write_byte(0xFF6C, random.byte());
    }
    for _ in 0..8 {
        write_random_register(&mut gpu, random);
    }
    gpu.write_byte(0xFF40, 0x80 | random.byte());
    gpu
}

/// Writes a random value to a random register that changes what mode 3 draws or when the STAT
/// interrupt is requested, leaving the LCD on or off as it is. LYC and WY often get the current or
/// the next line, where their late effects show.
fn write_random_register(gpu: &mut Gpu, random: &mut TestRandom) {
    let near_line = gpu.ly.wrapping_add(random.byte_below(2));
    let (address, value) = match random.below(9) {
        0 => (0xFF40, (gpu.lcdc & 0x80) | (random.byte() & 0x7F)),
        1 => (0xFF41, random.byte()),
        2 => (0xFF42, random.byte()),
        3 => (0xFF43, random.byte()),
        4 if random.chance(50) => (0xFF45, near_line),
        4 => (0xFF45, random.byte_below(155)),
        5 if random.chance(50) => (0xFF4A, near_line),
        5 => (0xFF4A, random.byte_below(150)),
        6 => (0xFF4B, random.byte_below(170)),
        7 => (0xFF46 + u16::from(random.byte_below(3)) + 1, random.byte()),
        _ => (
            0x8000 + u16::try_from(random.below(0x2000)).expect("in VRAM"),
            random.byte(),
        ),
    };
    gpu.write_byte(address, value);
}

/// Writes the same random value to the same random register of both PPUs.
fn write_random_register_to_both(by_dot: &mut Gpu, in_bulk: &mut Gpu, random: &mut TestRandom) {
    let seed = random.next() | 1;
    write_random_register(by_dot, &mut TestRandom(seed));
    write_random_register(in_bulk, &mut TestRandom(seed));
}

/// Runs `dots` dots one at a time, as `Gpu::run_dots` does without its shortcuts: `Gpu::tick_dot`
/// while the PPU draws, and one idle dot at a time while it doesn't (the LCD is off, or STOP mode
/// stopped a monochrome model's PPU).
fn tick_dots(gpu: &mut Gpu, hdma: &mut HdmaState, dots: u32) {
    for _ in 0..dots {
        if gpu.lcd_enabled() && !(gpu.stopped && !gpu.is_cgb()) {
            if std::mem::take(&mut gpu.lcd_off_hblank_dma) {
                hdma.set_hblank_started(true);
            }
            gpu.tick_dot(hdma);
        } else {
            gpu.run_dots(1, hdma, true);
        }
    }
}

/// Something the rest of the hardware does to the PPU over many dots, which both PPUs of
/// `bulk_runs_match_running_dot_by_dot` go through alike.
enum Event {
    /// Nothing: the CPU only writes registers between stretches.
    None,
    /// The LCD is off, and an LCDC write switches it on again after `dots_left` dots.
    LcdOff { dots_left: u32 },
    /// The CPU is in STOP mode, which a button press ends after `dots_left` dots. A monochrome
    /// model's PPU stands still meanwhile, and the CGB's is cut off from what the CPU could reach.
    Stopped { dots_left: u32 },
    /// An OAM DMA transfer, with the M-cycles it still has to run.
    OamDma(VecDeque<OamDmaCycle>),
}

/// How many bytes an OAM DMA transfer copies: the whole of OAM.
const OAM_DMA_BYTES: u8 = 160;

/// One M-cycle of an OAM DMA transfer as the PPU sees it: how the transfer holds OAM from half an
/// M-cycle in, and the byte it copies to OAM, if any.
struct OamDmaCycle {
    access: OamDmaAccess,
    copied: Option<(u16, u8)>,
}

/// The M-cycles of an OAM DMA transfer, as the address bus runs them (`address_bus/dma.rs`): one
/// for each byte it copies, then one that frees OAM. Sometimes HALT holds the transfer for up to a
/// few lines. The bytes place objects where they can show, as `random_scene` does.
fn random_oam_dma(random: &mut TestRandom) -> VecDeque<OamDmaCycle> {
    let halt_at = random.chance(30).then(|| random.byte_below(OAM_DMA_BYTES));
    let mut m_cycles = VecDeque::new();
    for index in 0..OAM_DMA_BYTES {
        if halt_at == Some(index) {
            for _ in 0..1 + random.below(300) {
                m_cycles.push_back(OamDmaCycle {
                    access: OamDmaAccess::PausedByHalt,
                    copied: None,
                });
            }
        }
        let value = match index % 4 {
            0 => random.byte_below(170),
            1 => random.byte_below(176),
            _ => random.byte(),
        };
        m_cycles.push_back(OamDmaCycle {
            access: OamDmaAccess::Running { destination: index },
            copied: Some((u16::from(index), value)),
        });
    }
    m_cycles.push_back(OamDmaCycle {
        access: OamDmaAccess::Free,
        copied: None,
    });
    m_cycles
}

/// How many dots the LCD stays off or STOP mode lasts: mostly a few hundred, sometimes long enough
/// for the PPU to deliver the blank frame it owes then.
fn event_dots(random: &mut TestRandom) -> u32 {
    if random.chance(10) {
        DOTS_PER_FRAME / 2 + random.below(DOTS_PER_FRAME)
    } else {
        100 + random.below(1500)
    }
}

/// After a stretch without an event, starts one in both PPUs alike now and then (switching the LCD
/// off, entering STOP mode or starting OAM DMA), or else often writes a random register.
fn start_random_event(by_dot: &mut Gpu, in_bulk: &mut Gpu, random: &mut TestRandom) -> Event {
    match random.below(100) {
        0..2 => {
            let lcdc = random.byte() & 0x7F;
            by_dot.write_byte(0xFF40, lcdc);
            in_bulk.write_byte(0xFF40, lcdc);
            Event::LcdOff {
                dots_left: event_dots(random),
            }
        }
        2..4 => {
            by_dot.enter_stop_mode();
            in_bulk.enter_stop_mode();
            Event::Stopped {
                dots_left: event_dots(random),
            }
        }
        4..7 => Event::OamDma(random_oam_dma(random)),
        7..47 => {
            write_random_register_to_both(by_dot, in_bulk, random);
            Event::None
        }
        _ => Event::None,
    }
}

/// Moves `event` on after a stretch of `dots` dots, in both PPUs alike, ending it when its time is
/// up. The CPU writes registers now and then meanwhile, but not in STOP mode.
fn continue_event(
    event: Event,
    dots: u32,
    by_dot: &mut Gpu,
    in_bulk: &mut Gpu,
    random: &mut TestRandom,
) -> Event {
    match event {
        Event::None => start_random_event(by_dot, in_bulk, random),
        Event::LcdOff { dots_left } if dots_left == dots => {
            let lcdc = 0x80 | random.byte();
            by_dot.write_byte(0xFF40, lcdc);
            in_bulk.write_byte(0xFF40, lcdc);
            Event::None
        }
        Event::LcdOff { dots_left } => {
            if random.chance(40) {
                write_random_register_to_both(by_dot, in_bulk, random);
            }
            Event::LcdOff {
                dots_left: dots_left - dots,
            }
        }
        Event::Stopped { dots_left } if dots_left == dots => {
            by_dot.leave_stop_mode();
            in_bulk.leave_stop_mode();
            Event::None
        }
        Event::Stopped { dots_left } => Event::Stopped {
            dots_left: dots_left - dots,
        },
        Event::OamDma(m_cycles) if m_cycles.is_empty() => Event::None,
        Event::OamDma(m_cycles) => {
            if random.chance(3) {
                write_random_register_to_both(by_dot, in_bulk, random);
            }
            Event::OamDma(m_cycles)
        }
    }
}

/// The dots after a bulk run that `Gpu::run_dots` promised the rest of the hardware can't see
/// (`quiet_dots`), and whether they hold no STAT interrupt request either, as the address bus
/// works it out for a read of IF (`AddressBus::ppu_read_can_wait`).
#[derive(Clone, Copy)]
struct QuietDots {
    dots: u32,
    hold_no_stat_interrupt: bool,
}

/// The first line of the two PPUs' debug output that differs, to show what a failed comparison
/// found.
fn first_difference(expected: &Gpu, actual: &Gpu) -> Option<(String, String)> {
    let (expected, actual) = (format!("{expected:#?}"), format!("{actual:#?}"));
    expected
        .lines()
        .zip(actual.lines())
        .find(|(expected, actual)| expected != actual)
        .map(|(expected, actual)| (expected.to_owned(), actual.to_owned()))
}

fn serialize(gpu: &Gpu) -> Vec<u8> {
    let mut bytes = Vec::new();
    gpu.serialize(&mut bytes)
        .expect("writing to a Vec never fails");
    bytes
}

/// Loads `state` into a new PPU of `model`.
fn load(state: &[u8], model: Model) -> std::io::Result<Gpu> {
    let mut gpu = Gpu::new();
    gpu.set_model(model);
    gpu.deserialize(&mut &state[..])?;
    Ok(gpu)
}

#[test]
fn bulk_runs_match_running_dot_by_dot() {
    let (count, seed) = scene_count_and_seed();
    let mut random = TestRandom(seed);
    for case in 0..count {
        let (model, double_speed) = TIMINGS[case % TIMINGS.len()];
        let scene = random_scene(&mut random, model, double_speed);
        let (mut by_dot, mut in_bulk) = (scene.clone(), scene);
        let (mut by_dot_hdma, mut in_bulk_hdma) = (HdmaState::new(), HdmaState::new());
        if model.is_cgb() && random.chance(50) {
            // An HBlank DMA of 128 blocks, so that the PPU tells it when HBlank starts.
            by_dot_hdma.write_byte(0xFF55, 0xFF);
            in_bulk_hdma.write_byte(0xFF55, 0xFF);
        }
        let m_cycle_dots = dots_per_m_cycle(double_speed);
        let mut event = Event::None;
        let mut quiet: Option<QuietDots> = None;
        for frame in 0..2 {
            let mut left = DOTS_PER_FRAME;
            while left > 0 {
                // OAM DMA changes what the PPU sees before its M-cycle's dots, and the address bus
                // runs the PPU as it goes then.
                let promised = if matches!(event, Event::OamDma(_)) {
                    None
                } else {
                    quiet.take()
                };
                let stretch = match &mut event {
                    // OAM DMA runs one M-cycle at a time, with the PPU seeing the M-cycle's DMA
                    // state from half an M-cycle in, as in `AddressBus::sync_all_but_timers`.
                    Event::OamDma(m_cycles) => {
                        let m_cycle = m_cycles.pop_front().expect("an M-cycle left");
                        for gpu in [&mut by_dot, &mut in_bulk] {
                            if let Some((index, value)) = m_cycle.copied {
                                gpu.write_oam_from_dma(index, value);
                            }
                            gpu.set_oam_dma_access(m_cycle.access, m_cycle_dots / 2);
                        }
                        u32::from(m_cycle_dots)
                    }
                    // Some stretches end where most timing events are: in the first dots of the
                    // next line, or about where its mode 3 ends.
                    _ if random.chance(30) => {
                        let dot = if random.chance(50) {
                            random.below(8)
                        } else {
                            240 + random.below(100)
                        };
                        u32::from(DOTS_PER_LINE - by_dot.line_dot) + dot
                    }
                    _ => 1 + random.below(4000),
                };
                let stretch = match event {
                    Event::OamDma(_) => stretch,
                    Event::LcdOff { dots_left } | Event::Stopped { dots_left } => {
                        stretch.min(dots_left).min(left)
                    }
                    Event::None => stretch.min(left),
                };
                // The quiet dots the last bulk run promised, while nothing touched the PPU since,
                // must hold nothing the CPU could see without the PPU catching up.
                let checked = promised.map_or(0, |promised| promised.dots.min(stretch));
                tick_dots(&mut by_dot, &mut by_dot_hdma, checked);
                if let Some(promised) = promised {
                    assert!(
                        !by_dot.vblank_interrupt
                            && !by_dot.frame_ready
                            && !by_dot_hdma.hblank_started()
                            && !(promised.hold_no_stat_interrupt && by_dot.stat_interrupt),
                        "seed {seed}, case {case}, frame {frame}, line {} dot {}: something \
                         happened in the {} quiet dots",
                        by_dot.line,
                        by_dot.line_dot,
                        promised.dots
                    );
                }
                tick_dots(&mut by_dot, &mut by_dot_hdma, stretch - checked);
                let mut bulk_left = stretch;
                while bulk_left > 0 {
                    let chunk = (1 + random.below(700)).min(bulk_left);
                    let stat_interrupt_enabled = random.chance(50);
                    let dots = in_bulk.run_dots(chunk, &mut in_bulk_hdma, stat_interrupt_enabled);
                    quiet = Some(QuietDots {
                        dots,
                        hold_no_stat_interrupt: stat_interrupt_enabled
                            || in_bulk.stat_line_stays_low(),
                    });
                    bulk_left -= chunk;
                }
                left = left.saturating_sub(stretch);

                // What a save state holds is compared after every stretch, as a save state
                // taken there would show what a shortcut left out even when a later dot puts it
                // right; in OAM DMA, whose M-cycles are short stretches, only now and then and
                // once it is over. Interrupt requests and delivered frames are compared and
                // cleared, so that one on one side only can't be hidden by a later one on both.
                let in_oam_dma = matches!(&event, Event::OamDma(m_cycles) if !m_cycles.is_empty());
                assert!(
                    (in_oam_dma && !random.chance(5)) || serialize(&by_dot) == serialize(&in_bulk),
                    "seed {seed}, case {case}, frame {frame}, line {} dot {}: {:?}",
                    by_dot.line,
                    by_dot.line_dot,
                    first_difference(&by_dot, &in_bulk)
                );
                let requests = |gpu: &Gpu| {
                    (
                        gpu.line,
                        gpu.line_dot,
                        gpu.stat_interrupt,
                        gpu.vblank_interrupt,
                        gpu.frame_ready,
                    )
                };
                assert_eq!(
                    requests(&by_dot),
                    requests(&in_bulk),
                    "seed {seed}, case {case}, frame {frame}"
                );
                for gpu in [&mut by_dot, &mut in_bulk] {
                    gpu.stat_interrupt = false;
                    gpu.vblank_interrupt = false;
                    gpu.frame_ready = false;
                }
                // Likewise HBlank's start for HBlank DMA, which the address bus would take.
                assert_eq!(
                    format!("{by_dot_hdma:?}"),
                    format!("{in_bulk_hdma:?}"),
                    "seed {seed}, case {case}, frame {frame}"
                );
                for hdma in [&mut by_dot_hdma, &mut in_bulk_hdma] {
                    hdma.set_hblank_started(false);
                }

                let after_stretch = serialize(&by_dot);
                event = continue_event(event, stretch, &mut by_dot, &mut in_bulk, &mut random);
                if serialize(&by_dot) != after_stretch {
                    quiet = None;
                }
            }
            assert_eq!(
                format!("{by_dot:?}"),
                format!("{in_bulk:?}"),
                "seed {seed}, case {case}, frame {frame}"
            );
            assert_eq!(
                format!("{by_dot_hdma:?}"),
                format!("{in_bulk_hdma:?}"),
                "seed {seed}, case {case}"
            );
        }
    }
}

#[test]
fn a_loaded_save_state_carries_on_like_the_original() {
    let mut random = TestRandom(0x9E37_79B9_7F4A_7C15);
    for case in 0..12 {
        let (model, double_speed) = TIMINGS[case % TIMINGS.len()];
        let mut original = random_scene(&mut random, model, double_speed);
        let mut hdma = HdmaState::new();
        tick_dots(&mut original, &mut hdma, random.below(2 * DOTS_PER_FRAME));
        let state = serialize(&original);

        let mut loaded = load(&state, model).expect("a state the PPU wrote loads");
        assert_eq!(serialize(&loaded), state, "case {case}");

        let mut loaded_hdma = hdma.clone();
        tick_dots(&mut original, &mut hdma, DOTS_PER_FRAME);
        tick_dots(&mut loaded, &mut loaded_hdma, DOTS_PER_FRAME);
        assert_eq!(
            format!("{original:?}"),
            format!("{loaded:?}"),
            "case {case}"
        );
    }
}

#[test]
fn a_corrupted_save_state_is_rejected_or_runs_without_panicking() {
    let mut random = TestRandom(0xD1B5_4A32_D192_ED03);
    for (model, double_speed) in TIMINGS {
        let mut scene = random_scene(&mut random, model, double_speed);
        let mut hdma = HdmaState::new();
        // Into mode 3 of a line in the middle of the frame, with objects and the window about.
        tick_dots(&mut scene, &mut hdma, DOTS_PER_FRAME + 70 * 456 + 120);
        let state = serialize(&scene);

        for offset in offsets_worth_corrupting(&state) {
            for value in [0x00, 0xFF, 0x80, state[offset] ^ 0x55, random.byte()] {
                let mut corrupted = state.clone();
                corrupted[offset] = value;
                if let Ok(mut gpu) = load(&corrupted, model) {
                    let mut hdma = HdmaState::new();
                    gpu.run_dots(2 * 456, &mut hdma, true);
                    tick_dots(&mut gpu, &mut hdma, 2 * 456);
                }
            }
        }
    }
}

/// The offsets of every byte of `state`, a serialized PPU, but the contents of the frame buffer,
/// VRAM and OAM: any value is valid there, and leaving those thousands of bytes out keeps the
/// corruption test fast. Their names and lengths are still corrupted.
fn offsets_worth_corrupting(state: &[u8]) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut position = 0;
    // Each field is a name with its length byte in front, the value's length as a u32, and the
    // value; a name of length 0 ends the fields.
    while state[position] != 0 {
        let name_end = position + 1 + usize::from(state[position]);
        let value_start = name_end + size_of::<u32>();
        let length = u32::from_le_bytes(state[name_end..value_start].try_into().unwrap());
        let value_end = value_start + usize::try_from(length).unwrap();
        let contents_valid = matches!(
            &state[position + 1..name_end],
            b"frame_buffer" | b"video_ram" | b"object_attribute_memory"
        );
        offsets.extend(
            position..if contents_valid {
                value_start
            } else {
                value_end
            },
        );
        position = value_end;
    }
    offsets.push(position);
    offsets
}
