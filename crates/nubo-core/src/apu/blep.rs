//! Band-limited step (BLEP) synthesis.
//!
//! A channel's output jumps between levels in sudden steps. A sudden step
//! contains frequencies far above what the output sample rate can represent,
//! and sampling it directly folds them back down as audible aliasing (a harsh,
//! out-of-tune buzz). Instead, every step is replaced by a smooth, band-limited
//! version from a precomputed table. Each step adds a short impulse to a
//! buffer that spans the next `BLEP_WIDTH` output samples, and reading the
//! buffer adds those impulses up into the output level.

use crate::serializable::{FieldWriter, Serializable, array_encoding, read_fields};
use std::f64::consts::PI;
use std::io::{self, Read, Write};
use std::sync::LazyLock;

/// How many output samples each step is spread over.
pub(super) const BLEP_WIDTH: usize = 64;
/// A bound far above any level the audio output reaches (the channels'
/// levels add up to at most 1, or 1.75 on the AGB), which save states are
/// checked against: a corrupted level could otherwise grow into infinity as
/// steps add to it.
pub(super) const MAX_PLAUSIBLE_LEVEL: f32 = 16.0;
/// How many positions within one output sample the table distinguishes.
pub(super) const BLEP_PHASES: usize = 256;

/// For each phase, the impulse that a step at that position adds to the next
/// `BLEP_WIDTH` output samples.
pub(super) type BlepTable = Box<[[f32; BLEP_WIDTH]; BLEP_PHASES]>;

/// The table shared by every emulator. It never changes, so it is built once,
/// the first time it is used.
pub(super) static BLEP_TABLE: LazyLock<BlepTable> = LazyLock::new(generate_blep_table);

/// Builds the table from a windowed sinc low-pass filter (exact Blackman
/// window) that cuts off just below half the output sample rate. Each row sums
/// to exactly 1, so a step of size 1 raises the output by exactly 1.
fn generate_blep_table() -> BlepTable {
    let mut table = Box::new([[0.0f32; BLEP_WIDTH]; BLEP_PHASES]);
    let total_points = BLEP_PHASES * BLEP_WIDTH;
    let lowpass = 15.0 / 16.0;

    let a0: f64 = 7938.0 / 18608.0;
    let a1: f64 = 9240.0 / 18608.0;
    let a2: f64 = 1430.0 / 18608.0;

    let mut master = vec![0.0f64; total_points];
    let mut global_sum: f64 = 0.0;

    for (i, sample) in master.iter_mut().enumerate() {
        let window_angle = 2.0 * PI * i as f64 / total_points as f64;
        let sinc_angle = PI * lowpass * (i as f64 - total_points as f64 / 2.0) / BLEP_PHASES as f64;
        let window = a0 - a1 * window_angle.cos() + a2 * (2.0 * window_angle).cos();
        let sinc = if sinc_angle == 0.0 {
            1.0
        } else {
            sinc_angle.sin() / sinc_angle
        };
        *sample = sinc * window;
        global_sum += *sample;
    }

    for sample in &mut master {
        *sample /= global_sum;
    }

    for phase in 0..BLEP_PHASES {
        let mut error: f64 = 1.0;
        for tap in 0..BLEP_WIDTH {
            let mut sum: f64 = 0.0;
            for j in 0..BLEP_PHASES {
                let index = (tap as i64) * (BLEP_PHASES as i64) - (phase as i64) + (j as i64);
                if index >= 0 && (index as usize) < total_points {
                    sum += master[index as usize];
                }
            }
            error -= sum;
            table[phase][tap] = sum as f32;
        }
        table[phase][BLEP_WIDTH / 2] += error as f32;
    }

    table
}

/// A buffer of upcoming output changes for one side of the stereo output.
/// It is twice as long as the stretch of output samples a step changes, so
/// every step changes one straight run of it, which the compiler can
/// vectorize. Once reading reaches the middle, the second half moves to the
/// front.
#[derive(Clone, Debug)]
pub(super) struct BlepBuffer {
    buffer: [f32; BLEP_WIDTH * 2],
    /// The position of the current output sample, below `BLEP_WIDTH`.
    position: usize,
    /// The output level: every step read so far, added up.
    accumulator: f32,
}

impl BlepBuffer {
    pub(super) fn new() -> Self {
        BlepBuffer {
            buffer: [0.0; BLEP_WIDTH * 2],
            position: 0,
            accumulator: 0.0,
        }
    }

    /// Adds a step of size `delta` that happens `phase / BLEP_PHASES` of the
    /// way into the current output sample.
    pub(super) fn add_delta(&mut self, delta: f32, phase: usize, table: &BlepTable) {
        let row = &table[phase.min(BLEP_PHASES - 1)];
        let upcoming = &mut self.buffer[self.position..self.position + BLEP_WIDTH];
        for (level, step) in upcoming.iter_mut().zip(row) {
            *level += delta * step;
        }
    }

    /// Returns the output level for the current sample and moves to the next.
    pub(super) fn read(&mut self) -> f32 {
        self.accumulator += self.buffer[self.position];
        self.position += 1;
        if self.position == BLEP_WIDTH {
            self.buffer.copy_within(BLEP_WIDTH.., 0);
            self.buffer[BLEP_WIDTH..].fill(0.0);
            self.position = 0;
        }
        self.accumulator
    }
}

/// Save states keep the steps that are still being spread over the coming
/// output samples, so they are heard after loading just as they would have
/// been.
impl Serializable for BlepBuffer {
    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        let mut fields = FieldWriter::new(writer);
        let buffer_encoding = || array_encoding::<f32>(self.buffer.len());
        fields.field_with("buffer", buffer_encoding, |writer| {
            self.buffer
                .iter()
                .try_for_each(|level| level.serialize(writer))
        })?;
        fields.field(
            "position",
            &u8::try_from(self.position).expect("the position is below BLEP_WIDTH"),
        )?;
        fields.field("accumulator", &self.accumulator)?;
        fields.finish()
    }

    fn deserialize(&mut self, reader: &mut dyn Read) -> io::Result<()> {
        let mut position = u8::try_from(self.position).unwrap_or(u8::MAX);
        read_fields(reader, |name, bytes| {
            match name {
                "buffer" => self
                    .buffer
                    .iter_mut()
                    .try_for_each(|level| level.deserialize(bytes))?,
                "position" => position.deserialize(bytes)?,
                "accumulator" => self.accumulator.deserialize(bytes)?,
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        if usize::from(position) >= BLEP_WIDTH {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "BLEP buffer position out of range",
            ));
        }
        self.position = usize::from(position);
        let plausible = |level: &f32| level.abs() <= MAX_PLAUSIBLE_LEVEL;
        if self.buffer.iter().all(plausible) && plausible(&self.accumulator) {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "BLEP buffer level out of range",
            ))
        }
    }
}
