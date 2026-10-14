//! Checks that the built library exports every function RetroArch looks up
//! when it loads a core: it refuses a core that lacks one.

use object::{NameOrOrdinal, Object};
use std::path::PathBuf;

/// The functions RetroArch looks up, from `CORE_SYMBOLS` in its `runloop.c`.
const CORE_SYMBOLS: [&str; 25] = [
    "retro_init",
    "retro_deinit",
    "retro_api_version",
    "retro_get_system_info",
    "retro_get_system_av_info",
    "retro_set_environment",
    "retro_set_video_refresh",
    "retro_set_audio_sample",
    "retro_set_audio_sample_batch",
    "retro_set_input_poll",
    "retro_set_input_state",
    "retro_set_controller_port_device",
    "retro_reset",
    "retro_run",
    "retro_serialize_size",
    "retro_serialize",
    "retro_unserialize",
    "retro_cheat_reset",
    "retro_cheat_set",
    "retro_load_game",
    "retro_load_game_special",
    "retro_unload_game",
    "retro_get_region",
    "retro_get_memory_data",
    "retro_get_memory_size",
];

/// The dynamic library Cargo built next to this test, in `target/<profile>/`.
fn library() -> PathBuf {
    let test = std::env::current_exe().unwrap();
    let deps = test.parent().unwrap();
    let name = format!(
        "{}nubo_libretro{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    [deps.join(&name), deps.parent().unwrap().join(&name)]
        .into_iter()
        .find(|path| path.exists())
        .unwrap_or_else(|| panic!("{name} wasn't built next to {}", test.display()))
}

#[test]
fn the_library_exports_every_function_retroarch_looks_up() {
    let bytes = std::fs::read(library()).unwrap();
    let file = object::File::parse(bytes.as_slice()).unwrap();
    let exported: Vec<String> = file
        .exports()
        .unwrap()
        .filter_map(|export| match export.unwrap().name() {
            NameOrOrdinal::Name(name) => Some(String::from_utf8_lossy(name).into_owned()),
            NameOrOrdinal::Ordinal(_) => None,
        })
        // Mach-O puts an underscore before C names.
        .map(|name| name.strip_prefix('_').map(String::from).unwrap_or(name))
        .collect();
    let missing: Vec<&str> = CORE_SYMBOLS
        .into_iter()
        .filter(|symbol| !exported.iter().any(|name| name == symbol))
        .collect();
    assert!(missing.is_empty(), "missing exports: {missing:?}");
    let extra: Vec<&String> = exported
        .iter()
        .filter(|name| name.starts_with("retro_") && !CORE_SYMBOLS.contains(&name.as_str()))
        .collect();
    assert!(
        extra.is_empty(),
        "exports RetroArch doesn't know: {extra:?}"
    );
}
