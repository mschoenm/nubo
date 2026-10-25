//! The libretro interface: the C functions RetroArch calls, with the types
//! and constants of `libretro.h` that the core uses, written by hand.
//!
//! The definitions are ported from `libretro.h` in
//! [libretro-common](https://github.com/libretro/libretro-common)
//! (`include/libretro.h`, Copyright (C) 2010-2024 The RetroArch team, MIT
//! license; see `NOTICE`).
//!
//! This is the only module with `unsafe` code, all of it to cross into C:
//! the exported functions have to be `#[unsafe(no_mangle)]`, and RetroArch
//! hands over raw pointers and C strings. Each `unsafe` block turns them into
//! Rust values right away and hands those to the safe [`Core`]. The rest of
//! what it does:
//!
//! - **State.** The loaded game is a `static Mutex<Option<Core>>` and the
//!   callbacks a `static Mutex<Frontend>`, so keeping them needs no
//!   `unsafe`. RetroArch calls a core from one thread at a time.
//! - **Panics.** Every exported function that does more than store a
//!   callback or return a constant runs its body under `catch_unwind`
//!   (`guard`), so a panic fails the call (a game doesn't load, a frame
//!   is skipped) and is logged, instead of crossing into C, which would
//!   abort RetroArch.
//! - **Logging** goes through RetroArch's log interface
//!   (`RETRO_ENVIRONMENT_GET_LOG_INTERFACE`), passing each message as an
//!   argument to `"%s"`, so it is never read as a format; without the
//!   interface, to standard error.
//! - **Pointers RetroArch keeps** (the save buffers, the memory maps, the
//!   frame during `retro_video_refresh`) point into buffers the `Core` owns
//!   and never moves or resizes while the game is loaded. RetroArch reads and
//!   writes them only between calls into the core, on the core's thread, and
//!   no function here holds a reference into them while it calls RetroArch
//!   (`present` holds `&Core` for the frame, but borrows none of them), so
//!   its accesses never overlap a Rust borrow. Keep it that way: never call
//!   RetroArch while holding `&mut` to the save, clock or memory map buffers.
//!   (Rust's aliasing rules formally end a pointer at the next mutable borrow
//!   of its buffer; buffers of `Cell<u8>` would be the fully sound form, if
//!   Miri ever runs these tests.)
#![expect(
    unsafe_code,
    reason = "a libretro core exports C functions and reads the pointers RetroArch passes"
)]

use crate::core::{
    BUTTONS, Core, FRAMES_PER_SECOND, MAX_HEIGHT, MAX_WIDTH, MemoryKind, PixelFormat, SAMPLE_RATE,
    SCREEN_HEIGHT, SCREEN_WIDTH, Video,
};
use crate::options::{self, OptionDefinition, Options};
use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

/// The libretro API version this core implements.
const RETRO_API_VERSION: c_uint = 1;

const RETRO_DEVICE_JOYPAD: c_uint = 1;
const RETRO_DEVICE_ID_JOYPAD_MASK: c_uint = 256;

const RETRO_REGION_NTSC: c_uint = 0;

const RETRO_MEMORY_SAVE_RAM: c_uint = 0;
const RETRO_MEMORY_RTC: c_uint = 1;
const RETRO_MEMORY_SYSTEM_RAM: c_uint = 2;
const RETRO_MEMORY_VIDEO_RAM: c_uint = 3;

const RETRO_ENVIRONMENT_EXPERIMENTAL: c_uint = 0x10000;
const RETRO_ENVIRONMENT_SET_PIXEL_FORMAT: c_uint = 10;
const RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS: c_uint = 11;
const RETRO_ENVIRONMENT_GET_VARIABLE: c_uint = 15;
const RETRO_ENVIRONMENT_SET_VARIABLES: c_uint = 16;
const RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE: c_uint = 17;
const RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE: c_uint = 23;
const RETRO_ENVIRONMENT_GET_LOG_INTERFACE: c_uint = 27;
const RETRO_ENVIRONMENT_SET_CONTROLLER_INFO: c_uint = 35;
const RETRO_ENVIRONMENT_SET_MEMORY_MAPS: c_uint = 36 | RETRO_ENVIRONMENT_EXPERIMENTAL;
const RETRO_ENVIRONMENT_SET_GEOMETRY: c_uint = 37;
const RETRO_ENVIRONMENT_SET_SUPPORT_ACHIEVEMENTS: c_uint = 42 | RETRO_ENVIRONMENT_EXPERIMENTAL;
const RETRO_ENVIRONMENT_GET_INPUT_BITMASKS: c_uint = 51 | RETRO_ENVIRONMENT_EXPERIMENTAL;
const RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION: c_uint = 52;
const RETRO_ENVIRONMENT_SET_CORE_OPTIONS: c_uint = 53;
const RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2: c_uint = 67;

const RETRO_PIXEL_FORMAT_0RGB1555: c_int = 0;
const RETRO_PIXEL_FORMAT_XRGB8888: c_int = 1;

const RETRO_MEMDESC_CONST: u64 = 1 << 0;
const RETRO_MEMDESC_SYSTEM_RAM: u64 = 1 << 2;
const RETRO_MEMDESC_SAVE_RAM: u64 = 1 << 3;
const RETRO_MEMDESC_VIDEO_RAM: u64 = 1 << 4;

const RETRO_LOG_INFO: c_int = 1;
const RETRO_LOG_WARN: c_int = 2;
const RETRO_LOG_ERROR: c_int = 3;

const RETRO_RUMBLE_STRONG: c_int = 0;

/// `RETRO_NUM_CORE_OPTION_VALUES_MAX`: the length of an option's value list,
/// including the empty entry that ends it.
const RETRO_NUM_CORE_OPTION_VALUES_MAX: usize = 128;

type RetroEnvironment = unsafe extern "C" fn(cmd: c_uint, data: *mut c_void) -> bool;
type RetroVideoRefresh =
    unsafe extern "C" fn(data: *const c_void, width: c_uint, height: c_uint, pitch: usize);
type RetroAudioSample = unsafe extern "C" fn(left: i16, right: i16);
type RetroAudioSampleBatch = unsafe extern "C" fn(data: *const i16, frames: usize) -> usize;
type RetroInputPoll = unsafe extern "C" fn();
type RetroInputState =
    unsafe extern "C" fn(port: c_uint, device: c_uint, index: c_uint, id: c_uint) -> i16;
type RetroLogPrintf = unsafe extern "C" fn(level: c_int, format: *const c_char, ...);
type RetroSetRumbleState = unsafe extern "C" fn(port: c_uint, effect: c_int, strength: u16) -> bool;

/// `struct retro_system_info`: the core's name, version and the file
/// extensions it opens, which RetroArch asks for before loading a game.
#[repr(C)]
pub struct RetroSystemInfo {
    library_name: *const c_char,
    library_version: *const c_char,
    valid_extensions: *const c_char,
    need_fullpath: bool,
    block_extract: bool,
}

/// `struct retro_game_geometry`: the size of the picture the core sends, and
/// the shape to show it in.
#[repr(C)]
pub struct RetroGameGeometry {
    base_width: c_uint,
    base_height: c_uint,
    max_width: c_uint,
    max_height: c_uint,
    aspect_ratio: f32,
}

/// `struct retro_system_timing`: how many frames a second the core makes, and
/// its sound's sample rate.
#[repr(C)]
pub struct RetroSystemTiming {
    fps: f64,
    sample_rate: f64,
}

/// `struct retro_system_av_info`: the picture's size and the timing together,
/// which RetroArch asks for once a game is loaded.
#[repr(C)]
pub struct RetroSystemAvInfo {
    geometry: RetroGameGeometry,
    timing: RetroSystemTiming,
}

/// `struct retro_game_info`: the game RetroArch hands the core to load, as a
/// path and the file's bytes.
#[repr(C)]
pub struct RetroGameInfo {
    path: *const c_char,
    data: *const c_void,
    size: usize,
    meta: *const c_char,
}

/// `struct retro_variable`: one option's key and value, to read the player's
/// choice or, in the oldest form, to declare the option.
#[repr(C)]
struct RetroVariable {
    key: *const c_char,
    value: *const c_char,
}

/// `struct retro_input_descriptor`: a button's name for RetroArch's remapping
/// menu.
#[repr(C)]
struct RetroInputDescriptor {
    port: c_uint,
    device: c_uint,
    index: c_uint,
    id: c_uint,
    description: *const c_char,
}

/// `struct retro_controller_description`: one kind of controller a player
/// port can take, by name.
#[repr(C)]
struct RetroControllerDescription {
    desc: *const c_char,
    id: c_uint,
}

/// `struct retro_controller_info`: the list of controller kinds one player
/// port can take.
#[repr(C)]
struct RetroControllerInfo {
    types: *const RetroControllerDescription,
    num_types: c_uint,
}

/// `struct retro_log_callback`: the function RetroArch gives the core to
/// write to its log.
#[repr(C)]
struct RetroLogCallback {
    log: Option<RetroLogPrintf>,
}

/// `struct retro_rumble_interface`: the function RetroArch gives the core to
/// make a controller rumble.
#[repr(C)]
struct RetroRumbleInterface {
    set_rumble_state: Option<RetroSetRumbleState>,
}

/// `struct retro_memory_descriptor`: one block of the console's memory that
/// RetroArch's achievements and cheats read, where it sits in the Game Boy's
/// address space (`start`, and `select`, the address bits that pick it) and
/// where its bytes are (`ptr`).
#[repr(C)]
struct RetroMemoryDescriptor {
    flags: u64,
    ptr: *mut c_void,
    offset: usize,
    start: usize,
    select: usize,
    disconnect: usize,
    len: usize,
    addrspace: *const c_char,
}

/// `struct retro_memory_map`: the list of memory descriptors the core hands
/// RetroArch.
#[repr(C)]
struct RetroMemoryMap {
    descriptors: *const RetroMemoryDescriptor,
    num_descriptors: c_uint,
}

/// `struct retro_core_option_value`: one value an option can take, with the
/// label RetroArch's menu shows for it.
#[repr(C)]
#[derive(Clone, Copy)]
struct RetroCoreOptionValue {
    value: *const c_char,
    label: *const c_char,
}

/// An option's list of values with none in it: the end of the list in
/// every entry.
const NO_VALUES: [RetroCoreOptionValue; RETRO_NUM_CORE_OPTION_VALUES_MAX] = [RetroCoreOptionValue {
    value: ptr::null(),
    label: ptr::null(),
};
    RETRO_NUM_CORE_OPTION_VALUES_MAX];

/// `struct retro_core_option_definition`, for
/// `RETRO_ENVIRONMENT_SET_CORE_OPTIONS`: one option as the version 1
/// interface declares it, for frontends without version 2.
#[repr(C)]
struct RetroCoreOptionDefinition {
    key: *const c_char,
    desc: *const c_char,
    info: *const c_char,
    values: [RetroCoreOptionValue; RETRO_NUM_CORE_OPTION_VALUES_MAX],
    default_value: *const c_char,
}

/// `struct retro_core_option_v2_category`: a heading the version 2 options
/// can be grouped under in RetroArch's menu.
#[repr(C)]
struct RetroCoreOptionV2Category {
    key: *const c_char,
    desc: *const c_char,
    info: *const c_char,
}

/// `struct retro_core_option_v2_definition`: one option as the version 2
/// interface declares it, with its description, values and default.
#[repr(C)]
struct RetroCoreOptionV2Definition {
    key: *const c_char,
    desc: *const c_char,
    desc_categorized: *const c_char,
    info: *const c_char,
    info_categorized: *const c_char,
    category_key: *const c_char,
    values: [RetroCoreOptionValue; RETRO_NUM_CORE_OPTION_VALUES_MAX],
    default_value: *const c_char,
}

/// `struct retro_core_options_v2`: every option and category, handed to
/// RetroArch with `RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2`.
#[repr(C)]
struct RetroCoreOptionsV2 {
    categories: *mut RetroCoreOptionV2Category,
    definitions: *mut RetroCoreOptionV2Definition,
}

/// The callbacks RetroArch hands over, and what the core learned about it.
#[derive(Clone, Copy)]
struct Frontend {
    environment: Option<RetroEnvironment>,
    video_refresh: Option<RetroVideoRefresh>,
    audio_sample: Option<RetroAudioSample>,
    audio_sample_batch: Option<RetroAudioSampleBatch>,
    input_poll: Option<RetroInputPoll>,
    input_state: Option<RetroInputState>,
    log: Option<RetroLogPrintf>,
    set_rumble_state: Option<RetroSetRumbleState>,
    /// Whether `input_state` can return every button at once
    /// (`RETRO_ENVIRONMENT_GET_INPUT_BITMASKS`).
    input_bitmasks: bool,
    /// The rumble strength last sent, to send only changes.
    rumble: u16,
}

impl Frontend {
    /// No callbacks yet, as before RetroArch hands them over.
    const NONE: Frontend = Frontend {
        environment: None,
        video_refresh: None,
        audio_sample: None,
        audio_sample_batch: None,
        input_poll: None,
        input_state: None,
        log: None,
        set_rumble_state: None,
        input_bitmasks: false,
        rumble: 0,
    };
}

/// The callbacks RetroArch gave, shared by every exported function.
static FRONTEND: Mutex<Frontend> = Mutex::new(Frontend::NONE);
/// The loaded game, or `None` when there is none. RetroArch is called with it
/// held only while a frame is handed over (see `retro_run`).
static CORE: Mutex<Option<Core>> = Mutex::new(None);
/// The picture size RetroArch was last told about, in `retro_get_system_av_info`
/// or with `RETRO_ENVIRONMENT_SET_GEOMETRY`.
static GEOMETRY: Mutex<(usize, usize)> = Mutex::new((SCREEN_WIDTH, SCREEN_HEIGHT));

/// Locks `mutex`. A panic while it was held, which `catch_unwind` caught,
/// leaves nothing half-written that matters more than going on, so a
/// poisoned lock is used anyway.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A copy of the callbacks, so none is called with the lock held.
fn frontend() -> Frontend {
    *lock(&FRONTEND)
}

/// Runs `body` and returns what it returns, or logs the panic and returns
/// `fallback` if it panics, so no panic crosses into C.
fn guard<T>(name: &str, fallback: T, body: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(result) => result,
        Err(payload) => {
            let reason = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("unknown");
            log(RETRO_LOG_ERROR, &format!("{name} failed: {reason}"));
            fallback
        }
    }
}

/// Calls RetroArch's environment callback, if it gave one.
fn environment(command: c_uint, data: *mut c_void) -> bool {
    let Some(callback) = frontend().environment else {
        return false;
    };
    // SAFETY: RetroArch's callback takes any command, with the data that
    // command's documentation in libretro.h asks for, which every caller here
    // passes.
    unsafe { callback(command, data) }
}

/// Logs `message` through RetroArch's log interface, or to standard error.
fn log(level: c_int, message: &str) {
    let text = CString::new(format!("[Nubo] {message}\n").replace('\0', " ")).unwrap_or_default();
    match frontend().log {
        // SAFETY: the format is a C string with one `%s`, and `text` is the
        // C string that matches it.
        Some(log) => unsafe { log(level, c"%s".as_ptr(), text.as_ptr()) },
        None => eprint!("{}", text.to_string_lossy()),
    }
}

/// Reads the core options from RetroArch.
fn read_options() -> Options {
    let mut options = Options::default();
    for definition in options::definitions() {
        let key = CString::new(definition.key).unwrap_or_default();
        let mut variable = RetroVariable {
            key: key.as_ptr(),
            value: ptr::null(),
        };
        if !environment(RETRO_ENVIRONMENT_GET_VARIABLE, (&raw mut variable).cast()) {
            continue;
        }
        if variable.value.is_null() {
            continue;
        }
        // SAFETY: RetroArch set `value` to a C string it keeps until the next
        // environment call; it is copied before that.
        let value = unsafe { CStr::from_ptr(variable.value) }.to_string_lossy();
        if !options.set(definition.key, &value) {
            log(
                RETRO_LOG_WARN,
                &format!("unknown value {value} for {}", definition.key),
            );
        }
    }
    options
}

/// The C strings of the option definitions, kept alive while RetroArch
/// copies them.
#[derive(Default)]
struct OptionStrings {
    strings: Vec<CString>,
}

impl OptionStrings {
    /// A C string of `text`, kept until `self` is dropped.
    fn c_string(&mut self, text: &str) -> *const c_char {
        let string = CString::new(text).unwrap_or_default();
        let pointer = string.as_ptr();
        self.strings.push(string);
        pointer
    }

    /// The values of `definition`, ended by an empty one.
    fn values(
        &mut self,
        definition: &OptionDefinition,
    ) -> [RetroCoreOptionValue; RETRO_NUM_CORE_OPTION_VALUES_MAX] {
        let mut values = NO_VALUES;
        for (slot, (value, label)) in values.iter_mut().zip(&definition.values) {
            *slot = RetroCoreOptionValue {
                value: self.c_string(value),
                label: self.c_string(label),
            };
        }
        values
    }
}

/// Tells RetroArch the core options, in the newest form it understands.
fn set_core_options() {
    let mut version: c_uint = 0;
    if !environment(
        RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION,
        (&raw mut version).cast(),
    ) {
        version = 0;
    }
    let option_definitions = options::definitions();
    let mut strings = OptionStrings::default();
    if version >= 2 {
        let mut definitions: Vec<RetroCoreOptionV2Definition> = option_definitions
            .iter()
            .map(|definition| RetroCoreOptionV2Definition {
                key: strings.c_string(definition.key),
                desc: strings.c_string(definition.description),
                desc_categorized: ptr::null(),
                info: strings.c_string(definition.info),
                info_categorized: ptr::null(),
                category_key: ptr::null(),
                values: strings.values(definition),
                default_value: strings.c_string(definition.default_value()),
            })
            .collect();
        definitions.push(RetroCoreOptionV2Definition {
            key: ptr::null(),
            desc: ptr::null(),
            desc_categorized: ptr::null(),
            info: ptr::null(),
            info_categorized: ptr::null(),
            category_key: ptr::null(),
            values: NO_VALUES,
            default_value: ptr::null(),
        });
        let mut no_categories = RetroCoreOptionV2Category {
            key: ptr::null(),
            desc: ptr::null(),
            info: ptr::null(),
        };
        let mut options = RetroCoreOptionsV2 {
            categories: &raw mut no_categories,
            definitions: definitions.as_mut_ptr(),
        };
        environment(
            RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2,
            (&raw mut options).cast(),
        );
    } else if version >= 1 {
        let mut definitions: Vec<RetroCoreOptionDefinition> = option_definitions
            .iter()
            .map(|definition| RetroCoreOptionDefinition {
                key: strings.c_string(definition.key),
                desc: strings.c_string(definition.description),
                info: strings.c_string(definition.info),
                values: strings.values(definition),
                default_value: strings.c_string(definition.default_value()),
            })
            .collect();
        definitions.push(RetroCoreOptionDefinition {
            key: ptr::null(),
            desc: ptr::null(),
            info: ptr::null(),
            values: NO_VALUES,
            default_value: ptr::null(),
        });
        environment(
            RETRO_ENVIRONMENT_SET_CORE_OPTIONS,
            definitions.as_mut_ptr().cast(),
        );
    } else {
        // The oldest form: "Description; first|second|...", the first value
        // being the default.
        let mut variables: Vec<RetroVariable> = option_definitions
            .iter()
            .map(|definition| {
                let values: Vec<&str> = definition
                    .values
                    .iter()
                    .map(|(value, _)| value.as_str())
                    .collect();
                RetroVariable {
                    key: strings.c_string(definition.key),
                    value: strings.c_string(&format!(
                        "{}; {}",
                        definition.description,
                        values.join("|")
                    )),
                }
            })
            .collect();
        variables.push(RetroVariable {
            key: ptr::null(),
            value: ptr::null(),
        });
        environment(
            RETRO_ENVIRONMENT_SET_VARIABLES,
            variables.as_mut_ptr().cast(),
        );
    }
}

/// Names the Game Boy's buttons in RetroArch's remapping menu.
fn set_input_descriptors() {
    // The same buttons on all four ports: the second to fourth are the other
    // players of a Super Game Boy's multitap.
    let mut descriptors: Vec<RetroInputDescriptor> = (0..4)
        .flat_map(|port| {
            BUTTONS
                .iter()
                .map(move |(id, _, name)| RetroInputDescriptor {
                    port,
                    device: RETRO_DEVICE_JOYPAD,
                    index: 0,
                    id: *id,
                    description: name.as_ptr(),
                })
        })
        .collect();
    descriptors.push(RetroInputDescriptor {
        port: 0,
        device: 0,
        index: 0,
        id: 0,
        description: ptr::null(),
    });
    environment(
        RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS,
        descriptors.as_mut_ptr().cast(),
    );
}

/// Asks for XRGB8888 pixels, or settles for the 0RGB1555 every frontend
/// takes.
fn negotiate_pixel_format() -> PixelFormat {
    let mut format = RETRO_PIXEL_FORMAT_XRGB8888;
    if environment(RETRO_ENVIRONMENT_SET_PIXEL_FORMAT, (&raw mut format).cast()) {
        return PixelFormat::Xrgb8888;
    }
    log(
        RETRO_LOG_WARN,
        "the frontend has no XRGB8888 pixels, so frames come as 0RGB1555",
    );
    let mut format = RETRO_PIXEL_FORMAT_0RGB1555;
    environment(RETRO_ENVIRONMENT_SET_PIXEL_FORMAT, (&raw mut format).cast());
    PixelFormat::Rgb1555
}

/// Describes where the Game Boy's memory is, for RetroAchievements: one
/// descriptor per region of `core`'s buffers. The map depends on the model (a
/// Game Boy Color has more work RAM), so it is built again whenever a restart
/// may have changed the model. The buffers never move, so the descriptors
/// stay true after the lock on `core` is released, and RetroArch uses them
/// only between calls into the core (see the module docs).
fn memory_descriptors(core: &mut Core) -> Vec<RetroMemoryDescriptor> {
    core.memory_regions()
        .into_iter()
        .map(|region| RetroMemoryDescriptor {
            flags: match region.kind {
                MemoryKind::Rom => RETRO_MEMDESC_CONST,
                MemoryKind::SystemRam => RETRO_MEMDESC_SYSTEM_RAM,
                MemoryKind::SaveRam => RETRO_MEMDESC_SAVE_RAM,
                MemoryKind::VideoRam => RETRO_MEMDESC_VIDEO_RAM,
                MemoryKind::Registers => RETRO_MEMDESC_CONST,
                MemoryKind::Other => 0,
            },
            select: region.select(),
            ptr: region.buffer.as_mut_ptr().cast(),
            offset: 0,
            start: region.start,
            disconnect: 0,
            len: region.length,
            addrspace: ptr::null(),
        })
        .collect()
}

/// Hands RetroArch the memory map `descriptors` describe. Called with the
/// core already in `CORE` but without its lock held, since RetroArch may ask
/// for the core's memory (`retro_get_memory_data`) while it handles the map.
fn set_memory_maps(descriptors: &[RetroMemoryDescriptor]) {
    let mut map = RetroMemoryMap {
        descriptors: descriptors.as_ptr(),
        num_descriptors: c_uint::try_from(descriptors.len()).unwrap_or(0),
    };
    environment(RETRO_ENVIRONMENT_SET_MEMORY_MAPS, (&raw mut map).cast());
}

/// Reads which buttons the player on `port` (0 for the first) holds, one
/// bit per libretro button number.
fn read_buttons(frontend: &Frontend, port: c_uint) -> u16 {
    let Some(input_state) = frontend.input_state else {
        return 0;
    };
    if frontend.input_bitmasks {
        // SAFETY: a plain call into RetroArch with numbers.
        let mask =
            unsafe { input_state(port, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_MASK) };
        return mask as u16;
    }
    BUTTONS.iter().fold(0, |held, (id, _, _)| {
        // SAFETY: a plain call into RetroArch with numbers.
        let pressed = unsafe { input_state(port, RETRO_DEVICE_JOYPAD, 0, *id) } != 0;
        if pressed { held | 1 << id } else { held }
    })
}

/// The geometry of a picture `width` by `height` pixels, which may grow to
/// a Super Game Boy's picture with its border.
fn geometry((width, height): (usize, usize)) -> RetroGameGeometry {
    RetroGameGeometry {
        base_width: width as c_uint,
        base_height: height as c_uint,
        max_width: MAX_WIDTH as c_uint,
        max_height: MAX_HEIGHT as c_uint,
        aspect_ratio: width as f32 / height as f32,
    }
}

/// Hands RetroArch the frame, the sound and the rumble of the frame `core`
/// just ran. When the picture's size changed (a Super Game Boy's border
/// shown or hidden), RetroArch is told its new size and shape first.
fn present(core: &Core) {
    let frontend = frontend();
    let size = core.video_size();
    let resized = std::mem::replace(&mut *lock(&GEOMETRY), size) != size;
    if resized {
        let mut new_geometry = geometry(size);
        environment(
            RETRO_ENVIRONMENT_SET_GEOMETRY,
            (&raw mut new_geometry).cast(),
        );
    }
    if let Some(video_refresh) = frontend.video_refresh {
        let data: *const c_void = match core.video() {
            Video::Xrgb8888(pixels) => pixels.as_ptr().cast(),
            Video::Rgb1555(pixels) => pixels.as_ptr().cast(),
        };
        // SAFETY: `data` points to `size` pixels of the negotiated format,
        // `pitch` bytes apart, which stay valid during the call.
        unsafe {
            video_refresh(data, size.0 as c_uint, size.1 as c_uint, core.pitch());
        }
    }

    let audio = core.audio();
    if let Some(batch) = frontend.audio_sample_batch {
        let frames = audio.len() / 2;
        let mut sent = 0;
        while sent < frames {
            // SAFETY: the pointer is to `frames - sent` frames of two samples.
            let written = unsafe { batch(audio[sent * 2..].as_ptr(), frames - sent) };
            if written == 0 {
                break;
            }
            sent += written.min(frames - sent);
        }
    } else if let Some(sample) = frontend.audio_sample {
        for &[left, right] in audio.as_chunks::<2>().0 {
            // SAFETY: a plain call into RetroArch with numbers.
            unsafe { sample(left, right) };
        }
    }

    let rumble = core.rumble();
    if rumble != frontend.rumble
        && let Some(set_rumble_state) = frontend.set_rumble_state
    {
        // SAFETY: a plain call into RetroArch with numbers.
        unsafe { set_rumble_state(0, RETRO_RUMBLE_STRONG, rumble) };
        lock(&FRONTEND).rumble = rumble;
    }
}

/// The core's version: Nubo's, with the commit for builds `cargo xtask`
/// made.
fn version() -> &'static CStr {
    static VERSION: OnceLock<CString> = OnceLock::new();
    VERSION.get_or_init(|| {
        let commit = nubo_core::BUILD_COMMIT;
        let version = if commit.is_empty() {
            String::from(env!("CARGO_PKG_VERSION"))
        } else {
            format!(
                "{} {}",
                env!("CARGO_PKG_VERSION"),
                commit.get(..7).unwrap_or(commit)
            )
        };
        CString::new(version).unwrap_or_default()
    })
}

/// The libretro API version the core implements.
#[unsafe(no_mangle)]
pub extern "C" fn retro_api_version() -> c_uint {
    RETRO_API_VERSION
}

/// Called once after the library is loaded, before a game.
#[unsafe(no_mangle)]
pub extern "C" fn retro_init() {}

/// Called once before the library is unloaded: drops the game, if any.
#[unsafe(no_mangle)]
pub extern "C" fn retro_deinit() {
    guard("retro_deinit", (), || {
        *lock(&CORE) = None;
        *lock(&FRONTEND) = Frontend::NONE;
    });
}

/// Describes the core: its name, version and the files it loads.
///
/// # Safety
///
/// `info` must point to a `struct retro_system_info` to fill in.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn retro_get_system_info(info: *mut RetroSystemInfo) {
    guard("retro_get_system_info", (), || {
        if info.is_null() {
            return;
        }
        // SAFETY: RetroArch passes a struct to fill in, and the strings are
        // static.
        unsafe {
            info.write(RetroSystemInfo {
                library_name: c"Nubo".as_ptr(),
                library_version: version().as_ptr(),
                valid_extensions: c"gb|gbc".as_ptr(),
                need_fullpath: false,
                block_extract: false,
            });
        }
    });
}

/// Describes the picture and the sound: 160×144 pixels (256×224 with a Super
/// Game Boy's border) at about 59.73 frames per second, and the sample rate.
///
/// # Safety
///
/// `info` must point to a `struct retro_system_av_info` to fill in.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn retro_get_system_av_info(info: *mut RetroSystemAvInfo) {
    guard("retro_get_system_av_info", (), || {
        if info.is_null() {
            return;
        }
        let size = lock(&CORE)
            .as_ref()
            .map_or((SCREEN_WIDTH, SCREEN_HEIGHT), Core::video_size);
        *lock(&GEOMETRY) = size;
        // SAFETY: RetroArch passes a struct to fill in.
        unsafe {
            info.write(RetroSystemAvInfo {
                geometry: geometry(size),
                timing: RetroSystemTiming {
                    fps: FRAMES_PER_SECOND,
                    sample_rate: f64::from(SAMPLE_RATE),
                },
            });
        }
    });
}

/// Keeps the environment callback, takes RetroArch's log interface, and
/// declares the core options and the four controller ports.
#[unsafe(no_mangle)]
pub extern "C" fn retro_set_environment(callback: Option<RetroEnvironment>) {
    guard("retro_set_environment", (), || {
        lock(&FRONTEND).environment = callback;
        let mut log_callback = RetroLogCallback { log: None };
        if environment(
            RETRO_ENVIRONMENT_GET_LOG_INTERFACE,
            (&raw mut log_callback).cast(),
        ) {
            lock(&FRONTEND).log = log_callback.log;
        }
        set_core_options();
        set_controller_info();
    });
}

/// The one kind of controller every port offers. RetroArch copies the list
/// of ports but keeps pointing at this, so it lives as long as the core.
struct ControllerTypes([RetroControllerDescription; 1]);

// SAFETY: the description only points at a C string literal, which lives
// as long as the program, and nothing ever writes to it.
unsafe impl Sync for ControllerTypes {}

static CONTROLLER_TYPES: ControllerTypes = ControllerTypes([RetroControllerDescription {
    desc: c"Game Boy".as_ptr(),
    id: RETRO_DEVICE_JOYPAD,
}]);

/// Declares the core's four controller ports, each a Game Boy's buttons: the
/// first player's, and the other players of a Super Game Boy's multitap.
fn set_controller_info() {
    let port = || RetroControllerInfo {
        types: CONTROLLER_TYPES.0.as_ptr(),
        num_types: 1,
    };
    let mut ports = [
        port(),
        port(),
        port(),
        port(),
        RetroControllerInfo {
            types: ptr::null(),
            num_types: 0,
        },
    ];
    environment(
        RETRO_ENVIRONMENT_SET_CONTROLLER_INFO,
        ports.as_mut_ptr().cast(),
    );
}

/// Keeps the callback that shows frames.
#[unsafe(no_mangle)]
pub extern "C" fn retro_set_video_refresh(callback: Option<RetroVideoRefresh>) {
    lock(&FRONTEND).video_refresh = callback;
}

/// Keeps the callback that plays one sample, which the core uses only
/// without the batch callback.
#[unsafe(no_mangle)]
pub extern "C" fn retro_set_audio_sample(callback: Option<RetroAudioSample>) {
    lock(&FRONTEND).audio_sample = callback;
}

/// Keeps the callback that plays a frame's sound.
#[unsafe(no_mangle)]
pub extern "C" fn retro_set_audio_sample_batch(callback: Option<RetroAudioSampleBatch>) {
    lock(&FRONTEND).audio_sample_batch = callback;
}

/// Keeps the callback that reads the controllers.
#[unsafe(no_mangle)]
pub extern "C" fn retro_set_input_poll(callback: Option<RetroInputPoll>) {
    lock(&FRONTEND).input_poll = callback;
}

/// Keeps the callback that tells which buttons are held.
#[unsafe(no_mangle)]
pub extern "C" fn retro_set_input_state(callback: Option<RetroInputState>) {
    lock(&FRONTEND).input_state = callback;
}

/// Every port is a Game Boy's buttons, so there is nothing to change.
#[unsafe(no_mangle)]
pub extern "C" fn retro_set_controller_port_device(_port: c_uint, _device: c_uint) {}

/// Starts the game again, on the model and clock the options pick now.
#[unsafe(no_mangle)]
pub extern "C" fn retro_reset() {
    guard("retro_reset", (), || {
        let restarted = lock(&CORE)
            .as_mut()
            .map(|core| core.reset().map(|()| memory_descriptors(core)));
        match restarted {
            Some(Ok(descriptors)) => set_memory_maps(&descriptors),
            Some(Err(error)) => log(
                RETRO_LOG_ERROR,
                &format!("the game didn't start again: {error}"),
            ),
            None => {}
        }
    });
}

/// Runs one frame: reads the buttons, runs the emulator, and hands over the
/// frame and its sound.
///
/// The lock on `CORE` stays held while `present` calls RetroArch's video,
/// sound, rumble and geometry callbacks, since the frame and the sound they
/// read are the core's own buffers. A callback that called back into the core
/// (such as `retro_get_memory_data`) would wait for that lock forever; no
/// known RetroArch callback for these does.
#[unsafe(no_mangle)]
pub extern "C" fn retro_run() {
    guard("retro_run", (), || {
        let mut updated = false;
        if environment(
            RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE,
            (&raw mut updated).cast(),
        ) && updated
        {
            let options = read_options();
            if let Some(core) = lock(&CORE).as_mut() {
                core.set_options(options);
            }
        }

        let frontend = frontend();
        if let Some(input_poll) = frontend.input_poll {
            // SAFETY: a plain call into RetroArch.
            unsafe { input_poll() };
        }
        let buttons = read_buttons(&frontend, 0);
        let other_players = [1, 2, 3].map(|port| read_buttons(&frontend, port));

        let mut core = lock(&CORE);
        let Some(core) = core.as_mut() else {
            return;
        };
        core.set_other_players(other_players);
        core.run(buttons);
        present(core);
    });
}

/// The size of every save state, the same as long as the game is loaded; 0
/// without a game.
#[unsafe(no_mangle)]
pub extern "C" fn retro_serialize_size() -> usize {
    guard("retro_serialize_size", 0, || {
        lock(&CORE).as_ref().map_or(0, Core::serialize_size)
    })
}

/// Writes a save state into `data`.
///
/// # Safety
///
/// `data` must point to `size` bytes the core may write. They may be
/// uninitialized: RetroArch allocates the buffer without clearing it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn retro_serialize(data: *mut c_void, size: usize) -> bool {
    guard("retro_serialize", false, || {
        if data.is_null() {
            return false;
        }
        // SAFETY: RetroArch passes a buffer of `size` bytes to write, maybe uninitialized (it
        // mallocs it); zeroing them first makes them valid bytes to make a slice of.
        let buffer = unsafe {
            ptr::write_bytes(data.cast::<u8>(), 0, size);
            std::slice::from_raw_parts_mut(data.cast::<u8>(), size)
        };
        lock(&CORE)
            .as_mut()
            .is_some_and(|core| core.serialize(buffer))
    })
}

/// Loads a save state from `data`.
///
/// # Safety
///
/// `data` must point to `size` bytes the core may read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn retro_unserialize(data: *const c_void, size: usize) -> bool {
    guard("retro_unserialize", false, || {
        if data.is_null() {
            return false;
        }
        // SAFETY: RetroArch passes a buffer of `size` bytes to read.
        let state = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) };
        let loaded = lock(&CORE).as_mut().map(|core| core.unserialize(state));
        match loaded {
            Some(Ok(())) => true,
            Some(Err(error)) => {
                log(
                    RETRO_LOG_WARN,
                    &format!("the save state didn't load: {error}"),
                );
                false
            }
            None => {
                log(
                    RETRO_LOG_WARN,
                    "no game is loaded to load the save state into",
                );
                false
            }
        }
    })
}

/// Turns every cheat off.
#[unsafe(no_mangle)]
pub extern "C" fn retro_cheat_reset() {
    guard("retro_cheat_reset", (), || {
        if let Some(core) = lock(&CORE).as_mut() {
            core.cheat_reset();
        }
    });
}

/// Sets cheat number `index`: Game Genie or GameShark codes joined with `+`.
///
/// # Safety
///
/// `code` must be null or a C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn retro_cheat_set(index: c_uint, enabled: bool, code: *const c_char) {
    guard("retro_cheat_set", (), || {
        let code = if code.is_null() {
            String::new()
        } else {
            // SAFETY: RetroArch passes a C string.
            unsafe { CStr::from_ptr(code) }
                .to_string_lossy()
                .into_owned()
        };
        let invalid_codes = lock(&CORE)
            .as_mut()
            .map(|core| core.cheat_set(index, enabled, &code))
            .unwrap_or_default();
        for invalid in invalid_codes {
            log(
                RETRO_LOG_WARN,
                &format!("the cheat code {} can't be used", invalid.0),
            );
        }
    });
}

/// Loads a game from the bytes RetroArch read.
///
/// # Safety
///
/// `game` must be null or point to a `struct retro_game_info` whose `data`
/// is null or points to `size` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn retro_load_game(game: *const RetroGameInfo) -> bool {
    guard("retro_load_game", false, || {
        if game.is_null() {
            return false;
        }
        // SAFETY: RetroArch passes the game's information...
        let game = unsafe { &*game };
        if game.data.is_null() {
            log(RETRO_LOG_ERROR, "no game was given");
            return false;
        }
        // SAFETY: ...whose data is the file's `size` bytes.
        let rom = unsafe { std::slice::from_raw_parts(game.data.cast::<u8>(), game.size) };

        let pixel_format = negotiate_pixel_format();
        set_input_descriptors();
        let mut core = match Core::load(rom, read_options(), pixel_format) {
            Ok(core) => core,
            Err(error) => {
                log(RETRO_LOG_ERROR, &format!("the game didn't load: {error}"));
                return false;
            }
        };
        log(
            RETRO_LOG_INFO,
            &format!("{} on {}", core.header().title, core.model().description()),
        );
        let descriptors = memory_descriptors(&mut core);
        let has_rumble = core.has_rumble();
        *lock(&CORE) = Some(core);
        set_memory_maps(&descriptors);
        // Before the first `retro_run`, as libretro.h asks.
        let mut supported = true;
        environment(
            RETRO_ENVIRONMENT_SET_SUPPORT_ACHIEVEMENTS,
            (&raw mut supported).cast(),
        );

        let mut frontend = lock(&FRONTEND);
        frontend.input_bitmasks = false;
        frontend.set_rumble_state = None;
        frontend.rumble = 0;
        drop(frontend);
        if environment(RETRO_ENVIRONMENT_GET_INPUT_BITMASKS, ptr::null_mut()) {
            lock(&FRONTEND).input_bitmasks = true;
        }
        if has_rumble {
            let mut rumble = RetroRumbleInterface {
                set_rumble_state: None,
            };
            if environment(
                RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE,
                (&raw mut rumble).cast(),
            ) {
                lock(&FRONTEND).set_rumble_state = rumble.set_rumble_state;
            }
        }
        true
    })
}

/// Loads several files as one game, which no Game Boy game needs.
#[unsafe(no_mangle)]
pub extern "C" fn retro_load_game_special(
    _game_type: c_uint,
    _info: *const RetroGameInfo,
    _num_info: usize,
) -> bool {
    false
}

/// Unloads the game.
#[unsafe(no_mangle)]
pub extern "C" fn retro_unload_game() {
    guard("retro_unload_game", (), || {
        *lock(&CORE) = None;
        let frontend = frontend();
        if frontend.rumble != 0
            && let Some(set_rumble_state) = frontend.set_rumble_state
        {
            // SAFETY: a plain call into RetroArch with numbers.
            unsafe { set_rumble_state(0, RETRO_RUMBLE_STRONG, 0) };
        }
        let mut frontend = lock(&FRONTEND);
        frontend.rumble = 0;
        frontend.set_rumble_state = None;
    });
}

/// The Game Boy shows the same 59.73 frames per second everywhere; libretro
/// calls that NTSC.
#[unsafe(no_mangle)]
pub extern "C" fn retro_get_region() -> c_uint {
    RETRO_REGION_NTSC
}

/// The buffer of a kind of memory: the battery save, the clock, work RAM or
/// video RAM. They stay at one address while the game is loaded.
fn memory(core: &mut Core, id: c_uint) -> &mut [u8] {
    match id {
        RETRO_MEMORY_SAVE_RAM => core.save_ram(),
        RETRO_MEMORY_RTC => core.rtc_memory(),
        RETRO_MEMORY_SYSTEM_RAM => core.system_ram(),
        RETRO_MEMORY_VIDEO_RAM => core.video_ram(),
        _ => &mut [],
    }
}

/// Where a kind of memory is, or null. RetroArch keeps the pointer and uses
/// it only between calls into the core (see the module docs).
#[unsafe(no_mangle)]
pub extern "C" fn retro_get_memory_data(id: c_uint) -> *mut c_void {
    guard("retro_get_memory_data", ptr::null_mut(), || {
        let mut core = lock(&CORE);
        let Some(core) = core.as_mut() else {
            return ptr::null_mut();
        };
        let buffer = memory(core, id);
        if buffer.is_empty() {
            ptr::null_mut()
        } else {
            buffer.as_mut_ptr().cast()
        }
    })
}

/// How long a kind of memory is, or 0.
#[unsafe(no_mangle)]
pub extern "C" fn retro_get_memory_size(id: c_uint) -> usize {
    guard("retro_get_memory_size", 0, || {
        lock(&CORE)
            .as_mut()
            .map_or(0, |core| memory(core, id).len())
    })
}

#[cfg(test)]
mod tests;
