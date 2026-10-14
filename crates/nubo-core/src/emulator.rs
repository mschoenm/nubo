//! The public entry point of the emulator core.
//!
//! [`Emulator`] owns the CPU, which owns the address bus and through it the
//! rest of the hardware, and offers everything frontends can do with them.

use crate::address_bus::{AddressBus, CartridgeEffects, CartridgeHeader, read_header};
use crate::controller::{ControllerMapping, ControllerState};
use crate::cpu::{BusActivityEntry, Cpu, Registers};
use crate::error::{CheatError, LoadRomError};
use crate::joypad::Key;
use crate::model::Model;
use crate::rom_archive;
use std::ops::Range;

/// Where the cartridge header starts: the entry point the boot ROM jumps to.
const HEADER_START: usize = 0x0100;
/// The end of the cartridge header, after its checksum at 0x014D.
const HEADER_END: usize = 0x014E;
/// The end of the part of the header a Super Game Boy sends to the Super
/// Nintendo, which includes the global checksum at 0x014E-0x014F.
const SGB_HEADER_END: usize = 0x0150;

/// PPU dots (ticks of the 4 MHz clock) in one frame: 154 lines of 456 dots,
/// about 1/59.73 of a second. [`Emulator::run_frame`] runs to the next
/// multiple of it since power-on.
pub const DOTS_PER_FRAME: u64 = crate::gpu::DOTS_PER_FRAME as u64;

/// A Game Boy or Game Boy Color.
///
/// Create one with [`Emulator::new`] for the hardware to emulate, load a game
/// with [`Emulator::load_rom`], and then call
/// [`Emulator::step_until_next_audio_buffer`] in a loop to run it.
///
/// Its `Debug` output shows the complete state of the hardware, and test
/// runners compare it to prove two emulators are in exactly the same state
/// (see `clone_with_rom_matches_a_real_boot` and the test ROM runner's
/// `--verify-boot`). Keep it that way: a hand-written `Debug` that leaves
/// fields out, or state kept in a collection with an unpredictable order such
/// as a `HashMap`, would make those checks miss differences.
#[derive(Debug)]
pub struct Emulator {
    cpu: Cpu,
}

impl Emulator {
    /// Creates an emulator of `model` with no game loaded, ready to run that
    /// model's boot ROM, the startup program that shows the logo.
    pub fn new(model: Model) -> Self {
        let mut emulator = Emulator {
            cpu: Cpu::new(AddressBus::new(false)),
        };
        emulator.address_bus_mut().set_model(model);
        emulator.address_bus_mut().load_boot_rom(model.is_cgb());
        emulator
    }

    /// Creates an emulator whose whole address space is plain RAM.
    ///
    /// CPU test suites use this to check single instructions: write the test
    /// program with [`Emulator::write_test_memory`], set the registers with
    /// [`Emulator::set_registers`], and call [`Emulator::step`].
    pub fn for_cpu_tests() -> Self {
        Emulator {
            cpu: Cpu::new(AddressBus::new(true)),
        }
    }

    fn address_bus(&self) -> &AddressBus {
        self.cpu.address_bus()
    }

    fn address_bus_mut(&mut self) -> &mut AddressBus {
        self.cpu.address_bus_mut()
    }

    /// Inserts a game cartridge. `rom` is either a ROM file or a zip archive
    /// containing one. Returns the cartridge header, which describes the game.
    ///
    /// `cartridge_effects` lets the emulator read the clock and load and store
    /// battery-backed save data. Loading a ROM doesn't reset the rest of the
    /// hardware, so use a new emulator for each game.
    pub fn load_rom(
        &mut self,
        rom: &[u8],
        cartridge_effects: Box<dyn CartridgeEffects>,
    ) -> Result<CartridgeHeader, LoadRomError> {
        let rom = rom_archive::extract_rom(rom)?;
        self.address_bus_mut().load_rom(rom, cartridge_effects)
    }

    /// A copy of this emulator, in exactly the state it is in, with the game
    /// in `rom` inserted instead of the current one.
    ///
    /// Test runners use this to skip the boot ROM: they let it run once and
    /// start each test ROM from a copy taken when it hands over to the
    /// cartridge. That matches booting the test ROM itself as long as both
    /// ROMs agree on [`Emulator::boot_rom_input`], the only bytes the boot
    /// ROM looks at.
    pub fn clone_with_rom(
        &self,
        rom: &[u8],
        cartridge_effects: Box<dyn CartridgeEffects>,
    ) -> Result<Emulator, LoadRomError> {
        let rom = rom_archive::extract_rom(rom)?;
        Ok(Emulator {
            cpu: self.cpu.clone_with_rom(rom, cartridge_effects)?,
        })
    }

    /// The part of a ROM that `model`'s boot ROM, and the hand-over to the
    /// cartridge that follows it, can read: the entry point at 0x0100 and the
    /// cartridge header up to its checksum. Super Game Boys also send the
    /// global checksum after it to the Super Nintendo, which changes their
    /// timing. Two ROMs that agree on these bytes leave the boot ROM in the
    /// same state, which `clone_with_rom_matches_a_real_boot` checks.
    pub fn boot_rom_input(model: Model) -> Range<usize> {
        if model.is_sgb() {
            HEADER_START..SGB_HEADER_END
        } else {
            HEADER_START..HEADER_END
        }
    }

    /// Reads the cartridge header of a ROM file or a zip archive containing
    /// one, without loading it. Frontends use it to list games and to pick
    /// the hardware model before starting one. It fails like
    /// [`Emulator::load_rom`] would.
    pub fn read_rom_header(rom: &[u8]) -> Result<CartridgeHeader, LoadRomError> {
        read_header(&rom_archive::extract_rom(rom)?)
    }

    /// Replaces the contents of the cartridge RAM, for example with a save file.
    pub fn set_cartridge_ram(&mut self, ram: &[u8]) {
        self.address_bus_mut().set_cartridge_ram(ram.to_vec());
    }

    /// The contents of the cartridge RAM, which holds the game's save data.
    pub fn cartridge_ram(&self) -> &[u8] {
        self.address_bus().cartridge_ram()
    }

    /// The Game Boy hardware being emulated.
    pub fn model(&self) -> Model {
        self.address_bus().model()
    }

    /// Sets how many audio samples per emulated second the emulator produces,
    /// exactly. Rates from 1,000 to 1,048,576 Hz are accepted; others are
    /// clamped to that range. The rate belongs to the sound card, not the
    /// console: save states don't hold it, and a state loaded into an
    /// emulator with another rate goes on at this emulator's rate.
    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.address_bus_mut().set_sample_rate(sample_rate);
    }

    /// Runs one CPU instruction, with the rest of the hardware running along.
    /// While the CPU is halted, stopped or locked up, a step runs one machine
    /// cycle (four clock ticks), or, while the CPU is halted and nothing can
    /// wake it for a while, that whole stretch at once (up to about a thousand
    /// machine cycles), so the time one step covers varies.
    pub fn step(&mut self) {
        self.cpu.step();
        self.finish_running();
    }

    /// Runs until a full buffer of audio has been produced and returns the
    /// left and right channel samples. Calling this in a loop and playing the
    /// audio keeps the emulator running at the right speed.
    pub fn step_until_next_audio_buffer(&mut self) -> (&[f32], &[f32]) {
        self.address_bus_mut().clear_audio_buffers();

        while !self.address_bus().audio_buffers_full() {
            self.cpu.step();
        }
        self.finish_running();

        self.address_bus().audio_samples()
    }

    /// Runs one frame's worth of time, for a frontend that calls it once per
    /// frame of the screen it shows on, and adds every audio sample made in
    /// that time to the end of `left` and `right`.
    ///
    /// It runs until the time since power-on reaches the next multiple of
    /// [`DOTS_PER_FRAME`] PPU dots: it counts time, not frames. A CPU step
    /// can't stop halfway, so a call may end up to one step past that point
    /// (an instruction with an interrupt dispatch, usually at most 44 dots,
    /// or a general-purpose VRAM DMA, up to about 4,100), and the next call
    /// is that much shorter: calls never drift from the emulated time. The
    /// point comes from the time alone, so after loading a save state the next
    /// call ends where it did in the run the state was saved from.
    ///
    /// Each call normally finishes one frame, which [`Emulator::take_frame`]
    /// then returns. While the game has the screen off, the PPU still hands
    /// over a frame every [`DOTS_PER_FRAME`] dots, holding what it last drew
    /// (white on a monochrome model in STOP mode), just as frontends that run
    /// the emulator by audio buffers get. Frames come at the PPU's own phase,
    /// which turning the screen on or off moves, so when a frame falls within
    /// a call's overshoot past the boundary, that call finishes none and the
    /// next one two, of which [`Emulator::take_frame`] returns the latest.
    pub fn run_frame(&mut self, left: &mut Vec<f32>, right: &mut Vec<f32>) {
        let frame_end = (self.address_bus().elapsed_dots() / DOTS_PER_FRAME + 1) * DOTS_PER_FRAME;
        let address_bus = self.address_bus_mut();
        address_bus.clear_audio_buffers();
        address_bus.set_dot_limit(frame_end);
        while self.address_bus().elapsed_dots_including_waiting() < frame_end {
            self.cpu.step();
            if self.address_bus().audio_buffers_full() {
                self.address_bus_mut().take_audio_samples(left, right);
            }
        }
        self.address_bus_mut().set_dot_limit(u64::MAX);
        self.finish_running();
        self.address_bus_mut().take_audio_samples(left, right);
    }

    /// Runs at most `max_steps` steps and stops early once the audio buffer is
    /// full. Returns whether the buffer is full.
    ///
    /// This lets a caller do other work while an audio buffer is produced, for
    /// example check a time limit.
    pub fn step_toward_next_audio_buffer(&mut self, max_steps: usize) -> bool {
        if self.address_bus().audio_buffers_full() {
            self.address_bus_mut().clear_audio_buffers();
        }

        for _ in 0..max_steps {
            self.cpu.step();
            if self.address_bus().audio_buffers_full() {
                self.finish_running();
                return true;
            }
        }
        self.finish_running();
        false
    }

    /// Ends a call that ran the CPU: runs the M-cycles still waiting, so the
    /// hardware is up to date, and passes battery-backed RAM the game wrote
    /// on to the frontend to save.
    fn finish_running(&mut self) {
        let address_bus = self.address_bus_mut();
        address_bus.run_deferred_cycles();
        address_bus.report_save_changes();
    }

    /// The most recent complete frame: 160x144 pixels, four bytes (RGBA) each.
    pub fn frame_buffer(&self) -> &[u8] {
        self.address_bus().completed_frame()
    }

    /// Returns the latest frame if a new one was finished since the last call.
    pub fn take_frame(&mut self) -> Option<&[u8]> {
        if !self.address_bus_mut().take_frame_ready() {
            return None;
        }
        Some(self.frame_buffer())
    }

    /// Presses a Game Boy button from the keyboard.
    pub fn press_key(&mut self, key: Key) {
        self.address_bus_mut().press_key(key);
    }

    /// Releases a Game Boy button from the keyboard.
    pub fn release_key(&mut self, key: Key) {
        self.address_bus_mut().release_key(key);
    }

    /// Updates which controller buttons are held and where the stick points.
    pub fn set_controller_state(&mut self, state: &ControllerState) {
        self.address_bus_mut().set_controller_state(state);
    }

    /// Chooses which controller buttons act as which Game Boy buttons.
    pub fn set_controller_mapping(&mut self, mapping: ControllerMapping) {
        self.address_bus_mut().set_controller_mapping(mapping);
    }

    /// Turns on a GameShark cheat, which makes the game read a
    /// different value from a byte of cartridge or work RAM.
    /// `cheat_id` is used to turn it off again.
    pub fn register_gameshark_cheat(
        &mut self,
        cheat_id: &str,
        code: &str,
    ) -> Result<(), CheatError> {
        self.address_bus_mut()
            .cheats_mut()
            .register_gameshark_cheat(cheat_id, code)
    }

    /// Turns on a Game Genie cheat, which makes the game read a
    /// different value from a byte of its ROM.
    /// `cheat_id` is used to turn it off again.
    pub fn register_gamegenie_cheat(
        &mut self,
        cheat_id: &str,
        code: &str,
    ) -> Result<(), CheatError> {
        self.address_bus_mut()
            .cheats_mut()
            .register_gamegenie_cheat(cheat_id, code)
    }

    /// Turns off the cheat that was registered with `cheat_id`.
    pub fn unregister_cheat(&mut self, cheat_id: &str) {
        self.address_bus_mut().cheats_mut().unregister(cheat_id);
    }

    /// Overwrites all CPU registers.
    pub fn set_registers(&mut self, registers: Registers) {
        *self.cpu.registers_mut() = registers;
    }

    /// The current CPU registers.
    pub fn registers(&self) -> &Registers {
        self.cpu.registers()
    }

    /// Reads a byte the way the CPU would see it right now, without side
    /// effects such as acknowledging a register read. Test runners use this
    /// to read results that test ROMs leave in memory. It takes `&mut self`
    /// because the PPU runs behind until something looks at it (see
    /// "Lazy components" in AGENTS.md): it is caught up first, so registers
    /// such as LY and STAT read right. Catching up early changes nothing
    /// else.
    pub fn read_memory(&mut self, address: u16) -> u8 {
        self.address_bus_mut().catch_up_ppu();
        self.address_bus().peek(address)
    }

    /// Whether the boot ROM is still running, before the game starts at 0x0100.
    pub fn is_running_boot_rom(&self) -> bool {
        self.address_bus().in_boot_rom()
    }

    /// Whether the CPU ran one of the opcodes the Game Boy doesn't define and
    /// has stopped for good, as on real hardware. The rest of the console
    /// keeps running. Some test ROMs end this way.
    pub fn is_locked_up(&self) -> bool {
        self.cpu.is_locked_up()
    }

    /// Writes a byte to memory in an emulator made with [`Emulator::for_cpu_tests`].
    pub fn write_test_memory(&mut self, address: u16, value: u8) {
        self.address_bus_mut().processor_test_ram_mut()[usize::from(address)] = value;
    }

    /// Reads a byte from memory in an emulator made with [`Emulator::for_cpu_tests`].
    pub fn read_test_memory(&self, address: u16) -> u8 {
        self.address_bus().processor_test_ram()[usize::from(address)]
    }

    /// The memory reads and writes of the last instruction, one entry per
    /// machine cycle. Cycles without a memory access are `None`. CPU test
    /// suites compare this with the expected bus activity.
    pub fn bus_activity(&self) -> &[Option<BusActivityEntry>] {
        self.cpu.opcode_bus_activity()
    }
}

mod save_state;
pub use save_state::{BUILD_COMMIT, SaveStateHeader};
#[cfg(test)]
mod tests;
