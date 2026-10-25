//! Calls the exported functions the way RetroArch does, with callbacks that
//! play RetroArch's part and record what the core hands them.
//!
//! The core and its callbacks are global, as a dynamic library's are, so the
//! tests take turns through `TURN`.

use super::*;
use crate::test_roms::{
    CHEATS, JOYPAD, MBC1_RAM_BATTERY, MBC5_RUMBLE, RAM, RAM_8_KIB, ROM_BYTE_AT_0200, ROM_ONLY,
    RUMBLE, TONE, cartridge,
};

/// One test at a time.
static TURN: Mutex<()> = Mutex::new(());

/// How the pretend RetroArch answers.
struct Settings {
    accepts_xrgb8888: bool,
    options_version: c_uint,
    bitmasks: bool,
    /// The core options' values, as `RETRO_ENVIRONMENT_GET_VARIABLE` hands
    /// them out.
    variables: Vec<(String, CString)>,
    variables_updated: bool,
}

/// What the core handed the pretend RetroArch.
#[derive(Default)]
struct Record {
    pixel_formats: Vec<c_int>,
    /// The first player's input descriptors: each button's id and name.
    input_descriptors: Vec<(c_uint, String)>,
    /// The port of every input descriptor.
    descriptor_ports: Vec<c_uint>,
    /// Where every input descriptor's name is, which RetroArch keeps.
    descriptor_names: Vec<usize>,
    /// `RETRO_ENVIRONMENT_SET_CONTROLLER_INFO`: the device ids each port
    /// offers.
    controller_ports: Vec<Vec<c_uint>>,
    /// Where each port's list of controller types is, which RetroArch keeps.
    controller_types: Vec<usize>,
    /// `RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2` and `_SET_CORE_OPTIONS`: each
    /// option's key, default and values.
    options: Vec<(String, String, Vec<String>)>,
    /// `RETRO_ENVIRONMENT_SET_VARIABLES`: each key and value.
    variables: Vec<(String, String)>,
    /// Each memory descriptor: flags, start, select, length and where.
    memory_maps: Vec<(u64, usize, usize, usize, usize)>,
    /// The work RAM's size, which the pretend RetroArch asks the core for
    /// while it handles the memory map, as rcheevos may.
    system_ram_size_while_mapping: Option<usize>,
    achievements: Option<bool>,
    /// Each frame: width, height, pitch and the first pixel's bytes.
    frames: Vec<(c_uint, c_uint, usize, Vec<u8>)>,
    /// `RETRO_ENVIRONMENT_SET_GEOMETRY`: each new base width and height.
    geometries: Vec<(c_uint, c_uint)>,
    /// Each batch of sound: how many frames of two samples.
    audio_batches: Vec<usize>,
    polls: usize,
    rumble: Vec<u16>,
}

static SETTINGS: Mutex<Settings> = Mutex::new(Settings {
    accepts_xrgb8888: true,
    options_version: 2,
    bitmasks: true,
    variables: Vec::new(),
    variables_updated: false,
});
static RECORD: Mutex<Option<Record>> = Mutex::new(None);
/// The buttons the pretend player holds, one bit per libretro button.
static HELD: Mutex<[u16; 4]> = Mutex::new([0; 4]);

/// Changes this test's record, making it first if there is none.
fn record<T>(change: impl FnOnce(&mut Record) -> T) -> T {
    change(lock(&RECORD).get_or_insert_with(Record::default))
}

/// A C string as a Rust one, or an empty one for null.
fn text(pointer: *const c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    // SAFETY: the core passes C strings.
    unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

/// The values of an option definition, up to the empty one.
fn values(values: &[RetroCoreOptionValue]) -> Vec<String> {
    values
        .iter()
        .take_while(|value| !value.value.is_null())
        .map(|value| text(value.value))
        .collect()
}

/// RetroArch's environment callback: answers each command as `SETTINGS` say
/// and records what the core hands over.
unsafe extern "C" fn test_environment(cmd: c_uint, data: *mut c_void) -> bool {
    let settings = lock(&SETTINGS);
    // SAFETY: each command's data is what libretro.h says it is.
    unsafe {
        match cmd {
            RETRO_ENVIRONMENT_SET_PIXEL_FORMAT => {
                let format = *data.cast::<c_int>();
                record(|record| record.pixel_formats.push(format));
                format == RETRO_PIXEL_FORMAT_0RGB1555 || settings.accepts_xrgb8888
            }
            RETRO_ENVIRONMENT_SET_CONTROLLER_INFO => {
                let mut port = data.cast::<RetroControllerInfo>().cast_const();
                let mut ports = Vec::new();
                while !(*port).types.is_null() {
                    let types =
                        std::slice::from_raw_parts((*port).types, (*port).num_types as usize);
                    ports.push(types.iter().map(|device| device.id).collect());
                    let types = (*port).types.addr();
                    record(|record| record.controller_types.push(types));
                    port = port.add(1);
                }
                record(|record| record.controller_ports = ports);
                true
            }
            RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS => {
                let mut descriptor = data.cast::<RetroInputDescriptor>().cast_const();
                while !(*descriptor).description.is_null() {
                    if (*descriptor).port == 0 {
                        let entry = ((*descriptor).id, text((*descriptor).description));
                        record(|record| record.input_descriptors.push(entry));
                    }
                    record(|record| record.descriptor_ports.push((*descriptor).port));
                    let name = (*descriptor).description.addr();
                    record(|record| record.descriptor_names.push(name));
                    descriptor = descriptor.add(1);
                }
                true
            }
            RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION => {
                *data.cast::<c_uint>() = settings.options_version;
                true
            }
            RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2 if settings.options_version >= 2 => {
                let options = &*data.cast::<RetroCoreOptionsV2>();
                assert!((*options.categories).key.is_null());
                let mut definition = options.definitions.cast_const();
                while !(*definition).key.is_null() {
                    let entry = (
                        text((*definition).key),
                        text((*definition).default_value),
                        values(&(*definition).values),
                    );
                    record(|record| record.options.push(entry));
                    definition = definition.add(1);
                }
                true
            }
            RETRO_ENVIRONMENT_SET_CORE_OPTIONS if settings.options_version >= 1 => {
                let mut definition = data.cast::<RetroCoreOptionDefinition>().cast_const();
                while !(*definition).key.is_null() {
                    let entry = (
                        text((*definition).key),
                        text((*definition).default_value),
                        values(&(*definition).values),
                    );
                    record(|record| record.options.push(entry));
                    definition = definition.add(1);
                }
                true
            }
            RETRO_ENVIRONMENT_SET_VARIABLES => {
                let mut variable = data.cast::<RetroVariable>().cast_const();
                while !(*variable).key.is_null() {
                    let entry = (text((*variable).key), text((*variable).value));
                    record(|record| record.variables.push(entry));
                    variable = variable.add(1);
                }
                true
            }
            RETRO_ENVIRONMENT_GET_VARIABLE => {
                let variable = &mut *data.cast::<RetroVariable>();
                let key = text(variable.key);
                match settings.variables.iter().find(|(name, _)| *name == key) {
                    Some((_, value)) => {
                        variable.value = value.as_ptr();
                        true
                    }
                    None => false,
                }
            }
            RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE => {
                *data.cast::<bool>() = settings.variables_updated;
                true
            }
            RETRO_ENVIRONMENT_GET_INPUT_BITMASKS => settings.bitmasks,
            RETRO_ENVIRONMENT_SET_GEOMETRY => {
                let geometry = &*data.cast::<RetroGameGeometry>();
                assert_eq!(
                    (geometry.max_width, geometry.max_height),
                    (MAX_WIDTH as c_uint, MAX_HEIGHT as c_uint)
                );
                record(|record| {
                    record
                        .geometries
                        .push((geometry.base_width, geometry.base_height));
                });
                true
            }
            RETRO_ENVIRONMENT_SET_MEMORY_MAPS => {
                let map = &*data.cast::<RetroMemoryMap>();
                let descriptors =
                    std::slice::from_raw_parts(map.descriptors, map.num_descriptors as usize);
                record(|record| {
                    record.memory_maps = descriptors
                        .iter()
                        .map(|descriptor| {
                            assert!(descriptor.addrspace.is_null());
                            assert_eq!(descriptor.disconnect, 0);
                            (
                                descriptor.flags,
                                descriptor.start,
                                descriptor.select,
                                descriptor.len,
                                descriptor.ptr as usize + descriptor.offset,
                            )
                        })
                        .collect();
                });
                let size = retro_get_memory_size(RETRO_MEMORY_SYSTEM_RAM);
                record(|record| record.system_ram_size_while_mapping = Some(size));
                true
            }
            RETRO_ENVIRONMENT_SET_SUPPORT_ACHIEVEMENTS => {
                let supported = *data.cast::<bool>();
                record(|record| record.achievements = Some(supported));
                true
            }
            RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE => {
                (*data.cast::<RetroRumbleInterface>()).set_rumble_state = Some(test_rumble);
                true
            }
            _ => false,
        }
    }
}

/// RetroArch's video callback: records each frame's size, pitch and first
/// pixel.
unsafe extern "C" fn test_video_refresh(
    data: *const c_void,
    width: c_uint,
    height: c_uint,
    pitch: usize,
) {
    assert!(!data.is_null());
    // SAFETY: the core passes `height` lines of `pitch` bytes.
    let pixels = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), pitch * height as usize) };
    let first = pixels[..pitch / width as usize].to_vec();
    record(|record| record.frames.push((width, height, pitch, first)));
}

/// RetroArch's callback for one sound frame at a time, which the core must
/// never use.
unsafe extern "C" fn test_audio_sample(_left: i16, _right: i16) {
    panic!("the core should use the batch callback");
}

/// Takes at most 500 frames of sound at a time, as a frontend may.
unsafe extern "C" fn test_audio_sample_batch(data: *const i16, frames: usize) -> usize {
    assert!(!data.is_null());
    let taken = frames.min(500);
    // SAFETY: the core passes `frames` frames of two samples.
    let samples = unsafe { std::slice::from_raw_parts(data, frames * 2) };
    assert_eq!(samples.len(), frames * 2);
    record(|record| record.audio_batches.push(taken));
    taken
}

/// RetroArch's input poll callback: counts the polls.
unsafe extern "C" fn test_input_poll() {
    record(|record| record.polls += 1);
}

/// Which buttons the pretend player on `port` holds (see `HELD`): all at
/// once as a bitmask, or whether button `id` is held.
unsafe extern "C" fn test_input_state(
    port: c_uint,
    device: c_uint,
    index: c_uint,
    id: c_uint,
) -> i16 {
    assert!(port < 4, "only four players");
    assert_eq!((device, index), (RETRO_DEVICE_JOYPAD, 0));
    let held = lock(&HELD)[port as usize];
    if id == RETRO_DEVICE_ID_JOYPAD_MASK {
        assert!(lock(&SETTINGS).bitmasks);
        held as i16
    } else {
        i16::from(held & 1 << id != 0)
    }
}

/// RetroArch's rumble callback: records the strength of each change to the
/// first player's strong motor.
unsafe extern "C" fn test_rumble(port: c_uint, effect: c_int, strength: u16) -> bool {
    assert_eq!((port, effect), (0, RETRO_RUMBLE_STRONG));
    record(|record| record.rumble.push(strength));
    true
}

/// Starts a test: RetroArch answers as `settings` say, and hands the core its
/// callbacks, in the order RetroArch does.
fn start(settings: Settings) -> MutexGuard<'static, ()> {
    let turn = lock(&TURN);
    *lock(&SETTINGS) = settings;
    *lock(&RECORD) = None;
    *lock(&HELD) = [0; 4];
    retro_set_environment(Some(test_environment));
    retro_set_video_refresh(Some(test_video_refresh));
    retro_set_audio_sample(Some(test_audio_sample));
    retro_set_audio_sample_batch(Some(test_audio_sample_batch));
    retro_set_input_poll(Some(test_input_poll));
    retro_set_input_state(Some(test_input_state));
    retro_init();
    turn
}

/// The settings RetroArch has by default: XRGB8888, version 2 core options
/// and input bitmasks.
fn default_settings() -> Settings {
    Settings {
        accepts_xrgb8888: true,
        options_version: 2,
        bitmasks: true,
        variables: Vec::new(),
        variables_updated: false,
    }
}

/// Loads `rom` as RetroArch does, with its bytes in memory, and returns
/// whether the core took it.
fn load_game(rom: &[u8]) -> bool {
    let game = RetroGameInfo {
        path: c"/games/test.gb".as_ptr(),
        data: rom.as_ptr().cast(),
        size: rom.len(),
        meta: ptr::null(),
    };
    // SAFETY: a game's information with its bytes.
    unsafe { retro_load_game(&raw const game) }
}

/// What the core has handed the pretend RetroArch so far, starting a new
/// record.
fn take_record() -> Record {
    lock(&RECORD).take().unwrap_or_default()
}

/// Runs `frames` frames, as RetroArch does once per frame.
fn run_frames(frames: usize) {
    for _ in 0..frames {
        retro_run();
    }
}

/// The memory RetroArch reads at `id`, copied.
fn memory(id: c_uint) -> Vec<u8> {
    let data = retro_get_memory_data(id);
    let size = retro_get_memory_size(id);
    if size == 0 {
        assert!(data.is_null());
        return Vec::new();
    }
    // SAFETY: the core points to `size` bytes.
    unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) }.to_vec()
}

/// Writes `bytes` to the start of the memory at `id`, as RetroArch does
/// when it loads a `.srm` or `.rtc` file.
fn fill_memory(id: c_uint, bytes: &[u8]) {
    let data = retro_get_memory_data(id);
    assert!(retro_get_memory_size(id) >= bytes.len());
    // SAFETY: the core points to at least `bytes.len()` bytes it lets
    // RetroArch write.
    unsafe { std::slice::from_raw_parts_mut(data.cast::<u8>(), bytes.len()) }
        .copy_from_slice(bytes);
}

/// Ends a test as RetroArch closes a game: unloads it and shuts the core
/// down.
fn finish() {
    retro_unload_game();
    retro_deinit();
}

#[test]
fn the_core_describes_itself() {
    let _turn = start(default_settings());
    assert_eq!(retro_api_version(), 1);
    assert_eq!(retro_get_region(), RETRO_REGION_NTSC);

    let mut info = RetroSystemInfo {
        library_name: ptr::null(),
        library_version: ptr::null(),
        valid_extensions: ptr::null(),
        need_fullpath: true,
        block_extract: true,
    };
    // SAFETY: a struct to fill in.
    unsafe { retro_get_system_info(&raw mut info) };
    assert_eq!(text(info.library_name), "Nubo");
    assert!(text(info.library_version).starts_with(env!("CARGO_PKG_VERSION")));
    assert_eq!(text(info.valid_extensions), "gb|gbc");
    assert!(!info.need_fullpath);
    assert!(!info.block_extract);

    let mut av_info = RetroSystemAvInfo {
        geometry: RetroGameGeometry {
            base_width: 0,
            base_height: 0,
            max_width: 0,
            max_height: 0,
            aspect_ratio: 0.0,
        },
        timing: RetroSystemTiming {
            fps: 0.0,
            sample_rate: 0.0,
        },
    };
    // SAFETY: a struct to fill in.
    unsafe { retro_get_system_av_info(&raw mut av_info) };
    assert_eq!(av_info.geometry.base_width, 160);
    assert_eq!(av_info.geometry.base_height, 144);
    assert_eq!(
        av_info.geometry.max_width, 256,
        "room for a Super Game Boy's border"
    );
    assert_eq!(av_info.geometry.max_height, 224);
    assert!((av_info.geometry.aspect_ratio - 10.0 / 9.0).abs() < 1e-6);
    assert!((av_info.timing.fps - 59.7275).abs() < 1e-4);
    assert_eq!(av_info.timing.sample_rate, 48_000.0);

    let record = take_record();
    assert_eq!(
        record.controller_ports,
        vec![vec![RETRO_DEVICE_JOYPAD]; 4],
        "four ports, each a Game Boy's buttons"
    );
    assert!(
        record
            .controller_types
            .iter()
            .all(|&types| types == CONTROLLER_TYPES.0.as_ptr().addr()),
        "RetroArch keeps the controller types, so they must outlive the call"
    );
    let keys: Vec<&str> = record
        .options
        .iter()
        .map(|(key, _, _)| key.as_str())
        .collect();
    assert_eq!(
        keys,
        [
            "nubo_model",
            "nubo_palette",
            "nubo_sgb_colors",
            "nubo_sgb_border",
            "nubo_rtc"
        ]
    );
    for (key, default, values) in &record.options {
        assert_eq!(values.first(), Some(default), "{key}");
    }
    finish();
}

#[test]
fn a_game_plays_in_retroarchs_order_with_one_frame_and_its_sound_per_run() {
    let _turn = start(default_settings());
    assert_eq!(retro_serialize_size(), 0, "no game yet");
    assert!(load_game(&cartridge(ROM_ONLY, 0, false, TONE)));

    let record = take_record();
    assert_eq!(record.pixel_formats, [RETRO_PIXEL_FORMAT_XRGB8888]);
    let names: Vec<&str> = record
        .input_descriptors
        .iter()
        .map(|(_, name)| name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "B",
            "Select",
            "Start",
            "D-Pad Up",
            "D-Pad Down",
            "D-Pad Left",
            "D-Pad Right",
            "A"
        ]
    );
    for port in 0..4 {
        assert_eq!(
            record
                .descriptor_ports
                .iter()
                .filter(|&&p| p == port)
                .count(),
            8,
            "the buttons of port {port}"
        );
    }
    let lasting_names: Vec<usize> = BUTTONS
        .iter()
        .map(|(_, _, name)| name.as_ptr().addr())
        .collect();
    assert!(
        record
            .descriptor_names
            .iter()
            .all(|name| lasting_names.contains(name)),
        "RetroArch keeps the names, so they must outlive the call"
    );
    assert_eq!(record.achievements, Some(true));
    assert!(!record.memory_maps.is_empty());
    assert_eq!(record.system_ram_size_while_mapping, Some(0x2000));

    for frame in 0..300 {
        retro_run();
        let record = take_record();
        assert_eq!(record.polls, 1, "frame {frame}");
        assert_eq!(record.frames.len(), 1, "frame {frame}");
        let (width, height, pitch, _) = &record.frames[0];
        assert_eq!((*width, *height, *pitch), (160, 144, 640));
        let sound: usize = record.audio_batches.iter().sum();
        assert!(
            (790..=820).contains(&sound),
            "frame {frame}: {sound} frames of sound"
        );
    }
    finish();
}

#[test]
fn a_frontend_without_xrgb8888_gets_0rgb1555() {
    let _turn = start(Settings {
        accepts_xrgb8888: false,
        ..default_settings()
    });
    assert!(load_game(&cartridge(ROM_ONLY, 0, false, JOYPAD)));
    assert_eq!(
        take_record().pixel_formats,
        [RETRO_PIXEL_FORMAT_XRGB8888, RETRO_PIXEL_FORMAT_0RGB1555]
    );
    retro_run();
    let record = take_record();
    let (width, height, pitch, first) = &record.frames[0];
    assert_eq!((*width, *height, *pitch), (160, 144, 320));
    // The top-left pixel is white, 0x7FFF.
    assert_eq!(u16::from_ne_bytes([first[0], first[1]]), 0x7FFF);
    finish();
}

#[test]
fn buttons_reach_the_game_with_and_without_bitmasks() {
    for bitmasks in [true, false] {
        let _turn = start(Settings {
            bitmasks,
            ..default_settings()
        });
        assert!(load_game(&cartridge(ROM_ONLY, 0, false, JOYPAD)));
        run_frames(200);
        lock(&HELD)[0] = 1 << 7 | 1 << 8; // right and A
        run_frames(2);
        let work_ram = memory(RETRO_MEMORY_SYSTEM_RAM);
        assert_eq!(work_ram.len(), 0x2000);
        assert_eq!(work_ram[0] & 0x0F, 0b1110, "right (bitmasks: {bitmasks})");
        assert_eq!(work_ram[1] & 0x0F, 0b1110, "A (bitmasks: {bitmasks})");
        finish();
    }
}

#[test]
fn battery_saves_go_through_the_save_ram_retroarch_keeps() {
    let _turn = start(default_settings());
    assert!(load_game(&cartridge(
        MBC1_RAM_BATTERY,
        RAM_8_KIB,
        false,
        RAM
    )));
    assert_eq!(retro_get_memory_size(RETRO_MEMORY_SAVE_RAM), 0x2000);
    assert_eq!(retro_get_memory_size(RETRO_MEMORY_RTC), 0);
    fill_memory(RETRO_MEMORY_SAVE_RAM, &[0x5A]);
    let address = retro_get_memory_data(RETRO_MEMORY_SAVE_RAM);

    run_frames(200);
    assert_eq!(
        memory(RETRO_MEMORY_SYSTEM_RAM)[0],
        0x5A,
        "the game read the save"
    );
    let counter = memory(RETRO_MEMORY_SAVE_RAM)[1];
    run_frames(1);
    assert_ne!(
        memory(RETRO_MEMORY_SAVE_RAM)[1],
        counter,
        "the game's writes arrive"
    );
    assert_eq!(
        retro_get_memory_data(RETRO_MEMORY_SAVE_RAM),
        address,
        "it never moves"
    );
    finish();
}

#[test]
fn games_without_a_battery_give_retroarch_nothing_to_save() {
    let _turn = start(default_settings());
    assert!(load_game(&cartridge(ROM_ONLY, 0, false, TONE)));
    assert!(memory(RETRO_MEMORY_SAVE_RAM).is_empty());
    assert!(memory(RETRO_MEMORY_RTC).is_empty());
    assert!(memory(42).is_empty());
    finish();
}

#[test]
fn save_states_load_back_and_go_on_alike() {
    let _turn = start(default_settings());
    assert!(load_game(&cartridge(
        MBC1_RAM_BATTERY,
        RAM_8_KIB,
        true,
        RAM
    )));
    run_frames(50);
    let size = retro_serialize_size();
    assert!(size > 0);
    let mut state = vec![0; size];
    // SAFETY: a buffer of `size` bytes.
    assert!(unsafe { retro_serialize(state.as_mut_ptr().cast(), size) });
    take_record();
    run_frames(100);
    let first = take_record();

    // SAFETY: a buffer of `size` bytes.
    assert!(unsafe { retro_unserialize(state.as_ptr().cast(), size) });
    run_frames(100);
    let second = take_record();
    assert!(first.frames == second.frames);
    assert_eq!(first.audio_batches, second.audio_batches);
    assert_eq!(retro_serialize_size(), size);

    // SAFETY: a buffer too small for a state, and garbage to load.
    unsafe {
        assert!(!retro_serialize(state.as_mut_ptr().cast(), 10));
        assert!(!retro_unserialize([7u8; 64].as_ptr().cast(), 64));
        assert!(!retro_serialize(ptr::null_mut(), size));
        assert!(!retro_unserialize(ptr::null(), size));
    }
    finish();
}

#[test]
fn cheats_from_retroarchs_list_work() {
    let _turn = start(default_settings());
    assert!(load_game(&cartridge(ROM_ONLY, 0, false, CHEATS)));
    run_frames(200);
    // SAFETY: C strings, and a null one RetroArch shouldn't send.
    unsafe {
        retro_cheat_set(0, true, c"014200C1+772-00F".as_ptr());
        retro_cheat_set(1, true, c"not a code".as_ptr());
        retro_cheat_set(2, true, ptr::null());
    }
    run_frames(2);
    let work_ram = memory(RETRO_MEMORY_SYSTEM_RAM);
    assert_eq!((work_ram[0], work_ram[1]), (0x42, 0x77));

    retro_cheat_reset();
    run_frames(2);
    let work_ram = memory(RETRO_MEMORY_SYSTEM_RAM);
    assert_eq!((work_ram[0], work_ram[1]), (0x00, ROM_BYTE_AT_0200));
    finish();
}

#[test]
fn the_options_retroarch_keeps_pick_the_model_and_the_palette() {
    let variable = |key: &str, value: &CStr| (String::from(key), CString::from(value));
    let turn = start(Settings {
        variables: vec![
            variable("nubo_model", c"CGB-C"),
            variable("nubo_palette", c"green"),
            variable("nubo_rtc", c"emulated"),
        ],
        ..default_settings()
    });
    assert!(load_game(&cartridge(ROM_ONLY, 0, false, JOYPAD)));
    assert_eq!(
        retro_get_memory_size(RETRO_MEMORY_SYSTEM_RAM),
        0x8000,
        "a Game Boy Color"
    );
    finish();
    drop(turn);

    let _turn = start(Settings {
        variables: vec![variable("nubo_palette", c"green")],
        ..default_settings()
    });
    assert!(load_game(&cartridge(ROM_ONLY, 0, false, JOYPAD)));
    assert_eq!(
        retro_get_memory_size(RETRO_MEMORY_SYSTEM_RAM),
        0x2000,
        "a Game Boy"
    );
    retro_run();
    let (_, _, _, first) = take_record().frames.remove(0);
    assert_eq!(
        u32::from_ne_bytes(first.try_into().unwrap()),
        0x009B_BC0F,
        "the green palette's lightest color"
    );

    lock(&SETTINGS).variables = vec![variable("nubo_palette", c"gray")];
    lock(&SETTINGS).variables_updated = true;
    retro_run();
    let (_, _, _, first) = take_record().frames.remove(0);
    assert_eq!(
        u32::from_ne_bytes(first.try_into().unwrap()),
        0x00FF_FFFF,
        "white"
    );
    finish();
}

#[test]
fn a_super_game_boy_border_changes_the_pictures_size() {
    let variable = |key: &str, value: &CStr| (String::from(key), CString::from(value));
    let _turn = start(Settings {
        variables: vec![variable("nubo_model", c"SGB")],
        ..default_settings()
    });
    assert!(load_game(&cartridge(ROM_ONLY, 0, false, JOYPAD)));
    take_record();
    retro_run();
    let record = take_record();
    assert_eq!(record.geometries, [(256, 224)]);
    let (width, height, pitch, _) = &record.frames[0];
    assert_eq!((*width, *height, *pitch), (256, 224, 1024));

    lock(&SETTINGS).variables = vec![
        variable("nubo_model", c"SGB"),
        variable("nubo_sgb_border", c"disabled"),
    ];
    lock(&SETTINGS).variables_updated = true;
    retro_run();
    let record = take_record();
    assert_eq!(record.geometries, [(160, 144)]);
    let (width, height, pitch, _) = &record.frames[0];
    assert_eq!((*width, *height, *pitch), (160, 144, 640));
    finish();
}

#[test]
fn the_next_game_after_a_bordered_one_announces_its_size_again() {
    for _ in 0..2 {
        let _turn = start(Settings {
            variables: vec![(String::from("nubo_model"), CString::from(c"SGB"))],
            ..default_settings()
        });
        assert!(load_game(&cartridge(ROM_ONLY, 0, false, JOYPAD)));
        take_record();
        retro_run();
        assert_eq!(take_record().geometries, [(256, 224)]);
        finish();
    }
}

#[test]
fn older_frontends_get_the_options_in_the_form_they_know() {
    {
        let _turn = start(Settings {
            options_version: 1,
            ..default_settings()
        });
        let record = take_record();
        assert_eq!(record.options.len(), 5);
        assert!(record.variables.is_empty());
        finish();
    }
    let _turn = start(Settings {
        options_version: 0,
        ..default_settings()
    });
    let record = take_record();
    assert!(record.options.is_empty());
    let (key, value) = &record.variables[4];
    assert_eq!(key, "nubo_rtc");
    assert_eq!(value, "Cartridge clock; real|emulated");
    assert!(record.variables[0].1.starts_with("Model; auto|DMG-0|DMG|"));
    finish();
}

#[test]
fn the_memory_map_points_at_buffers_that_hold_the_games_memory() {
    let _turn = start(default_settings());
    assert!(load_game(&cartridge(
        MBC1_RAM_BATTERY,
        RAM_8_KIB,
        true,
        RAM
    )));
    let maps = take_record().memory_maps;
    fill_memory(RETRO_MEMORY_SAVE_RAM, &[0x6B]);
    run_frames(200);
    let read = |address: usize| {
        let (_, start, _, length, pointer) = maps
            .iter()
            .find(|(_, start, select, _, _)| (start ^ address) & select == 0)
            .copied()
            .unwrap();
        assert!(address - start < length);
        // SAFETY: the descriptor points to `length` bytes the core keeps.
        unsafe { *(pointer as *const u8).add(address - start) }
    };
    assert_eq!(read(0xC000), 0x6B, "work RAM");
    assert_eq!(read(0xA000), 0x6B, "cartridge RAM");
    assert_eq!(read(0x0147), MBC1_RAM_BATTERY, "the header");
    let flags = |start: usize| maps.iter().find(|map| map.1 == start).unwrap().0;
    assert_eq!(flags(0x0000), RETRO_MEMDESC_CONST);
    assert_eq!(flags(0xC000), RETRO_MEMDESC_SYSTEM_RAM);
    assert_eq!(flags(0xA000), RETRO_MEMDESC_SAVE_RAM);
    assert_eq!(
        flags(0x1_0000),
        RETRO_MEMDESC_SYSTEM_RAM,
        "Game Boy Color banks 2-7"
    );
    assert_eq!(flags(0xFF00), RETRO_MEMDESC_CONST, "the hardware registers");
    finish();
}

#[test]
fn what_retroarch_writes_through_its_pointers_reaches_the_game() {
    let _turn = start(default_settings());
    assert!(load_game(&cartridge(ROM_ONLY, 0, false, CHEATS)));
    let maps = take_record().memory_maps;
    run_frames(200);

    // RETRO_MEMORY_SYSTEM_RAM, as RetroArch's memory editor writes it.
    let system_ram = retro_get_memory_data(RETRO_MEMORY_SYSTEM_RAM).cast::<u8>();
    // SAFETY: the core points to its work RAM, 8 KiB on a Game Boy.
    unsafe { system_ram.add(0x100).write(0x42) };
    run_frames(2);
    assert_eq!(memory(RETRO_MEMORY_SYSTEM_RAM)[0], 0x42);

    // The memory map, as cheats from RetroArch's memory search write it.
    let &(_, start, _, _, pointer) = maps.iter().find(|map| map.1 == 0xC000).unwrap();
    // SAFETY: the descriptor points to the work RAM's first 8 KiB.
    unsafe { (pointer as *mut u8).add(0xC100 - start).write(0x43) };
    run_frames(2);
    assert_eq!(memory(RETRO_MEMORY_SYSTEM_RAM)[0], 0x43);
    finish();
}

#[test]
fn a_restart_on_another_model_gives_retroarch_that_models_memory_map() {
    let has_color_work_ram = |maps: &[(u64, usize, usize, usize, usize)]| {
        maps.iter().any(|&(_, start, _, _, _)| start == 0x1_0000)
    };
    let _turn = start(default_settings());
    assert!(load_game(&cartridge(ROM_ONLY, 0, false, JOYPAD)));
    assert!(
        !has_color_work_ram(&take_record().memory_maps),
        "a Game Boy"
    );

    let mut settings = lock(&SETTINGS);
    settings.variables = vec![(String::from("nubo_model"), CString::from(c"CGB-E"))];
    settings.variables_updated = true;
    drop(settings);
    // RetroArch's menu pauses the core, so no frame runs between the
    // option's change and the restart.
    retro_reset();
    let record = take_record();
    assert!(has_color_work_ram(&record.memory_maps), "a Game Boy Color");
    // The core is free while RetroArch handles the map, so asking it for
    // memory then doesn't deadlock.
    assert_eq!(record.system_ram_size_while_mapping, Some(0x8000));
    assert_eq!(retro_get_memory_size(RETRO_MEMORY_SYSTEM_RAM), 0x8000);
    finish();
}

#[test]
fn rumble_cartridges_rumble_until_the_game_is_unloaded() {
    let _turn = start(default_settings());
    assert!(load_game(&cartridge(MBC5_RUMBLE, 0, false, RUMBLE)));
    run_frames(200);
    retro_unload_game();
    let rumble = take_record().rumble;
    // Only changes are sent: on from the frame the game switched the motor
    // on in (for part of it), then full, then off when the game ends.
    assert_eq!(rumble.len(), 3, "{rumble:?}");
    assert!(rumble[0] > 0 && rumble[0] < u16::MAX);
    assert_eq!(rumble[1..], [u16::MAX, 0]);
    retro_deinit();
}

#[test]
fn restarting_keeps_the_game_running() {
    let _turn = start(default_settings());
    assert!(load_game(&cartridge(
        MBC1_RAM_BATTERY,
        RAM_8_KIB,
        false,
        RAM
    )));
    fill_memory(RETRO_MEMORY_SAVE_RAM, &[0x5A]);
    run_frames(200);
    retro_reset();
    assert_eq!(memory(RETRO_MEMORY_SAVE_RAM)[0], 0x5A);
    run_frames(200);
    assert_eq!(memory(RETRO_MEMORY_SYSTEM_RAM)[0], 0x5A);
    finish();
}

#[test]
fn what_isnt_a_game_does_not_load() {
    let _turn = start(default_settings());
    assert!(!load_game(&[0; 0x40]));
    let mut unsupported = cartridge(ROM_ONLY, 0, false, TONE);
    unsupported[0x0147] = 0x22;
    assert!(!load_game(&unsupported));
    // SAFETY: no game, and a game without its bytes.
    unsafe {
        assert!(!retro_load_game(ptr::null()));
        let empty = RetroGameInfo {
            path: c"/games/missing.gb".as_ptr(),
            data: ptr::null(),
            size: 0,
            meta: ptr::null(),
        };
        assert!(!retro_load_game(&raw const empty));
    }
    assert!(!retro_load_game_special(0, ptr::null(), 0));
    retro_run();
    retro_reset();
    retro_cheat_reset();
    assert_eq!(retro_serialize_size(), 0);
    assert!(
        take_record().frames.is_empty(),
        "nothing to show without a game"
    );
    finish();
}

#[test]
fn a_panic_fails_the_call_instead_of_crossing_into_c() {
    assert_eq!(guard("test", 7, || panic!("on purpose")), 7);
    assert_eq!(guard("test", 7, || 8), 8);
}

#[test]
fn the_second_port_plays_the_second_player_of_a_super_game_boy() {
    let variable = |key: &str, value: &CStr| (String::from(key), CString::from(value));
    let _turn = start(Settings {
        variables: vec![variable("nubo_model", c"SGB")],
        ..default_settings()
    });
    assert!(load_game(&cartridge(ROM_ONLY, 0, false, JOYPAD)));
    lock(&HELD)[1] = 1 << 8; // A on the second port
    retro_run();
    let held = lock(&CORE).as_ref().map(Core::other_players_held_for_tests);
    assert_eq!(held, Some([1 << 8, 0, 0]));
    finish();
}
