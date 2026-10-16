//! The libretro core without any `unsafe`: everything the core does, as a
//! plain Rust type that `ffi.rs` calls from RetroArch's C functions and tests
//! call directly.
//!
//! RetroArch calls [`Core::run`] once per frame of its screen and expects one
//! frame's worth of emulated time (`Emulator::run_frame`). It keeps battery
//! saves itself, as the cartridge RAM and clock buffers the core owns
//! ([`Core::save_ram`] and [`Core::rtc_memory`]), which the core keeps in
//! step with the emulator (see "Battery saves" below), and it keeps save
//! states as blocks of one fixed size ([`Core::serialize_size`]).
//!
//! # Battery saves
//!
//! RetroArch copies the `.srm` and `.rtc` files into the buffers after the
//! game is loaded, and reads them back whenever it saves. So at the start of
//! each [`Core::run`], a buffer that differs from what the core last copied
//! into it was changed by RetroArch and goes to the emulator; after the
//! frame, what the game saved goes to the buffers. Loading a save state and
//! restarting the game copy the emulator's RAM and clock to the buffers, since
//! neither tells the frontend to save. The `.srm` is the raw cartridge RAM,
//! like the Nubo app's `.sav`, and the `.rtc` is the app's 17-byte layout
//! (`RtcState::to_bytes`), always on the wall clock, whichever clock the game
//! follows: so saves move between the app and RetroArch.

use crate::options::Options;
use nubo_core::{
    CartridgeEffects, CartridgeHeader, DOTS_PER_FRAME, Emulator, Key, LoadRomError, Model,
    RtcClock, RtcState, SGB_FRAME_HEIGHT, SGB_FRAME_WIDTH, SgbBorder, extract_rom,
    validate_gamegenie_code,
};
use nubo_frontend::picture::Frame;
use nubo_frontend::sgb_menu::{BorderClock, DEFAULT_OWN_PALETTE};
use std::collections::BTreeMap;
use std::ffi::CStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// The width of the Game Boy's screen in pixels.
pub const SCREEN_WIDTH: usize = 160;
/// The height of the Game Boy's screen in pixels.
pub const SCREEN_HEIGHT: usize = 144;
/// The widest picture the core hands over: a Super Game Boy's, with its
/// border.
pub const MAX_WIDTH: usize = SGB_FRAME_WIDTH as usize;
/// The highest picture the core hands over.
pub const MAX_HEIGHT: usize = SGB_FRAME_HEIGHT as usize;
/// How many samples per second of each channel the core hands RetroArch.
/// Nubo makes exactly this many per emulated second, and RetroArch
/// resamples them for the sound card.
pub const SAMPLE_RATE: u32 = 48_000;
/// The Game Boy's clock: 4,194,304 ticks (PPU dots) per second.
const DOTS_PER_SECOND: f64 = 4_194_304.0;
/// Frames per second: 4,194,304 / 70,224, about 59.73.
pub const FRAMES_PER_SECOND: f64 = DOTS_PER_SECOND / DOTS_PER_FRAME as f64;

/// The bytes a save state may grow by after the first one of a game on its
/// longest model (see [`largest_state`]), so [`Core::serialize_size`] never
/// has to change. Only a few values change
/// the length of a state, and by little: enum variants are stored by name
/// (`Halted` is longer than `Running`), `Option`s with their value only when
/// there is one, and the PPU's pixel queues with the pixels they hold, which
/// together come to well under a kilobyte (the tests measure it).
const STATE_SIZE_MARGIN: usize = 16 * 1024;
/// The length of the number before each save state: its length in bytes.
const STATE_LENGTH_BYTES: usize = 4;

/// The Game Boy's buttons as libretro numbers them (`RETRO_DEVICE_ID_JOYPAD_*`),
/// with the button each one presses. Libretro's B and A are the bottom and
/// right face buttons, where the Game Boy has its B and A. The names are C
/// strings that live as long as the core, since RetroArch keeps pointing at
/// them for its controls menus. A static rather than a constant, so the
/// names have one address: each use of a constant may get its own copy of
/// them, and on Windows does.
pub static BUTTONS: [(u32, Key, &CStr); 8] = [
    (0, Key::B, c"B"),
    (2, Key::Select, c"Select"),
    (3, Key::Start, c"Start"),
    (4, Key::Up, c"D-Pad Up"),
    (5, Key::Down, c"D-Pad Down"),
    (6, Key::Left, c"D-Pad Left"),
    (7, Key::Right, c"D-Pad Right"),
    (8, Key::A, c"A"),
];

/// How a frame is handed to RetroArch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// A native-endian `u32` per pixel, `0x00RRGGBB` (`RETRO_PIXEL_FORMAT_XRGB8888`).
    Xrgb8888,
    /// A native-endian `u16` per pixel, five bits per color with the top bit
    /// unused (`RETRO_PIXEL_FORMAT_0RGB1555`): what a frontend that refuses
    /// XRGB8888 expects.
    Rgb1555,
}

impl PixelFormat {
    /// The bytes from the start of one line of a frame `width` pixels wide
    /// to the next.
    pub fn pitch(self, width: usize) -> usize {
        match self {
            PixelFormat::Xrgb8888 => width * 4,
            PixelFormat::Rgb1555 => width * 2,
        }
    }
}

/// A frame ready for RetroArch, in the [`PixelFormat`] it asked for.
#[derive(Debug, Clone, Copy)]
pub enum Video<'a> {
    /// [`PixelFormat::Xrgb8888`] pixels, line after line.
    Xrgb8888(&'a [u32]),
    /// [`PixelFormat::Rgb1555`] pixels, line after line.
    Rgb1555(&'a [u16]),
}

/// What the cartridge asked the core to save during a frame. [`CoreEffects`]
/// sets the flags when the emulator tells it the game saved, and
/// [`Core::run`] clears them.
#[derive(Debug, Default)]
struct SaveRequests {
    ram: AtomicBool,
    rtc: AtomicBool,
}

/// Connects the emulated cartridge to the core: it reads the wall clock,
/// follows the clock the `nubo_rtc` option picked, and notes what the game
/// saved. It loads nothing, since RetroArch hands the saves over after the
/// game is loaded.
struct CoreEffects {
    rtc_clock: RtcClock,
    requests: Arc<SaveRequests>,
}

impl CartridgeEffects for CoreEffects {
    fn current_time_millis(&self) -> f64 {
        wall_clock_millis()
    }

    fn rtc_clock(&self) -> RtcClock {
        self.rtc_clock
    }

    fn save_rtc_state(&mut self, _state: &RtcState) {
        self.requests.rtc.store(true, Ordering::Relaxed);
    }

    fn save_ram(&mut self, _ram: &[u8]) {
        self.requests.ram.store(true, Ordering::Relaxed);
    }
}

/// Milliseconds since the Unix epoch, the wall clock the cartridge clock and
/// the `.rtc` layout use.
fn wall_clock_millis() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as f64
}

/// One code of a RetroArch cheat, as the emulator has it on.
#[derive(Debug, Clone)]
struct Cheat {
    /// The id the emulator knows it by: the cheat's index and the code's
    /// place in it, such as `3-0`.
    id: String,
    code: String,
    /// Whether it is a Game Genie code, which changes ROM, rather than a
    /// GameShark code, which changes RAM.
    is_game_genie: bool,
}

impl Cheat {
    /// Turns the code on in `emulator`.
    fn register(&self, emulator: &mut Emulator) -> Result<(), nubo_core::CheatError> {
        if self.is_game_genie {
            emulator.register_gamegenie_cheat(&self.id, &self.code)
        } else {
            emulator.register_gameshark_cheat(&self.id, &self.code)
        }
    }
}

/// A cheat code, as RetroArch sent it, that is neither a Game Genie nor a
/// GameShark code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidCheat(pub String);

/// A loaded game and everything the core keeps for RetroArch while it runs.
pub struct Core {
    /// The game's ROM, unpacked from a zip archive if it came in one, to
    /// start it again on [`Core::reset`].
    rom: Vec<u8>,
    header: CartridgeHeader,
    emulator: Emulator,
    /// The options as the player has them now. The model and the clock take
    /// effect on the next [`Core::reset`].
    options: Options,
    requests: Arc<SaveRequests>,
    /// The cartridge RAM RetroArch reads and fills (`RETRO_MEMORY_SAVE_RAM`):
    /// as long as the RAM on a cartridge with a battery, empty otherwise. Its
    /// length never changes, so RetroArch's pointer to it stays valid.
    save_ram: Vec<u8>,
    /// What the core last copied into `save_ram`, to tell RetroArch's
    /// changes apart.
    save_ram_copy: Vec<u8>,
    /// The clock RetroArch reads and fills (`RETRO_MEMORY_RTC`), in the
    /// `.rtc` layout on the wall clock: [`RtcState::BYTES`] long on a
    /// cartridge with a clock and a battery, empty otherwise.
    rtc_memory: Vec<u8>,
    /// What the core last copied into `rtc_memory`.
    rtc_copy: Vec<u8>,
    pixel_format: PixelFormat,
    /// The latest frame as the emulator drew it.
    frame: Frame,
    /// The width and height of the picture handed over, which a Super Game
    /// Boy's border makes larger.
    video_size: (usize, usize),
    frame_xrgb8888: Vec<u32>,
    frame_rgb1555: Vec<u16>,
    left: Vec<f32>,
    right: Vec<f32>,
    /// The latest frame's sound, left and right samples in turn.
    audio: Vec<i16>,
    /// The libretro buttons held during the last frame, one bit each.
    held_buttons: u16,
    /// The libretro buttons the second to fourth players held during the
    /// last frame, one bit each (see [`Core::set_other_players`]).
    other_players_held: [u16; 3],
    /// The codes of each RetroArch cheat that are on, by its index.
    cheats: BTreeMap<u32, Vec<Cheat>>,
    /// The length every save state is padded to.
    state_size: usize,
    memory: MemorySnapshot,
    /// The rumble motor's strength during the latest frame, 0 to 0xFFFF.
    rumble: u16,
    /// Turns a Super Game Boy's border to its night or day look as the
    /// player's clock says.
    border_clock: BorderClock,
}

impl Core {
    /// Loads a game, a ROM file or a zip archive with one, on the hardware
    /// `options` picks for it.
    pub fn load(
        rom: &[u8],
        options: Options,
        pixel_format: PixelFormat,
    ) -> Result<Core, LoadRomError> {
        let rom = extract_rom(rom)?;
        let header = Emulator::read_rom_header(&rom)?;
        let requests = Arc::new(SaveRequests::default());
        let mut emulator = start(&rom, &header, options, &requests)?;
        let border_clock = BorderClock::start(&mut emulator);
        let mut core = Core {
            rom,
            header,
            emulator,
            options,
            requests,
            save_ram: Vec::new(),
            save_ram_copy: Vec::new(),
            rtc_memory: Vec::new(),
            rtc_copy: Vec::new(),
            pixel_format,
            frame: Frame::default(),
            video_size: (SCREEN_WIDTH, SCREEN_HEIGHT),
            frame_xrgb8888: Vec::with_capacity(MAX_WIDTH * MAX_HEIGHT),
            frame_rgb1555: Vec::with_capacity(MAX_WIDTH * MAX_HEIGHT),
            left: Vec::new(),
            right: Vec::new(),
            audio: Vec::new(),
            held_buttons: 0,
            other_players_held: [0; 3],
            cheats: BTreeMap::new(),
            state_size: 0,
            memory: MemorySnapshot::new(),
            rumble: 0,
            border_clock,
        };
        if core.header.has_battery {
            core.save_ram = core.emulator.cartridge_ram().to_vec();
            if let Some(clock) = core.emulator.rtc_state(RtcClock::WallClock) {
                core.rtc_memory = clock.to_bytes().to_vec();
            }
        }
        core.save_ram_copy.clone_from(&core.save_ram);
        core.rtc_copy.clone_from(&core.rtc_memory);
        let longest = largest_state(&core.rom, options).max(core.emulator.save_state().len());
        core.state_size = STATE_LENGTH_BYTES + longest + STATE_SIZE_MARGIN;
        core.draw_frame();
        core.memory.update(&mut core.emulator);
        Ok(core)
    }

    /// The cartridge header of the game.
    pub fn header(&self) -> &CartridgeHeader {
        &self.header
    }

    /// The hardware the game runs on.
    pub fn model(&self) -> Model {
        self.emulator.model()
    }

    /// The emulator, for tests that look inside.
    #[cfg(test)]
    fn emulator(&mut self) -> &mut Emulator {
        &mut self.emulator
    }

    /// Takes on the options the player set. The palettes and the border
    /// change right away; the model and the clock when the game starts
    /// again.
    pub fn set_options(&mut self, options: Options) {
        let colors_changed = options.sgb_colors != self.options.sgb_colors;
        let border_changed = options.nubo_border != self.options.nubo_border;
        let picture_changed = colors_changed
            || border_changed
            || options.palette != self.options.palette
            || options.sgb_border != self.options.sgb_border;
        self.options = options;
        // Only the choice that changed goes to the Super Game Boy, so a new
        // border doesn't bring back a palette the game has taken back.
        if colors_changed {
            apply_sgb_colors(&mut self.emulator, options);
        }
        if border_changed {
            apply_sgb_border(&mut self.emulator, options);
        }
        if picture_changed {
            self.draw_frame();
        }
    }

    /// Runs one frame's worth of time with `buttons` held (one bit per
    /// libretro button number, see [`BUTTONS`]), then leaves the frame in
    /// [`Core::video`], the sound in [`Core::audio`] and the rumble in
    /// [`Core::rumble`].
    pub fn run(&mut self, buttons: u16) {
        self.take_frontend_changes();
        self.press_buttons(buttons);

        self.left.clear();
        self.right.clear();
        self.emulator.run_frame(&mut self.left, &mut self.right);
        self.emulator.take_frame();
        self.border_clock.frame_finished(&mut self.emulator);

        self.pass_on_game_saves();
        self.draw_frame();
        self.mix_audio();
        self.memory.update(&mut self.emulator);
        self.rumble = (self.emulator.take_rumble_level() * f32::from(u16::MAX)).round() as u16;
    }

    /// Starts the game again, as if the console had been switched off and
    /// on, on the model the options pick now and with the clock they pick.
    /// The cartridge keeps what its battery keeps: its RAM and its clock.
    pub fn reset(&mut self) -> Result<(), LoadRomError> {
        self.take_frontend_changes();
        let clock = self.emulator.rtc_state(RtcClock::WallClock);
        let mut emulator = start(&self.rom, &self.header, self.options, &self.requests)?;
        self.border_clock = BorderClock::start(&mut emulator);
        if self.header.has_battery {
            emulator.set_cartridge_ram(self.emulator.cartridge_ram());
            if let Some(clock) = clock {
                emulator.set_rtc_state(&clock, RtcClock::WallClock);
            }
        }
        self.emulator = emulator;
        self.reregister_cheats();
        self.held_buttons = 0;
        self.other_players_held = [0; 3];
        self.sync_saves_from_emulator();
        self.draw_frame();
        self.memory.update(&mut self.emulator);
        Ok(())
    }

    /// Takes everything RetroArch changed since the core last looked: the
    /// battery saves first (a loaded `.srm`), then its memory writes, so a
    /// write to cartridge RAM lands on the loaded save rather than the save
    /// being overwritten with what the emulator held before.
    fn take_frontend_changes(&mut self) {
        self.take_frontend_saves();
        self.take_frontend_memory_writes();
    }

    /// Writes what RetroArch wrote to the memory it can reach (its memory
    /// editor, the cheats its memory search makes, `WRITE_CORE_MEMORY`)
    /// into the emulator. A change to battery-backed cartridge RAM goes to
    /// the save RAM buffer too, so RetroArch's `.srm` keeps it.
    fn take_frontend_memory_writes(&mut self) {
        if self.memory.apply(&mut self.emulator) && !self.save_ram.is_empty() {
            copy_into(&mut self.save_ram, self.emulator.cartridge_ram());
            self.save_ram_copy.copy_from_slice(&self.save_ram);
        }
    }

    /// Copies the battery saves RetroArch changed since the last frame to
    /// the emulator.
    fn take_frontend_saves(&mut self) {
        if self.save_ram != self.save_ram_copy {
            self.emulator.set_cartridge_ram(&self.save_ram);
            self.save_ram_copy.copy_from_slice(&self.save_ram);
        }
        if self.rtc_memory != self.rtc_copy {
            if let Some(clock) = RtcState::from_bytes(&self.rtc_memory) {
                self.emulator.set_rtc_state(&clock, RtcClock::WallClock);
            }
            self.rtc_copy.copy_from_slice(&self.rtc_memory);
        }
    }

    /// Copies what the game saved during the frame to RetroArch's buffers.
    fn pass_on_game_saves(&mut self) {
        if self.requests.ram.swap(false, Ordering::Relaxed) && !self.save_ram.is_empty() {
            copy_into(&mut self.save_ram, self.emulator.cartridge_ram());
            self.save_ram_copy.copy_from_slice(&self.save_ram);
        }
        if self.requests.rtc.swap(false, Ordering::Relaxed) && !self.rtc_memory.is_empty() {
            self.copy_clock_to_buffers();
        }
    }

    /// Copies the emulator's cartridge RAM and clock to RetroArch's buffers,
    /// after something replaced them without the game saving: a loaded save
    /// state or a restart.
    fn sync_saves_from_emulator(&mut self) {
        self.requests.ram.store(false, Ordering::Relaxed);
        self.requests.rtc.store(false, Ordering::Relaxed);
        if !self.save_ram.is_empty() {
            copy_into(&mut self.save_ram, self.emulator.cartridge_ram());
            self.save_ram_copy.copy_from_slice(&self.save_ram);
        }
        if !self.rtc_memory.is_empty() {
            self.copy_clock_to_buffers();
        }
    }

    /// Copies the emulator's clock, on the wall clock, into `rtc_memory`
    /// (RetroArch's `.rtc` buffer) and `rtc_copy`.
    fn copy_clock_to_buffers(&mut self) {
        if let Some(clock) = self.emulator.rtc_state(RtcClock::WallClock) {
            self.rtc_memory.copy_from_slice(&clock.to_bytes());
            self.rtc_copy.copy_from_slice(&self.rtc_memory);
        }
    }

    /// Holds the buttons of the second to fourth players, RetroArch's ports
    /// 2 to 4 (one bit per libretro button number, see [`BUTTONS`]). A Super
    /// Game Boy game reads them once it asks for more than one player;
    /// otherwise they do nothing.
    pub fn set_other_players(&mut self, buttons: [u16; 3]) {
        for (player, (&held, before)) in (2..).zip(buttons.iter().zip(&mut self.other_players_held))
        {
            if held == *before {
                continue;
            }
            let keys: Vec<Key> = BUTTONS
                .iter()
                .filter(|(id, _, _)| held & 1 << id != 0)
                .map(|&(_, key, _)| key)
                .collect();
            self.emulator.set_player_keys(player, &keys);
            *before = held;
        }
    }

    /// The buttons last handed over for the second to fourth players.
    #[cfg(test)]
    pub fn other_players_held_for_tests(&self) -> [u16; 3] {
        self.other_players_held
    }

    /// Presses and releases the Game Boy buttons whose libretro buttons
    /// changed since the last frame.
    fn press_buttons(&mut self, buttons: u16) {
        let changed = buttons ^ self.held_buttons;
        for (id, key, _) in BUTTONS {
            let bit = 1 << id;
            if changed & bit == 0 {
                continue;
            }
            if buttons & bit == 0 {
                self.emulator.release_key(key);
            } else {
                self.emulator.press_key(key);
            }
        }
        self.held_buttons = buttons;
    }

    /// Converts the latest frame to RetroArch's pixel format, in the
    /// palette the player picked on a Game Boy, with a Super Game Boy's
    /// border unless the options hide it.
    fn draw_frame(&mut self) {
        self.frame.update(&self.emulator);
        let picture = self
            .frame
            .shown(self.options.palette, self.options.sgb_border);
        self.video_size = (picture.width() as usize, picture.height() as usize);
        let pixels = picture.pixels().as_chunks::<4>().0.iter();
        match self.pixel_format {
            PixelFormat::Xrgb8888 => {
                self.frame_xrgb8888.clear();
                self.frame_xrgb8888.extend(
                    pixels.map(|&[red, green, blue, _]| u32::from_be_bytes([0, red, green, blue])),
                );
            }
            PixelFormat::Rgb1555 => {
                self.frame_rgb1555.clear();
                self.frame_rgb1555
                    .extend(pixels.map(|&[red, green, blue, _]| {
                        (u16::from(red >> 3) << 10)
                            | (u16::from(green >> 3) << 5)
                            | u16::from(blue >> 3)
                    }));
            }
        }
    }

    /// Interleaves the frame's samples as 16-bit numbers, as RetroArch takes
    /// them.
    fn mix_audio(&mut self) {
        self.audio.clear();
        for (&left, &right) in self.left.iter().zip(&self.right) {
            self.audio.push(to_i16(left));
            self.audio.push(to_i16(right));
        }
    }

    /// The width and height of [`Core::video`]: [`SCREEN_WIDTH`] ×
    /// [`SCREEN_HEIGHT`], or [`MAX_WIDTH`] × [`MAX_HEIGHT`] for a Super Game
    /// Boy's picture with its border.
    pub fn video_size(&self) -> (usize, usize) {
        self.video_size
    }

    /// The bytes from the start of one line of [`Core::video`] to the next.
    pub fn pitch(&self) -> usize {
        self.pixel_format.pitch(self.video_size.0)
    }

    /// The latest frame, [`Core::video_size`] pixels.
    pub fn video(&self) -> Video<'_> {
        match self.pixel_format {
            PixelFormat::Xrgb8888 => Video::Xrgb8888(&self.frame_xrgb8888),
            PixelFormat::Rgb1555 => Video::Rgb1555(&self.frame_rgb1555),
        }
    }

    /// The pixel format of [`Core::video`].
    pub fn pixel_format(&self) -> PixelFormat {
        self.pixel_format
    }

    /// The latest frame's sound: left and right samples in turn.
    pub fn audio(&self) -> &[i16] {
        &self.audio
    }

    /// How strongly the rumble motor ran during the latest frame, from 0
    /// (off, and always on cartridges without one) to 0xFFFF.
    pub fn rumble(&self) -> u16 {
        self.rumble
    }

    /// Whether the cartridge has a rumble motor (MBC5 types 0x1C-0x1E).
    pub fn has_rumble(&self) -> bool {
        matches!(self.header.cartridge_type, 0x1C..=0x1E)
    }

    /// The cartridge RAM buffer RetroArch keeps as the `.srm` file. Empty for
    /// a cartridge without a battery, so no file is written.
    pub fn save_ram(&mut self) -> &mut [u8] {
        &mut self.save_ram
    }

    /// The clock buffer RetroArch keeps as the `.rtc` file. Empty for a
    /// cartridge without a clock and a battery.
    pub fn rtc_memory(&mut self) -> &mut [u8] {
        &mut self.rtc_memory
    }

    /// The length of every save state the core makes, which stays the same
    /// for as long as the game is loaded, as run-ahead and netplay expect.
    pub fn serialize_size(&self) -> usize {
        self.state_size
    }

    /// Writes a save state into `buffer`: its length as a 4-byte
    /// little-endian number, the state, and zeros up to the end. Returns
    /// false, leaving `buffer` alone, if it doesn't fit. The battery saves
    /// and memory RetroArch changed go to the emulator first, so the state
    /// holds them.
    pub fn serialize(&mut self, buffer: &mut [u8]) -> bool {
        self.take_frontend_changes();
        let state = self.emulator.save_state();
        let end = STATE_LENGTH_BYTES + state.len();
        let Ok(length) = u32::try_from(state.len()) else {
            return false;
        };
        if end > buffer.len() {
            return false;
        }
        buffer[..STATE_LENGTH_BYTES].copy_from_slice(&length.to_le_bytes());
        buffer[STATE_LENGTH_BYTES..end].copy_from_slice(&state);
        buffer[end..].fill(0);
        true
    }

    /// Loads a save state written by [`Core::serialize`]. Returns false,
    /// leaving the game as it was, if it isn't one or is for another game or
    /// model.
    pub fn unserialize(&mut self, data: &[u8]) -> bool {
        let Some((length, rest)) = data.split_first_chunk::<STATE_LENGTH_BYTES>() else {
            return false;
        };
        let Some(state) = usize::try_from(u32::from_le_bytes(*length))
            .ok()
            .and_then(|length| rest.get(..length))
        else {
            return false;
        };
        if self.emulator.load_state(state).is_err() {
            return false;
        }
        self.sync_saves_from_emulator();
        self.draw_frame();
        self.memory.update(&mut self.emulator);
        true
    }

    /// Turns every cheat off.
    pub fn cheat_reset(&mut self) {
        for cheat in std::mem::take(&mut self.cheats).into_values().flatten() {
            self.emulator.unregister_cheat(&cheat.id);
        }
    }

    /// Sets RetroArch's cheat number `index` to `code`, several codes
    /// joined with `+`, each a Game Genie or a GameShark code. The codes the
    /// index had before are turned off first, and none are turned on if
    /// `enabled` is false. Returns the codes that aren't valid, which are
    /// skipped.
    pub fn cheat_set(&mut self, index: u32, enabled: bool, code: &str) -> Vec<InvalidCheat> {
        for cheat in self.cheats.remove(&index).unwrap_or_default() {
            self.emulator.unregister_cheat(&cheat.id);
        }
        if !enabled {
            return Vec::new();
        }
        let mut cheats = Vec::new();
        let mut invalid = Vec::new();
        for code in code
            .split('+')
            .map(str::trim)
            .filter(|code| !code.is_empty())
        {
            let cheat = Cheat {
                id: format!("{index}-{}", cheats.len()),
                code: String::from(code),
                is_game_genie: validate_gamegenie_code(code).is_ok(),
            };
            match cheat.register(&mut self.emulator) {
                Ok(()) => cheats.push(cheat),
                Err(_) => invalid.push(InvalidCheat(String::from(code))),
            }
        }
        if !cheats.is_empty() {
            self.cheats.insert(index, cheats);
        }
        invalid
    }

    /// Turns the cheats that are on, on in a new emulator too.
    fn reregister_cheats(&mut self) {
        for cheat in self.cheats.values().flatten() {
            // They were accepted before, by the same validators.
            let _ = cheat.register(&mut self.emulator);
        }
    }

    /// The memory RetroAchievements and RetroArch's memory tools read, as
    /// copies the core refreshes after every frame, laid out the way
    /// rcheevos expects a Game Boy (Color).
    pub fn memory_regions(&mut self) -> Vec<MemoryRegion<'_>> {
        let is_cgb = self.emulator.model().is_cgb();
        self.memory.regions(&self.rom, is_cgb)
    }

    /// Work RAM as copied after the latest frame, for
    /// `RETRO_MEMORY_SYSTEM_RAM`: 8 KiB on a Game Boy, 32 KiB on a Game Boy
    /// Color.
    pub fn system_ram(&mut self) -> &mut [u8] {
        let length = if self.emulator.model().is_cgb() {
            WORK_RAM_SIZE
        } else {
            DMG_WORK_RAM_SIZE
        };
        &mut self.memory.work_ram[..length]
    }

    /// Video RAM as copied after the latest frame, for
    /// `RETRO_MEMORY_VIDEO_RAM`: 8 KiB on a Game Boy, 16 KiB on a Game Boy
    /// Color.
    pub fn video_ram(&mut self) -> &mut [u8] {
        let length = if self.emulator.model().is_cgb() {
            VIDEO_RAM_SIZE
        } else {
            VIDEO_RAM_SIZE / 2
        };
        &mut self.memory.video_ram[..length]
    }
}

/// A new emulator for `rom` on the model `options` picks, with the clock it
/// picks, run to nowhere yet.
fn start(
    rom: &[u8],
    header: &CartridgeHeader,
    options: Options,
    requests: &Arc<SaveRequests>,
) -> Result<Emulator, LoadRomError> {
    let mut emulator = Emulator::new(options.model.model_for(header));
    emulator.set_sample_rate(SAMPLE_RATE);
    emulator.load_rom(
        rom,
        Box::new(CoreEffects {
            rtc_clock: options.rtc_clock,
            requests: requests.clone(),
        }),
    )?;
    apply_sgb_colors(&mut emulator, options);
    apply_sgb_border(&mut emulator, options);
    Ok(emulator)
}

/// The length of the longest save state `rom` makes just after it starts,
/// on any model. A restart takes on the model the options pick then (see
/// [`Core::reset`]), and a Super Game Boy's state is tens of kilobytes
/// longer than a Game Boy's, so [`Core::serialize_size`] must hold every
/// model's from the start.
fn largest_state(rom: &[u8], options: Options) -> usize {
    Model::ALL
        .iter()
        .filter_map(|&model| {
            let mut emulator = Emulator::new(model);
            emulator.set_sample_rate(SAMPLE_RATE);
            let effects = CoreEffects {
                rtc_clock: options.rtc_clock,
                requests: Arc::new(SaveRequests::default()),
            };
            emulator.load_rom(rom, Box::new(effects)).ok()?;
            Some(emulator.save_state().len())
        })
        .max()
        .unwrap_or(0)
}

/// Gives a Super Game Boy the colors `options` pick, as its menu would.
/// Other models ignore them. Save states don't keep them (they are the
/// player's), so they stay through loads, rewind and run-ahead.
fn apply_sgb_colors(emulator: &mut Emulator, options: Options) {
    emulator.set_sgb_player_palette(options.sgb_colors.palette(DEFAULT_OWN_PALETTE));
}

/// Gives a Super Game Boy the border `options` pick, as its menu would.
/// Other models ignore it; like the colors, it stays through loads.
fn apply_sgb_border(emulator: &mut Emulator, options: Options) {
    emulator.set_sgb_border(if options.nubo_border {
        SgbBorder::Nubo
    } else {
        SgbBorder::Game
    });
}

/// Copies as much of `from` into `to` as fits, leaving the rest of `to`.
fn copy_into(to: &mut [u8], from: &[u8]) {
    let length = to.len().min(from.len());
    to[..length].copy_from_slice(&from[..length]);
}

/// A sample from -1.0 to 1.0 as a 16-bit number, clamped to its range.
fn to_i16(sample: f32) -> i16 {
    (sample * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16
}

/// The Game Boy Color's work RAM: eight banks of 4 KiB.
const WORK_RAM_SIZE: usize = 0x8000;
/// The Game Boy's work RAM: two banks of 4 KiB.
const DMG_WORK_RAM_SIZE: usize = 0x2000;
/// The Game Boy Color's video RAM: two banks of 8 KiB.
const VIDEO_RAM_SIZE: usize = 0x4000;
/// The most cartridge RAM a cartridge has: 16 banks of 8 KiB.
const CARTRIDGE_RAM_SIZE: usize = 0x2_0000;
/// The addresses rcheevos reads stay below this (0x34000), so the
/// descriptors' `select` masks only look at these bits.
const ADDRESS_SPACE_MASK: usize = 0x3_FFFF;

/// The part of a memory region RetroAchievements reads, with what libretro's
/// memory descriptors say about it.
#[derive(Debug)]
pub struct MemoryRegion<'a> {
    /// The address rcheevos reads it at: a Game Boy address, or one above
    /// 0xFFFF for the banks the Game Boy can't show at once.
    pub start: usize,
    /// The region's bytes, at least `length` of them, which stay at one
    /// address for as long as the game is loaded.
    pub buffer: &'a mut [u8],
    /// The region's length, a power of two that `start` is a multiple of.
    pub length: usize,
    /// What kind of memory it is: [`MemoryKind`].
    pub kind: MemoryKind,
}

impl MemoryRegion<'_> {
    /// The address bits that pick this region (libretro's `select`): every
    /// bit of the address space above the region's own offsets.
    pub fn select(&self) -> usize {
        ADDRESS_SPACE_MASK & !(self.length - 1)
    }
}

/// What kind of memory a [`MemoryRegion`] is, which libretro's descriptor
/// flags tell RetroArch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryKind {
    /// ROM, which never changes (`RETRO_MEMDESC_CONST`).
    Rom,
    /// Work RAM (`RETRO_MEMDESC_SYSTEM_RAM`).
    SystemRam,
    /// Cartridge RAM (`RETRO_MEMDESC_SAVE_RAM`).
    SaveRam,
    /// Video RAM and OAM (`RETRO_MEMDESC_VIDEO_RAM`).
    VideoRam,
    /// The hardware registers, which RetroArch may only read
    /// (`RETRO_MEMDESC_CONST`).
    Registers,
    /// High RAM, which has no flag.
    Other,
}

/// Copies of the Game Boy's memory, refreshed after every frame, which
/// RetroAchievements reads and RetroArch's memory tools (its memory editor,
/// the cheats its memory search makes, `WRITE_CORE_MEMORY`) read and write.
/// Copies keep pointers into the emulator out of RetroArch's hands. Each
/// buffer is boxed, so it stays at one address.
///
/// What RetroArch writes reaches the game through [`MemorySnapshot::apply`]
/// before the next frame: `written` holds what the core last copied into
/// the RAM buffers, so a byte that differs from it was written by RetroArch.
/// ROM and the hardware registers are read-only (`RETRO_MEMDESC_CONST` for
/// the registers too), and so is IE, the last byte of `high_ram`.
struct MemorySnapshot {
    /// ROM banks 0 and 1, as 0x0000-0x7FFF shows them at power-on.
    rom: Box<[u8; 0x8000]>,
    video_ram: Box<[u8; VIDEO_RAM_SIZE]>,
    /// Cartridge RAM, all banks, with zeros after the cartridge's own; RAM
    /// shorter than a bank repeats through the first (`cartridge_ram_index`).
    cartridge_ram: Box<[u8; CARTRIDGE_RAM_SIZE]>,
    work_ram: Box<[u8; WORK_RAM_SIZE]>,
    /// OAM at 0xFE00-0xFE9F and the unusable area after it, which reads 0.
    oam: Box<[u8; 0x100]>,
    /// The hardware registers at 0xFF00-0xFF7F.
    io: Box<[u8; 0x80]>,
    /// High RAM at 0xFF80-0xFFFE, then IE at 0xFFFF.
    high_ram: Box<[u8; 0x80]>,
    /// What the core last copied into the buffers RetroArch may write.
    written: Box<WrittenMemory>,
    /// Whether `rom` was filled.
    has_rom: bool,
}

/// What [`MemorySnapshot`] last copied into the buffers RetroArch may write,
/// to tell its writes apart.
struct WrittenMemory {
    video_ram: [u8; VIDEO_RAM_SIZE],
    cartridge_ram: [u8; CARTRIDGE_RAM_SIZE],
    work_ram: [u8; WORK_RAM_SIZE],
    oam: [u8; 0x100],
    high_ram: [u8; 0x80],
}

/// Where byte `index` of the cartridge RAM buffer is in a cartridge RAM
/// `length` bytes long. RAM shorter than one 8 KiB bank (MBC2's 512 cells, a
/// 2 KiB chip) repeats through 0xA000-0xBFFF, as the game sees it.
fn cartridge_ram_index(index: usize, length: usize) -> Option<usize> {
    if index < length {
        Some(index)
    } else if length > 0 && index < 0x2000 {
        Some(index % length)
    } else {
        None
    }
}

/// Copies cartridge RAM `ram` into `target` as [`cartridge_ram_index`] lays
/// it out, a slice at a time rather than byte by byte, since it runs every
/// frame.
fn copy_cartridge_ram(target: &mut [u8], ram: &[u8]) {
    let filled = if !ram.is_empty() && ram.len() < 0x2000 {
        for chunk in target[..0x2000].chunks_mut(ram.len()) {
            chunk.copy_from_slice(&ram[..chunk.len()]);
        }
        0x2000
    } else {
        let length = ram.len().min(target.len());
        target[..length].copy_from_slice(&ram[..length]);
        length
    };
    target[filled..].fill(0);
}

/// Writes the bytes of `buffer` that differ from `written` into `target`,
/// the emulator's own memory, at the index `target_index` gives (none skips
/// the byte), except past `target`'s end. Returns whether it wrote any.
fn write_changes(
    buffer: &[u8],
    written: &[u8],
    target: &mut [u8],
    target_index: impl Fn(usize) -> Option<usize>,
) -> bool {
    let mut changed = false;
    for (index, (&byte, &before)) in buffer.iter().zip(written).enumerate() {
        if byte != before
            && let Some(target) = target_index(index).and_then(|index| target.get_mut(index))
        {
            *target = byte;
            changed = true;
        }
    }
    changed
}

impl MemorySnapshot {
    fn new() -> Self {
        MemorySnapshot {
            rom: Box::new([0; 0x8000]),
            video_ram: Box::new([0; VIDEO_RAM_SIZE]),
            cartridge_ram: Box::new([0; CARTRIDGE_RAM_SIZE]),
            work_ram: Box::new([0; WORK_RAM_SIZE]),
            oam: Box::new([0; 0x100]),
            io: Box::new([0; 0x80]),
            high_ram: Box::new([0; 0x80]),
            written: Box::new(WrittenMemory {
                video_ram: [0; VIDEO_RAM_SIZE],
                cartridge_ram: [0; CARTRIDGE_RAM_SIZE],
                work_ram: [0; WORK_RAM_SIZE],
                oam: [0; 0x100],
                high_ram: [0; 0x80],
            }),
            has_rom: false,
        }
    }

    /// Writes what RetroArch changed in the RAM buffers since the last
    /// [`MemorySnapshot::update`] into the emulator. Returns whether the
    /// cartridge RAM changed.
    fn apply(&mut self, emulator: &mut Emulator) -> bool {
        let written = &mut *self.written;
        let changed = [
            *self.work_ram != written.work_ram,
            self.high_ram[..0x7F] != written.high_ram[..0x7F],
            *self.video_ram != written.video_ram,
            self.oam[..0xA0] != written.oam[..0xA0],
            *self.cartridge_ram != written.cartridge_ram,
        ];
        if !changed.contains(&true) {
            return false;
        }
        let memory = emulator.memory_mut();
        write_changes(&self.work_ram[..], &written.work_ram, memory.work_ram, Some);
        write_changes(
            &self.high_ram[..0x7F],
            &written.high_ram[..0x7F],
            memory.high_ram,
            Some,
        );
        write_changes(
            &self.video_ram[..],
            &written.video_ram,
            memory.video_ram,
            Some,
        );
        write_changes(
            &self.oam[..0xA0],
            &written.oam[..0xA0],
            memory.object_attribute_memory,
            Some,
        );
        let cartridge_length = memory.cartridge_ram.len();
        let cartridge_changed = write_changes(
            &self.cartridge_ram[..],
            &written.cartridge_ram,
            memory.cartridge_ram,
            |index| cartridge_ram_index(index, cartridge_length),
        );
        written.work_ram.copy_from_slice(&self.work_ram[..]);
        written.high_ram.copy_from_slice(&self.high_ram[..]);
        written.video_ram.copy_from_slice(&self.video_ram[..]);
        written.oam.copy_from_slice(&self.oam[..]);
        written
            .cartridge_ram
            .copy_from_slice(&self.cartridge_ram[..]);
        cartridge_changed
    }

    /// Copies the emulator's memory.
    fn update(&mut self, emulator: &mut Emulator) {
        copy_into(self.work_ram.as_mut_slice(), emulator.work_ram());
        copy_into(&mut self.high_ram[..0x7F], emulator.high_ram());
        self.high_ram[0x7F] = emulator.read_memory(0xFFFF);
        copy_cartridge_ram(self.cartridge_ram.as_mut_slice(), emulator.cartridge_ram());
        for (offset, byte) in self.io.iter_mut().enumerate() {
            *byte = emulator.read_memory(0xFF00 + offset as u16);
        }
        let (video_ram, oam) = emulator.video_memory();
        copy_into(self.video_ram.as_mut_slice(), video_ram);
        copy_into(&mut self.oam[..], oam);
        let written = &mut *self.written;
        written.work_ram.copy_from_slice(&self.work_ram[..]);
        written.high_ram.copy_from_slice(&self.high_ram[..]);
        written.video_ram.copy_from_slice(&self.video_ram[..]);
        written.oam.copy_from_slice(&self.oam[..]);
        written
            .cartridge_ram
            .copy_from_slice(&self.cartridge_ram[..]);
    }

    /// The regions rcheevos reads, in the layout of its Game Boy and Game
    /// Boy Color memory maps (`rc_consoles.c`): the address space as the CPU
    /// sees it with bank 0 of video and cartridge RAM and bank 1 of work RAM,
    /// then on a Game Boy Color work RAM banks 2-7 from 0x10000, and
    /// cartridge RAM banks 1-15 from 0x16000.
    fn regions<'a>(&'a mut self, rom: &[u8], is_cgb: bool) -> Vec<MemoryRegion<'a>> {
        if !self.has_rom {
            copy_into(self.rom.as_mut_slice(), rom);
            self.has_rom = true;
        }
        let MemorySnapshot {
            rom,
            video_ram,
            cartridge_ram,
            work_ram,
            oam,
            io,
            high_ram,
            written: _,
            has_rom: _,
        } = self;
        let (low_work_ram, high_work_ram) = work_ram.split_at_mut(DMG_WORK_RAM_SIZE);
        let (cartridge_bank_zero, other_cartridge_banks) = cartridge_ram.split_at_mut(0x2000);
        let region = |start, buffer, length, kind| MemoryRegion {
            start,
            buffer,
            length,
            kind,
        };
        let mut regions = vec![
            region(0x0000, &mut rom[..], 0x8000, MemoryKind::Rom),
            region(0x8000, &mut video_ram[..], 0x2000, MemoryKind::VideoRam),
            region(0xA000, cartridge_bank_zero, 0x2000, MemoryKind::SaveRam),
            region(0xC000, low_work_ram, 0x2000, MemoryKind::SystemRam),
            region(0xFE00, &mut oam[..], 0x100, MemoryKind::VideoRam),
            region(0xFF00, &mut io[..], 0x80, MemoryKind::Registers),
            region(0xFF80, &mut high_ram[..], 0x80, MemoryKind::Other),
        ];
        if is_cgb {
            // Banks 2-5, then 6-7: each region a power of two long.
            let (banks_two_to_five, banks_six_and_seven) = high_work_ram.split_at_mut(0x4000);
            regions.push(region(
                0x1_0000,
                banks_two_to_five,
                0x4000,
                MemoryKind::SystemRam,
            ));
            regions.push(region(
                0x1_4000,
                banks_six_and_seven,
                0x2000,
                MemoryKind::SystemRam,
            ));
        }
        // Banks 1, 2-5, 6-13 and 14-15.
        let (bank_one, rest) = other_cartridge_banks.split_at_mut(0x2000);
        let (banks_two_to_five, rest) = rest.split_at_mut(0x8000);
        let (banks_six_to_thirteen, banks_fourteen_and_fifteen) = rest.split_at_mut(0x1_0000);
        regions.push(region(0x1_6000, bank_one, 0x2000, MemoryKind::SaveRam));
        regions.push(region(
            0x1_8000,
            banks_two_to_five,
            0x8000,
            MemoryKind::SaveRam,
        ));
        regions.push(region(
            0x2_0000,
            banks_six_to_thirteen,
            0x1_0000,
            MemoryKind::SaveRam,
        ));
        regions.push(region(
            0x3_0000,
            banks_fourteen_and_fifteen,
            0x4000,
            MemoryKind::SaveRam,
        ));
        regions
    }
}

#[cfg(test)]
mod tests;
