//! The public entry point of the emulator core.
//!
//! [`Emulator`] owns the CPU, which owns the address bus and through it the
//! rest of the hardware, and offers everything frontends can do with them.

use crate::address_bus::constants::HEADER_END_ADDRESS;
use crate::address_bus::{
    AddressBus, CartridgeEffects, CartridgeHeader, MemoryMut, RtcClock, RtcState, read_header,
};
use crate::controller::{ControllerMapping, ControllerState};
use crate::cpu::{BusActivityEntry, Cpu, Registers};
use crate::error::{CheatError, LoadRomError};
use crate::joypad::Key;
use crate::model::Model;
use crate::rom_archive;
use crate::sgb::{Sgb, SgbBorder, SgbBorderTime};
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

/// The width of the Game Boy screen ([`Emulator::frame_buffer`]), in pixels.
pub const SCREEN_WIDTH: u32 = crate::gpu::GB_SCREEN_WIDTH;
/// The height of the Game Boy screen, in pixels.
pub const SCREEN_HEIGHT: u32 = crate::gpu::GB_SCREEN_HEIGHT;
/// The width of a Super Game Boy's picture with its border
/// ([`Emulator::frame_with_border`]), in pixels.
pub const SGB_FRAME_WIDTH: u32 = crate::sgb::BORDERED_WIDTH as u32;
/// The height of a Super Game Boy's picture with its border, in pixels.
pub const SGB_FRAME_HEIGHT: u32 = crate::sgb::BORDERED_HEIGHT as u32;
/// Where the Game Boy screen's top-left corner is in a Super Game Boy's
/// picture with its border, in pixels from the left and from the top.
pub const SGB_SCREEN_POSITION: (u32, u32) = (
    crate::sgb::SCREEN_OFFSET.0 as u32,
    crate::sgb::SCREEN_OFFSET.1 as u32,
);

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
    /// What loading a save state starts from, kept after the first load
    /// (see `save_state.rs`).
    power_on_state: PowerOnState,
}

impl Emulator {
    /// Creates an emulator of `model` with no game loaded, ready to run that
    /// model's boot ROM, the startup program that shows the logo.
    pub fn new(model: Model) -> Self {
        let mut emulator = Emulator {
            cpu: Cpu::new(AddressBus::new(false)),
            power_on_state: PowerOnState::default(),
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
            power_on_state: PowerOnState::default(),
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
        self.power_on_state = PowerOnState::default();
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
            power_on_state: PowerOnState::default(),
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
    /// [`Emulator::load_rom`] would, except that a zip archive damaged past
    /// the header fails only when the game loads: only the header's bytes
    /// are unpacked.
    pub fn read_rom_header(rom: &[u8]) -> Result<CartridgeHeader, LoadRomError> {
        let header_bytes = HEADER_END_ADDRESS as u64 + 1;
        read_header(&rom_archive::extract_rom_borrowed(rom, Some(header_bytes))?)
    }

    /// Replaces the contents of the cartridge RAM, for example with a save file.
    pub fn set_cartridge_ram(&mut self, ram: &[u8]) {
        self.address_bus_mut().set_cartridge_ram(ram.to_vec());
    }

    /// The contents of the cartridge RAM, which holds the game's save data.
    pub fn cartridge_ram(&self) -> &[u8] {
        self.address_bus().cartridge_ram()
    }

    /// The state of the cartridge's real-time clock (the clock chip on some
    /// MBC3 cartridges), or `None` for a cartridge without one. Its
    /// `base_timestamp` is given as a time on `clock`, converted from the
    /// clock the cartridge follows ([`CartridgeEffects::rtc_clock`]) if that
    /// is the other one, so a frontend can keep it in one layout whichever
    /// clock the game ran on.
    pub fn rtc_state(&self, clock: RtcClock) -> Option<RtcState> {
        let address_bus = self.address_bus();
        address_bus
            .cartridge()
            .rtc_state(clock, address_bus.elapsed_dots_including_waiting())
    }

    /// Replaces the state of the cartridge's real-time clock, for example
    /// with one a frontend kept, as if the cartridge had been inserted with
    /// it: reads show it until the game latches the clock again. `state`'s
    /// `base_timestamp` is a time on `clock`, which is converted to the
    /// clock the cartridge follows if that is the other one. Does nothing
    /// for a cartridge without a clock.
    pub fn set_rtc_state(&mut self, state: &RtcState, clock: RtcClock) {
        let elapsed_dots = self.address_bus().elapsed_dots_including_waiting();
        self.address_bus_mut()
            .cartridge_mut()
            .set_rtc_state(state.clone(), clock, elapsed_dots);
    }

    /// The share of the emulated time since the last call that the rumble
    /// motor of an MBC5 rumble cartridge was on, from 0.0 (off, and always
    /// for other cartridges) to 1.0 (on all the time). Games switch the motor
    /// on and off many times a frame to rumble less strongly, so a frontend
    /// that calls this once a frame can pass it on as the rumble's strength.
    pub fn take_rumble_level(&mut self) -> f32 {
        let elapsed_dots = self.address_bus().elapsed_dots_including_waiting();
        self.address_bus_mut()
            .cartridge_mut()
            .take_rumble_level(elapsed_dots)
    }

    /// All of work RAM (0xC000-0xDFFF): bank 0 followed by the banks
    /// 0xD000 can show, 4 KiB each. A CGB has banks 1-7; on the DMG, and on
    /// a CGB running a game for the DMG, only bank 1 is used. Frontends read
    /// it for features such as RetroAchievements, and change it with
    /// [`Emulator::memory_mut`].
    pub fn work_ram(&self) -> &[u8] {
        self.address_bus().working_ram()
    }

    /// High RAM (0xFF80-0xFFFE), the 127 bytes the CPU can always reach.
    pub fn high_ram(&self) -> &[u8] {
        self.address_bus().high_ram()
    }

    /// Video RAM (0x8000-0x9FFF; bank 0 followed by bank 1, which only a CGB
    /// uses) and OAM (0xFE00-0xFE9F, the objects' attributes), as stored,
    /// whether or not the CPU could read them right now. It takes `&mut self`
    /// because the PPU runs behind until something looks at it (see
    /// [`Emulator::read_memory`]).
    pub fn video_memory(&mut self) -> (&[u8], &[u8]) {
        self.address_bus_mut().video_memory()
    }

    /// Work RAM, high RAM, video RAM, OAM and the cartridge RAM, to change
    /// directly, as a cheat device or a debugger would: a frontend's memory
    /// editor writes here. The changes land between two machine cycles, at
    /// no particular point of the frame, without the timing or blocking a
    /// CPU write has, and the game sees them from then on. Everything that
    /// runs behind (see "Lazy components" in AGENTS.md) is caught up first,
    /// so the emulator goes on exactly as if they had been made there.
    pub fn memory_mut(&mut self) -> MemoryMut<'_> {
        self.address_bus_mut().memory_mut()
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
        self.start_running();
        self.cpu.step();
        self.finish_running();
    }

    /// Runs until a full buffer of audio has been produced and returns the
    /// left and right channel samples. Calling this in a loop and playing the
    /// audio keeps the emulator running at the right speed.
    pub fn step_until_next_audio_buffer(&mut self) -> (&[f32], &[f32]) {
        self.start_running();
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
        self.start_running();
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
    /// example check a time limit. The samples it makes stay only until the
    /// next call clears a full buffer ([`Emulator::audio_samples`] reads
    /// them), so it is for callers that don't play sound, such as test
    /// runners.
    pub fn step_toward_next_audio_buffer(&mut self, max_steps: usize) -> bool {
        self.start_running();
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

    /// The left and right samples of the audio buffer, once it is full:
    /// after [`Emulator::step_toward_next_audio_buffer`] returned `true`, the
    /// whole buffer it filled. This lets a test runner that steps in small
    /// chunks, to check a time limit, still read the sound. A buffer that
    /// isn't full gives `None`, since the APU only adds the samples of the
    /// M-cycles it hasn't run yet when it catches up.
    pub fn audio_samples(&self) -> Option<(&[f32], &[f32])> {
        self.address_bus()
            .audio_buffers_full()
            .then(|| self.address_bus().audio_samples())
    }

    /// Starts a call that runs the CPU: after a save state loaded, the keys
    /// held now take over from its buttons, as the next button change would
    /// make them (see [`Emulator::load_state`]).
    fn start_running(&mut self) {
        self.address_bus_mut().apply_held_keys_after_load();
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

    /// On the Super Game Boy models, the most recent complete frame as the
    /// TV shows it: 256x224 RGBA pixels, with the Game Boy screen
    /// ([`Emulator::frame_buffer`]) at [`SGB_SCREEN_POSITION`] and the
    /// border around it. `None` on other models.
    pub fn frame_with_border(&self) -> Option<&[u8]> {
        self.address_bus().completed_frame_with_border()
    }

    /// Whether the most recent frame's Game Boy screen uses only the four
    /// [`DMG_SHADES`](crate::DMG_SHADES), which frontends may show in other
    /// colors: true on the original Game Boy models, and on a Super Game Boy
    /// until the game sets colors of its own. Frontends that repaint a frame
    /// with a border repaint only its Game Boy screen.
    pub fn frame_in_dmg_shades(&self) -> bool {
        self.address_bus().completed_frame_in_dmg_shades()
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

    /// Updates which controller buttons are held and where the stick points,
    /// for a frontend with one controller (or all of them combined).
    pub fn set_controller_state(&mut self, state: &ControllerState) {
        self.set_controller_states(std::slice::from_ref(state));
    }

    /// Updates which buttons are held and where the sticks point on each
    /// controller, the first controller's first. They all play as the first
    /// player, until a Super Game Boy game asks for more players: then the
    /// first controller plays the first player, the second the second, and
    /// so on up to four. The keyboard keys ([`Emulator::press_key`]) always
    /// play the first player.
    pub fn set_controller_states(&mut self, states: &[ControllerState]) {
        self.address_bus_mut().set_controller_states(states);
    }

    /// Holds `keys` for the second, third or fourth `player` of a Super Game
    /// Boy's multitap (2 to 4; other numbers are ignored), for a frontend
    /// that reads them as players rather than as controllers, such as
    /// RetroArch's ports. A game only reads them once it asks for more than
    /// one player.
    pub fn set_player_keys(&mut self, player: u8, keys: &[Key]) {
        self.address_bus_mut().set_player_keys(player, keys);
    }

    /// Holds exactly `players`' keys, the first player's first, for a
    /// frontend that reads every player's buttons each frame, such as
    /// RetroArch: the first player's replace the keyboard keys
    /// ([`Emulator::press_key`]), the others' the ones
    /// [`Emulator::set_player_keys`] holds, and players left out hold
    /// nothing. The buttons change in one go, so whether the change requests
    /// the joypad interrupt doesn't depend on the order of the keys. Called
    /// before every frame, it makes a loaded save state go on exactly like
    /// the run it was saved from, given the same buttons.
    pub fn set_held_keys(&mut self, players: &[&[Key]]) {
        self.address_bus_mut().set_held_keys(players);
    }

    /// On the Super Game Boy models, the palette the player picked from the
    /// Super Game Boy's menu ([`Emulator::set_sgb_player_palette`]), while it
    /// colors the screen: four RGB555 colors, from the lightest shade to the
    /// darkest. `None` while the game's own colors show, and on other models.
    pub fn sgb_player_palette(&self) -> Option<[u16; 4]> {
        self.address_bus().sgb()?.player_palette()
    }

    /// On the Super Game Boy models, colors the whole screen with `palette`
    /// (four RGB555 colors, from the lightest shade to the darkest), as a
    /// palette picked from the Super Game Boy's menu does, or brings back
    /// the game's own colors for `None`. The palette stays until the player
    /// changes it, or, if the game asks for that, until the game sets new
    /// colors; picking one again gives it back. It doesn't show while the
    /// game takes the palettes away
    /// ([`Emulator::sgb_player_palette_allowed`]). The last frame shows it
    /// at once, so a paused game shows it too.
    ///
    /// Save states don't keep it: it is the player's, like the keys held,
    /// and stays as it is when a state loads. The loaded state shows it too,
    /// unless the game had taken its colors back by then, which states do
    /// keep. A new game, or this one started again, starts with the game's
    /// colors.
    pub fn set_sgb_player_palette(&mut self, palette: Option<[u16; 4]>) {
        self.address_bus_mut()
            .change_sgb_menu(|sgb| sgb.set_player_palette(palette));
    }

    /// Whether the game lets the player pick a palette from the Super Game
    /// Boy's menu. False on other models.
    pub fn sgb_player_palette_allowed(&self) -> bool {
        self.address_bus()
            .sgb()
            .is_some_and(Sgb::player_palettes_allowed)
    }

    /// On the Super Game Boy models, the border the player picked
    /// ([`Emulator::set_sgb_border`]). [`SgbBorder::Game`] on other models.
    pub fn sgb_border(&self) -> SgbBorder {
        self.address_bus()
            .sgb()
            .map_or(SgbBorder::Game, Sgb::border_choice)
    }

    /// On the Super Game Boy models, picks the border around the screen in
    /// [`Emulator::frame_with_border`]: the game's own, or Nubo's. The last
    /// frame shows it at once, so a paused game shows it too. Like the
    /// palette ([`Emulator::set_sgb_player_palette`]), it is the player's:
    /// save states don't keep it, and it stays as it is when a state loads.
    pub fn set_sgb_border(&mut self, border: SgbBorder) {
        self.address_bus_mut()
            .change_sgb_menu(|sgb| sgb.set_border_choice(border));
    }

    /// On the Super Game Boy models, whether Nubo's own border shows its
    /// night or its day look ([`Emulator::set_sgb_border_time`]).
    /// [`SgbBorderTime::Night`] on other models.
    pub fn sgb_border_time(&self) -> SgbBorderTime {
        self.address_bus()
            .sgb()
            .map_or(SgbBorderTime::Night, Sgb::border_time)
    }

    /// On the Super Game Boy models, shows Nubo's own border with its night
    /// sky or its day sky; frontends pick it from the player's clock. It
    /// starts at night. The last frame shows it at once, and like the border
    /// choice ([`Emulator::set_sgb_border`]) save states don't keep it.
    pub fn set_sgb_border_time(&mut self, time: SgbBorderTime) {
        self.address_bus_mut()
            .change_sgb_menu(|sgb| sgb.set_border_time(time));
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
    ///
    /// # Panics
    ///
    /// If the emulator wasn't made with [`Emulator::for_cpu_tests`].
    pub fn write_test_memory(&mut self, address: u16, value: u8) {
        self.address_bus_mut().processor_test_ram_mut()[usize::from(address)] = value;
    }

    /// Reads a byte from memory in an emulator made with [`Emulator::for_cpu_tests`].
    ///
    /// # Panics
    ///
    /// If the emulator wasn't made with [`Emulator::for_cpu_tests`].
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
use save_state::PowerOnState;
pub use save_state::{BUILD_COMMIT, SaveStateHeader};
#[cfg(test)]
mod tests;
