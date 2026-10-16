//! Unit tests for the APU, driven through its registers like a game would.

use super::*;

const DIV_APU_PERIOD_M_CYCLES: usize = 1024;

fn powered_apu(cgb_mode: bool) -> Apu {
    let mut apu = Apu::new();
    apu.set_model(if cgb_mode { Model::CgbE } else { Model::Dmg });
    apu.write_byte(0xFF26, 0x80);
    apu
}

fn write_all(apu: &mut Apu, writes: &[(u16, u8)]) {
    for &(address, value) in writes {
        apu.write_byte(address, value);
    }
}

/// Drives the APU's timing the way the address bus does, one M-cycle at a time.
struct Clock {
    system_counter: u16,
}

impl Clock {
    fn new() -> Self {
        Clock { system_counter: 0 }
    }

    fn run(&mut self, apu: &mut Apu, m_cycles: usize) {
        for _ in 0..m_cycles {
            apu.begin_machine_cycle();
            let previous = self.system_counter;
            self.system_counter = self.system_counter.wrapping_add(4);
            apu.system_counter_changed(previous, self.system_counter, false);
            apu.step();
        }
    }
}

fn collect_pcm12(apu: &mut Apu, clock: &mut Clock, m_cycles: usize) -> Vec<u8> {
    (0..m_cycles)
        .map(|_| {
            clock.run(apu, 1);
            apu.pcm12()
        })
        .collect()
}

#[test]
fn reads_registers_with_unused_bits_set() {
    let mut apu = powered_apu(false);
    write_all(
        &mut apu,
        &[
            (0xFF10, 0x00),
            (0xFF11, 0x00),
            (0xFF1A, 0x00),
            (0xFF1C, 0x00),
            (0xFF23, 0x00),
        ],
    );

    assert_eq!(apu.read_byte(0xFF10), 0x80);
    assert_eq!(apu.read_byte(0xFF11), 0x3F);
    assert_eq!(apu.read_byte(0xFF13), 0xFF);
    assert_eq!(apu.read_byte(0xFF1A), 0x7F);
    assert_eq!(apu.read_byte(0xFF1C), 0x9F);
    assert_eq!(apu.read_byte(0xFF23), 0xBF);
    assert_eq!(apu.read_byte(0xFF26), 0xF0);
}

#[test]
fn triggering_a_square_channel_with_its_dac_on_activates_it() {
    let mut apu = powered_apu(false);
    write_all(&mut apu, &[(0xFF12, 0xF0), (0xFF14, 0x80)]);
    assert_eq!(apu.read_byte(0xFF26) & 0x01, 0x01);
}

#[test]
fn triggering_a_square_channel_with_its_dac_off_does_not_activate_it() {
    let mut apu = powered_apu(false);
    write_all(&mut apu, &[(0xFF12, 0x00), (0xFF14, 0x80)]);
    assert_eq!(apu.read_byte(0xFF26) & 0x01, 0x00);
}

#[test]
fn disabling_the_dac_deactivates_the_channel() {
    let mut apu = powered_apu(false);
    write_all(&mut apu, &[(0xFF17, 0xF0), (0xFF19, 0x80)]);
    assert_eq!(apu.read_byte(0xFF26) & 0x02, 0x02);

    apu.write_byte(0xFF17, 0x07);
    assert_eq!(apu.read_byte(0xFF26) & 0x02, 0x00);
}

#[test]
fn length_counter_disables_the_channel_when_it_expires() {
    let mut apu = powered_apu(false);
    let mut clock = Clock::new();
    write_all(&mut apu, &[(0xFF11, 0x3F), (0xFF12, 0xF0), (0xFF14, 0xC0)]);
    assert_eq!(apu.read_byte(0xFF26) & 0x01, 0x01);

    clock.run(&mut apu, DIV_APU_PERIOD_M_CYCLES * 2);
    assert_eq!(apu.read_byte(0xFF26) & 0x01, 0x00);
}

#[test]
fn square_output_follows_the_duty_cycle() {
    let mut apu = powered_apu(true);
    let mut clock = Clock::new();
    write_all(
        &mut apu,
        &[
            (0xFF11, 0x80),
            (0xFF12, 0xF0),
            (0xFF13, 0xFF),
            (0xFF14, 0x87),
        ],
    );

    let samples = collect_pcm12(&mut apu, &mut clock, 64);
    let high = samples
        .iter()
        .skip(16)
        .filter(|&&sample| sample & 0x0F == 0x0F)
        .count();
    let low = samples
        .iter()
        .skip(16)
        .filter(|&&sample| sample & 0x0F == 0)
        .count();
    assert_eq!(high, low);
}

#[test]
fn envelope_raises_the_volume_over_time() {
    let mut apu = powered_apu(true);
    let mut clock = Clock::new();
    write_all(
        &mut apu,
        &[
            (0xFF11, 0xC0),
            (0xFF12, 0x09),
            (0xFF13, 0xFF),
            (0xFF14, 0x87),
        ],
    );

    let early_peak = collect_pcm12(&mut apu, &mut clock, 64)
        .into_iter()
        .max()
        .unwrap();
    clock.run(&mut apu, DIV_APU_PERIOD_M_CYCLES * 32);
    let late_peak = collect_pcm12(&mut apu, &mut clock, 64)
        .into_iter()
        .max()
        .unwrap();

    assert_eq!(early_peak, 0);
    assert!(late_peak > 0);
}

#[test]
fn sweep_overflow_disables_channel_1() {
    let mut apu = powered_apu(false);
    let mut clock = Clock::new();
    write_all(
        &mut apu,
        &[
            (0xFF10, 0x11),
            (0xFF12, 0xF0),
            (0xFF13, 0xFF),
            (0xFF14, 0x87),
        ],
    );

    clock.run(&mut apu, 8);
    assert_eq!(apu.read_byte(0xFF26) & 0x01, 0x00);
}

#[test]
fn wave_channel_plays_wave_ram() {
    let mut apu = powered_apu(true);
    let mut clock = Clock::new();
    for address in 0xFF30..=0xFF3F {
        apu.write_byte(address, 0xF0);
    }
    write_all(
        &mut apu,
        &[
            (0xFF1A, 0x80),
            (0xFF1C, 0x20),
            (0xFF1D, 0xFE),
            (0xFF1E, 0x87),
        ],
    );

    let samples: Vec<u8> = (0..64)
        .map(|_| {
            clock.run(&mut apu, 1);
            apu.pcm34() & 0x0F
        })
        .collect();
    assert!(samples.contains(&0x0F));
    assert!(samples.contains(&0x00));
}

#[test]
fn noise_channel_output_changes_as_the_lfsr_steps() {
    let mut apu = powered_apu(true);
    let mut clock = Clock::new();
    write_all(&mut apu, &[(0xFF21, 0xF0), (0xFF22, 0x00), (0xFF23, 0x80)]);

    let samples: Vec<u8> = (0..256)
        .map(|_| {
            clock.run(&mut apu, 1);
            apu.pcm34() >> 4
        })
        .collect();
    assert!(samples.contains(&0x0F));
    assert!(samples.contains(&0x00));
}

#[test]
fn powering_off_clears_the_registers() {
    let mut apu = powered_apu(true);
    write_all(&mut apu, &[(0xFF24, 0x77), (0xFF26, 0x00)]);
    assert_eq!(apu.read_byte(0xFF24), 0x00);

    apu.write_byte(0xFF24, 0x77);
    assert_eq!(apu.read_byte(0xFF24), 0x00);
}

#[test]
fn dmg_keeps_length_writes_while_powered_off() {
    let mut apu = powered_apu(false);
    let mut clock = Clock::new();
    write_all(
        &mut apu,
        &[
            (0xFF26, 0x00),
            (0xFF11, 0x3F),
            (0xFF26, 0x80),
            (0xFF12, 0xF0),
            (0xFF14, 0xC0),
        ],
    );

    clock.run(&mut apu, DIV_APU_PERIOD_M_CYCLES * 2);
    assert_eq!(apu.read_byte(0xFF26) & 0x01, 0x00);
}

#[test]
fn wave_ram_reads_while_playing_are_blocked_on_dmg_outside_wave_reads() {
    let mut apu = powered_apu(false);
    write_all(
        &mut apu,
        &[
            (0xFF30, 0x12),
            (0xFF1A, 0x80),
            (0xFF1D, 0x00),
            (0xFF1E, 0x80),
        ],
    );
    assert_eq!(apu.read_byte(0xFF30), 0xFF);
}

#[test]
fn wave_ram_reads_while_playing_return_the_current_byte_on_cgb() {
    let mut apu = powered_apu(true);
    write_all(
        &mut apu,
        &[
            (0xFF30, 0x12),
            (0xFF31, 0x34),
            (0xFF1A, 0x80),
            (0xFF1D, 0x00),
            (0xFF1E, 0x80),
        ],
    );
    assert_eq!(apu.read_byte(0xFF31), 0x12);
}

fn serialized(apu: &Apu) -> Vec<u8> {
    let mut data = Vec::new();
    apu.serialize(&mut data).unwrap();
    data
}

#[test]
fn restores_its_own_save_state() {
    let mut apu = powered_apu(true);
    let mut clock = Clock::new();
    write_all(&mut apu, &[(0xFF12, 0xF0), (0xFF14, 0x80)]);
    clock.run(&mut apu, 100);

    let mut restored = Apu::new();
    restored
        .deserialize(&mut serialized(&apu).as_slice())
        .unwrap();
    assert_eq!(restored.read_byte(0xFF26), apu.read_byte(0xFF26));
    assert_eq!(restored.pcm12(), apu.pcm12());
}

/// An APU for `model` playing all four channels, with the register writes the
/// scenario below starts with.
fn playing_apu(model: Model) -> Apu {
    let mut apu = Apu::new();
    apu.set_model(model);
    apu.write_byte(0xFF26, 0x80);
    for (index, address) in (0xFF30..=0xFF3F).enumerate() {
        apu.write_byte(address, (index as u8).wrapping_mul(0x37));
    }
    for &(at, address, value) in &SCENARIO_WRITES {
        if at == 0 {
            apu.write_byte(address, value);
        }
    }
    apu
}

/// The queued samples, as bits.
fn sample_bits(apu: &Apu) -> Vec<u32> {
    let samples = apu.left_samples().iter().chain(apu.right_samples());
    samples.map(|sample| sample.to_bits()).collect()
}

#[test]
fn a_loaded_state_sounds_exactly_like_the_run_it_was_saved_from() {
    for model in Model::ALL {
        let mut apu = playing_apu(model);
        let mut clock = Clock::new();
        clock.run(&mut apu, 5003);
        apu.clear_audio_buffers();

        let mut loaded = Apu::new();
        loaded.set_model(model);
        loaded
            .deserialize(&mut serialized(&apu).as_slice())
            .unwrap();
        let mut loaded_clock = Clock {
            system_counter: clock.system_counter,
        };
        clock.run(&mut apu, 20000);
        loaded_clock.run(&mut loaded, 20000);
        apu.catch_up();
        loaded.catch_up();
        assert!(sample_bits(&apu).len() > 500);
        assert!(
            sample_bits(&apu) == sample_bits(&loaded),
            "the loaded state sounds different on {model}"
        );
    }
}

#[test]
fn debug_output_does_not_depend_on_when_the_apu_caught_up() {
    for model in [Model::Dmg, Model::CgbE, Model::Agb] {
        let mut eager = playing_apu(model);
        let mut lazy = eager.clone();
        let mut eager_clock = Clock::new();
        for _ in 0..5003 {
            eager_clock.run(&mut eager, 1);
            eager.catch_up();
        }
        Clock::new().run(&mut lazy, 5003);
        assert_eq!(format!("{eager:?}"), format!("{lazy:?}"), "on {model}");
    }
}

#[test]
fn a_state_loads_into_an_emulator_with_another_sample_rate() {
    let mut apu = playing_apu(Model::CgbE);
    apu.set_sample_rate(48_000);
    Clock::new().run(&mut apu, 5003);

    let mut loaded = Apu::new();
    loaded.set_model(Model::CgbE);
    loaded.set_sample_rate(8000);
    loaded
        .deserialize(&mut serialized(&apu).as_slice())
        .unwrap();
    assert_eq!(samples_in_one_second(&mut loaded, false), 8000);
}

#[test]
fn rejects_a_save_state_with_an_out_of_range_wave_position() {
    let mut apu = powered_apu(true);
    write_all(&mut apu, &[(0xFF1A, 0x80), (0xFF1E, 0x80)]);
    apu.state.wave.current_sample_index = 0xFF;

    let mut restored = Apu::new();
    assert!(
        restored
            .deserialize(&mut serialized(&apu).as_slice())
            .is_err()
    );
}

#[test]
fn rejects_a_save_state_with_an_out_of_range_duty_step() {
    let mut apu = powered_apu(true);
    apu.state.square1.current_sample_index = 0xFF;

    let mut restored = Apu::new();
    assert!(
        restored
            .deserialize(&mut serialized(&apu).as_slice())
            .is_err()
    );
}

/// Bytes of a serialized APU before its BLEP buffers, where most of what a
/// corruption can break lies.
const STATE_BYTES_BEFORE_BLEP: usize = 200;
/// Byte values at the edges of the ranges the APU's fields have.
const EDGE_BYTES: [u8; 10] = [0x00, 0x01, 0x07, 0x08, 0x0F, 0x10, 0x7F, 0x80, 0xFE, 0xFF];

/// Loads `state` into a new APU for `model` and, if it loads, plays on with
/// every channel triggered and a speed switch, so the loaded values go
/// through the channels' arithmetic. Returns whether it loaded.
fn play_on_from(state: &[u8], model: Model) -> bool {
    let mut loaded = Apu::new();
    loaded.set_model(model);
    if loaded.deserialize(&mut &state[..]).is_err() {
        return false;
    }
    let mut clock = Clock::new();
    // Four DIV-APU events, so one of them clocks the sweep.
    clock.run(&mut loaded, 4 * DIV_APU_PERIOD_M_CYCLES + 1);
    write_all(
        &mut loaded,
        &[
            (0xFF14, 0x87),
            (0xFF19, 0x87),
            (0xFF1E, 0x87),
            (0xFF23, 0x80),
        ],
    );
    clock.run(&mut loaded, 1500);
    loaded.set_cgb_double_speed(model.is_cgb());
    clock.run(&mut loaded, 1500);
    loaded.pcm12();
    loaded.pcm34();
    loaded.catch_up();
    assert!(
        sample_bits(&loaded)
            .iter()
            .all(|&bits| f32::from_bits(bits).is_finite()),
        "a loaded state made the audio output infinite"
    );
    true
}

#[test]
fn corrupted_save_states_never_make_the_apu_panic() {
    // One thread per model keeps this test at a few seconds.
    std::thread::scope(|scope| {
        for (index, model) in Model::ALL.into_iter().enumerate() {
            scope.spawn(move || {
                let mut apu = playing_apu(model);
                Clock::new().run(&mut apu, 3001);
                let state = serialized(&apu);
                let mut loaded = 0;
                for (position, value) in (0..STATE_BYTES_BEFORE_BLEP)
                    .flat_map(|position| EDGE_BYTES.map(|value| (position, value)))
                {
                    let mut corrupted = state.clone();
                    corrupted[position] = value;
                    loaded += usize::from(play_on_from(&corrupted, model));
                }
                let mut random = Random(0x0BAD_5EED + index as u64);
                for _ in 0..300 {
                    let mut corrupted = state.clone();
                    for _ in 0..2 + random.below(4) {
                        let position = random.below(corrupted.len());
                        corrupted[position] = random.byte();
                    }
                    loaded += usize::from(play_on_from(&corrupted, model));
                }
                assert!(
                    loaded > 1000,
                    "only {loaded} corrupted states loaded on {model}"
                );
            });
        }
    });
}

#[test]
fn random_bytes_never_make_loading_a_state_panic() {
    let mut random = Random(0xF00D);
    for _ in 0..2000 {
        let length = random.below(2000);
        let bytes: Vec<u8> = (0..length).map(|_| random.byte()).collect();
        let mut apu = Apu::new();
        let _ = apu.deserialize(&mut bytes.as_slice());
    }
}

#[test]
fn envelope_volume_wraps_like_a_4_bit_counter() {
    let mut apu = powered_apu(false);

    apu.write_byte(0xFF12, 0x09);
    apu.state.square1.envelope.volume = 15;
    apu.tick_square_envelope(Square::Channel1);
    assert_eq!(apu.state.square1.envelope.volume, 0);

    apu.write_byte(0xFF21, 0x01);
    apu.state.noise.envelope.volume = 0;
    apu.tick_noise_envelope();
    assert_eq!(apu.state.noise.envelope.volume, 15);
}

/// How the scenario below drives the APU.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Driver {
    /// Runs the channels in every M-cycle, as if nothing were batched.
    EveryCycle,
    /// Steps like the address bus and lets the APU batch M-cycles.
    Lazy,
    /// Like `Lazy`, but moves through stretches without DIV-APU edges with
    /// `advance_cycles`.
    Bulk,
}

/// Register writes the scenario makes after the M-cycle with the given index:
/// all four channels play, then channel 1 sweeps and changes its duty cycle,
/// the noise speeds up and turns narrow, the panning changes, the wave channel
/// is muted, unmuted and stops on its length counter, channel 2 plays at
/// volume 0, the envelopes get zombie mode writes, the noise slows down,
/// keeps counting with its DAC off and starts again, and the wave channel
/// starts again with the wave RAM byte its timer read from the CPU's bus while
/// it was stopped.
const SCENARIO_WRITES: [(usize, u16, u8); 36] = [
    (0, 0xFF24, 0x77),
    (0, 0xFF25, 0xFF),
    (0, 0xFF11, 0x80),
    (0, 0xFF12, 0xF3),
    (0, 0xFF13, 0x00),
    (0, 0xFF14, 0x87),
    (0, 0xFF16, 0x40),
    (0, 0xFF17, 0xA1),
    (0, 0xFF18, 0xC0),
    (0, 0xFF19, 0x87),
    (0, 0xFF1A, 0x80),
    (0, 0xFF1C, 0x20),
    (0, 0xFF1D, 0x80),
    (0, 0xFF1E, 0x87),
    (0, 0xFF21, 0xF1),
    (0, 0xFF22, 0x31),
    (0, 0xFF23, 0x80),
    (3000, 0xFF10, 0x21),
    (3000, 0xFF14, 0x86),
    (4500, 0xFF11, 0xC0),
    (6000, 0xFF22, 0x00),
    (6000, 0xFF25, 0x5A),
    (7500, 0xFF1C, 0x00),
    (7500, 0xFF22, 0x3A),
    (9000, 0xFF1C, 0x40),
    (10500, 0xFF17, 0x08),
    (10500, 0xFF19, 0x87),
    (12000, 0xFF17, 0xA8),
    (16000, 0xFF1B, 0xFE),
    (16000, 0xFF1E, 0xC7),
    (18000, 0xFF22, 0x70),
    (20000, 0xFF21, 0x00),
    (23000, 0xFF21, 0xF7),
    (23000, 0xFF23, 0x80),
    (26000, 0xFF22, 0x02),
    (27000, 0xFF1E, 0x87),
];
/// What a scenario does to the APU, M-cycle by M-cycle.
struct Scenario {
    /// Register writes made after the M-cycle with the given index, in order.
    writes: Vec<(usize, u16, u8)>,
    /// M-cycles after which DIV is written, which resets the system counter.
    div_writes: Vec<usize>,
    /// The M-cycle at whose start CGB models switch to double speed.
    speed_switch: usize,
    sample_rate: u32,
    cycles: usize,
}

/// The scenario `SCENARIO_WRITES` describes.
fn fixed_scenario() -> Scenario {
    Scenario {
        writes: SCENARIO_WRITES.to_vec(),
        div_writes: Vec::new(),
        speed_switch: 15000,
        sample_rate: 44100,
        cycles: 30000,
    }
}

/// A small pseudo-random number generator (xorshift64*), so the random
/// scenarios are the same on every run.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number below `limit`.
    fn below(&mut self, limit: usize) -> usize {
        (self.next() % limit as u64) as usize
    }

    fn byte(&mut self) -> u8 {
        self.next() as u8
    }
}

/// The sound registers random scenarios write, NR52 aside.
const RANDOM_WRITE_REGISTERS: [u16; 20] = [
    0xFF10, 0xFF11, 0xFF12, 0xFF13, 0xFF14, 0xFF16, 0xFF17, 0xFF18, 0xFF19, 0xFF1A, 0xFF1B, 0xFF1C,
    0xFF1D, 0xFF1E, 0xFF20, 0xFF21, 0xFF22, 0xFF23, 0xFF24, 0xFF25,
];

/// A scenario that starts like the fixed one and then writes random values
/// to random sound registers and wave RAM at random moments, often triggering
/// channels, sometimes writing DIV or turning the APU off and on, and plays at
/// a random sample rate.
fn random_scenario(seed: u64) -> Scenario {
    let mut random = Random(seed);
    let cycles = 40000;
    let mut writes: Vec<_> = SCENARIO_WRITES
        .iter()
        .copied()
        .filter(|&(at, _, _)| at == 0)
        .collect();
    let mut div_writes = Vec::new();
    let mut cycle = 0;
    loop {
        cycle += 1 + random.below(1200);
        if cycle >= cycles {
            break;
        }
        for _ in 0..1 + random.below(3) {
            match random.below(40) {
                0 => {
                    writes.push((cycle, 0xFF26, 0x00));
                    writes.push((cycle + random.below(40), 0xFF26, 0x80));
                }
                1 | 2 => div_writes.push(cycle),
                3..=6 => writes.push((cycle, 0xFF30 + random.below(16) as u16, random.byte())),
                _ => {
                    let address =
                        RANDOM_WRITE_REGISTERS[random.below(RANDOM_WRITE_REGISTERS.len())];
                    let mut value = random.byte();
                    if matches!(address, 0xFF14 | 0xFF19 | 0xFF1E | 0xFF23) && random.below(2) == 0
                    {
                        value |= 0x80;
                    }
                    writes.push((cycle, address, value));
                }
            }
        }
    }
    writes.sort_by_key(|&(at, _, _)| at);
    Scenario {
        writes,
        div_writes,
        speed_switch: random.below(cycles),
        sample_rate: SAMPLE_RATES[random.below(SAMPLE_RATES.len())],
        cycles,
    }
}

/// The address on the CPU's bus in the scenario's M-cycle `cycle`.
fn scenario_bus_address(cycle: usize) -> u16 {
    (cycle as u16).wrapping_mul(7)
}

/// Adds the M-cycle and the samples to `audio` and empties the buffers when
/// they are full, like `Emulator::step_until_next_audio_buffer`.
fn take_full_buffers(apu: &mut Apu, cycle: usize, audio: &mut Vec<u32>) {
    if apu.audio_buffers_full() {
        audio.push(cycle as u32);
        let samples = apu.left_samples().iter().chain(apu.right_samples());
        audio.extend(samples.map(|sample| sample.to_bits()));
        apu.clear_audio_buffers();
    }
}

/// The audio buffers (with the M-cycles in which they filled up) and the
/// PCM12/PCM34 values `scenario` produces on `model` with `driver`, and the
/// APU's state at the end.
fn run_scenario(scenario: &Scenario, model: Model, driver: Driver) -> (Vec<u32>, Vec<u8>, Vec<u8>) {
    let mut apu = Apu::new();
    apu.set_model(model);
    apu.set_sample_rate(scenario.sample_rate);
    apu.write_byte(0xFF26, 0x80);
    for (index, address) in (0xFF30..=0xFF3F).enumerate() {
        apu.write_byte(address, (index as u8).wrapping_mul(0x37));
    }

    let mut system_counter: u16 = 0;
    let mut audio = Vec::new();
    let mut pcm = Vec::new();
    let mut writes = scenario.writes.iter().peekable();
    let mut cycle = 0;
    while cycle < scenario.cycles {
        let next_event = scenario
            .writes
            .iter()
            .map(|&(at, _, _)| at)
            .chain(scenario.div_writes.iter().copied())
            .chain([scenario.speed_switch])
            .filter(|&at| at >= cycle)
            .min()
            .unwrap_or(scenario.cycles);
        let bulk_cycles = (apu.cycles_until_div_apu_edge() as usize - 1)
            .min(next_event - cycle)
            .min(apu.cycles_until_audio_buffers_full() as usize);
        if driver == Driver::Bulk && apu.can_advance_in_bulk() && bulk_cycles > 1 {
            // In the address bus's order: the M-cycles first, then the system
            // counter where they leave it.
            apu.advance_cycles(bulk_cycles as u32);
            let previous = system_counter;
            system_counter = system_counter.wrapping_add(4 * bulk_cycles as u16);
            apu.system_counter_changed(previous, system_counter, false);
            // The bus address doesn't matter in these M-cycles, but it is
            // saved, so it has to end up where single M-cycles leave it.
            apu.set_bus_address(scenario_bus_address(cycle + bulk_cycles - 1));
            cycle += bulk_cycles;
            pcm.push(apu.pcm12());
            pcm.push(apu.pcm34());
            take_full_buffers(&mut apu, cycle, &mut audio);
            continue;
        }

        apu.begin_machine_cycle();
        let previous = system_counter;
        system_counter = system_counter.wrapping_add(4);
        apu.system_counter_changed(previous, system_counter, false);
        if cycle == scenario.speed_switch && model.is_cgb() {
            apu.set_cgb_double_speed(true);
        }
        // A stopped wave channel reads wave RAM at the address on the CPU's bus.
        apu.set_bus_address(scenario_bus_address(cycle));
        apu.step();
        if driver == Driver::EveryCycle {
            apu.catch_up();
        }
        while let Some(&(_, address, value)) = writes.next_if(|&&(at, _, _)| at == cycle) {
            apu.write_byte(address, value);
        }
        if scenario.div_writes.contains(&cycle) {
            let previous = system_counter;
            system_counter = 0;
            apu.system_counter_changed(previous, system_counter, true);
        }
        if driver != Driver::Bulk {
            pcm.push(apu.pcm12());
            pcm.push(apu.pcm34());
        }
        cycle += 1;
        take_full_buffers(&mut apu, cycle, &mut audio);
    }

    apu.catch_up();
    let samples = apu.left_samples().iter().chain(apu.right_samples());
    audio.extend(samples.map(|sample| sample.to_bits()));
    (audio, pcm, serialized(&apu))
}

/// Checks that running `scenario` on `model` lazily and in bulk gives the
/// same audio, PCM12/PCM34 reads and end state as running every M-cycle on
/// its own.
fn assert_batching_changes_nothing(scenario: &Scenario, model: Model, name: &str) {
    let (samples, pcm, state) = run_scenario(scenario, model, Driver::EveryCycle);
    assert!(samples.len() > 200);
    let (lazy_samples, lazy_pcm, lazy_state) = run_scenario(scenario, model, Driver::Lazy);
    assert!(
        samples == lazy_samples,
        "lazy audio differs on {model}, {name}"
    );
    assert!(
        pcm == lazy_pcm,
        "lazy PCM12/PCM34 differ on {model}, {name}"
    );
    assert!(state == lazy_state, "lazy state differs on {model}, {name}");
    let (bulk_samples, _, bulk_state) = run_scenario(scenario, model, Driver::Bulk);
    assert!(
        samples == bulk_samples,
        "bulk audio differs on {model}, {name}"
    );
    assert!(state == bulk_state, "bulk state differs on {model}, {name}");
}

#[test]
fn batched_m_cycles_sound_exactly_like_single_ones() {
    let scenario = fixed_scenario();
    for model in Model::ALL {
        assert_batching_changes_nothing(&scenario, model, "fixed scenario");
    }
}

#[test]
fn batched_m_cycles_sound_exactly_like_single_ones_with_random_writes() {
    // One thread per model keeps this test at a few seconds.
    std::thread::scope(|scope| {
        for (index, model) in Model::ALL.into_iter().enumerate() {
            scope.spawn(move || {
                for run in 0..4 {
                    let seed = 0x9E37_79B9_7F4A_7C15 ^ (index as u64 * 16 + run);
                    let scenario = random_scenario(seed);
                    assert_batching_changes_nothing(&scenario, model, &format!("seed {seed:X}"));
                }
            });
        }
    });
}

/// Sample rates sound cards use, from Bluetooth headsets in hands-free mode
/// up to studio interfaces.
const SAMPLE_RATES: [u32; 6] = [8000, 16000, 44100, 48000, 96000, 192_000];

#[test]
fn knows_when_the_audio_buffers_fill_up() {
    for sample_rate in SAMPLE_RATES {
        for double_speed in [false, true] {
            let mut apu = powered_apu(true);
            let mut clock = Clock::new();
            apu.set_sample_rate(sample_rate);
            apu.set_cgb_double_speed(double_speed);
            clock.run(&mut apu, 37);
            apu.clear_audio_buffers();

            let cycles = apu.cycles_until_audio_buffers_full();
            clock.run(&mut apu, cycles as usize - 1);
            assert!(!apu.audio_buffers_full());
            clock.run(&mut apu, 1);
            assert!(apu.audio_buffers_full());
            assert_eq!(apu.cycles_until_audio_buffers_full(), 0);
        }
    }
}

/// The output samples `apu` queues in one emulated second.
fn samples_in_one_second(apu: &mut Apu, double_speed: bool) -> usize {
    let mut clock = Clock::new();
    apu.set_cgb_double_speed(double_speed);
    apu.clear_audio_buffers();
    let m_cycles_per_second =
        T_CYCLES_PER_SECOND as usize / usize::from(dots_per_m_cycle(double_speed));
    let mut samples = 0;
    for _ in 0..m_cycles_per_second {
        clock.run(apu, 1);
        if apu.audio_buffers_full() {
            samples += apu.left_samples().len();
            apu.clear_audio_buffers();
        }
    }
    apu.catch_up();
    samples + apu.left_samples().len()
}

#[test]
fn queues_exactly_the_sample_rate_per_emulated_second() {
    for sample_rate in SAMPLE_RATES {
        for double_speed in [false, true] {
            let mut apu = powered_apu(true);
            apu.set_sample_rate(sample_rate);
            assert_eq!(
                samples_in_one_second(&mut apu, double_speed),
                sample_rate as usize,
                "{sample_rate} Hz, double speed {double_speed}"
            );
        }
    }
}

#[test]
fn clamps_sample_rates_outside_the_accepted_range() {
    for (requested, expected) in [(0, MIN_SAMPLE_RATE), (u32::MAX, MAX_SAMPLE_RATE)] {
        let mut apu = powered_apu(true);
        apu.set_sample_rate(requested);
        assert_eq!(samples_in_one_second(&mut apu, false), expected as usize);
    }
}
