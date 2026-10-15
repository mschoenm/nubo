//! Randomized tests of the PPU on random scenes: random VRAM, OAM, palettes and registers on DMG,
//! CGB and CGB in double speed, with register writes at random dots.
//!
//! They check that the bulk runs (`Gpu::run_dots` with its steady stretches and plain pixels) give
//! exactly what running dot by dot does, and that a save state taken at any dot carries on
//! exactly like the PPU it came from and can't make the PPU panic when corrupted.
//!
//! `NUBO_RANDOM_SCENES` sets how many scenes the comparison of bulk runs with running dot by dot
//! tries (18 by default, a few seconds in a debug build), and `NUBO_RANDOM_SEED` the seed it starts
//! from, which a failure reports. The nightly run tries thousands with a new seed each night.

use crate::address_bus::MemoryMapped;
use crate::address_bus::hdma::HdmaState;
use crate::gpu::{DOTS_PER_FRAME, Gpu};
use crate::model::Model;
use crate::serializable::Serializable;

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

/// The three timing variants (DMG, CGB, CGB in double speed), as a model and a speed.
const TIMINGS: [(Model, bool); 3] = [
    (Model::Dmg, false),
    (Model::CgbE, false),
    (Model::CgbE, true),
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
/// interrupt is requested, keeping the LCD on. LYC and WY often get the current or the next line,
/// where their late effects show.
fn write_random_register(gpu: &mut Gpu, random: &mut TestRandom) {
    let near_line = gpu.ly.wrapping_add(random.byte_below(2));
    let (address, value) = match random.below(9) {
        0 => (0xFF40, 0x80 | random.byte()),
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

fn tick_dots(gpu: &mut Gpu, hdma: &mut HdmaState, dots: u32) {
    for _ in 0..dots {
        gpu.tick_dot(hdma);
    }
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
        for frame in 0..2 {
            let mut left = DOTS_PER_FRAME;
            while left > 0 {
                let stretch = (1 + random.below(4000)).min(left);
                tick_dots(&mut by_dot, &mut by_dot_hdma, stretch);
                let mut bulk_left = stretch;
                while bulk_left > 0 {
                    let chunk = (1 + random.below(700)).min(bulk_left);
                    in_bulk.run_dots(chunk, &mut in_bulk_hdma, random.chance(50));
                    bulk_left -= chunk;
                }
                left -= stretch;

                // Interrupt requests are compared and cleared after every stretch, so that one
                // requested on one side only can't be hidden by a later one on both.
                let requests = |gpu: &Gpu| {
                    (
                        gpu.line,
                        gpu.line_dot,
                        gpu.stat_interrupt,
                        gpu.vblank_interrupt,
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
                }

                if random.chance(40) {
                    let seed = random.next() | 1;
                    write_random_register(&mut by_dot, &mut TestRandom(seed));
                    write_random_register(&mut in_bulk, &mut TestRandom(seed));
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

        // The scalar state sits before the frame buffer, VRAM and OAM, and after them.
        let offsets = (0..400).chain(state.len() - 200..state.len());
        for offset in offsets {
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
