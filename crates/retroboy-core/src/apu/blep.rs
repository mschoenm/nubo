use std::f64::consts::PI;

pub(super) const BLEP_WIDTH: usize = 64;
const BLEP_PHASES: usize = 256;

pub(super) type BlepTable = Box<[[f32; BLEP_WIDTH]; BLEP_PHASES]>;

pub(super) fn generate_blep_table() -> BlepTable {
    let mut table = Box::new([[0.0f32; BLEP_WIDTH]; BLEP_PHASES]);
    let total_points = BLEP_PHASES * BLEP_WIDTH;
    let lowpass = 15.0 / 16.0;

    let a0: f64 = 7938.0 / 18608.0;
    let a1: f64 = 9240.0 / 18608.0;
    let a2: f64 = 1430.0 / 18608.0;

    let mut master = vec![0.0f64; total_points];
    let mut global_sum: f64 = 0.0;

    for i in 0..total_points {
        let window_angle = 2.0 * PI * i as f64 / total_points as f64;
        let sinc_angle = PI * lowpass * (i as f64 - total_points as f64 / 2.0) / BLEP_PHASES as f64;
        let window = a0 - a1 * window_angle.cos() + a2 * (2.0 * window_angle).cos();
        let sinc = if sinc_angle == 0.0 { 1.0 } else { sinc_angle.sin() / sinc_angle };
        master[i] = sinc * window;
        global_sum += master[i];
    }

    for i in 0..total_points {
        master[i] /= global_sum;
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

#[derive(Debug)]
pub(super) struct BlepBuffer {
    buffer: [f32; BLEP_WIDTH],
    pos: usize,
    accumulator: f32,
}

impl BlepBuffer {
    pub(super) fn new() -> Self {
        BlepBuffer {
            buffer: [0.0; BLEP_WIDTH],
            pos: 0,
            accumulator: 0.0,
        }
    }

    pub(super) fn add_delta(&mut self, delta: f32, phase: usize, table: &BlepTable) {
        let phase = phase.min(BLEP_PHASES - 1);
        for i in 0..BLEP_WIDTH {
            let index = (self.pos + i) % BLEP_WIDTH;
            self.buffer[index] += delta * table[phase][i];
        }
    }

    pub(super) fn read(&mut self) -> f32 {
        self.accumulator += self.buffer[self.pos];
        self.buffer[self.pos] = 0.0;
        self.pos = (self.pos + 1) % BLEP_WIDTH;
        self.accumulator
    }

    pub(super) fn reset(&mut self) {
        self.buffer = [0.0; BLEP_WIDTH];
        self.pos = 0;
        self.accumulator = 0.0;
    }
}
