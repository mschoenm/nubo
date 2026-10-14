# nubo-libretro

Nubo as a libretro core: the dynamic library RetroArch loads to run Game Boy and Game Boy Color games. `src/lib.rs` lists the modules; each file's docs explain it. `docs/RETROARCH_PLAN.md` has the plan it follows and what is left. This guide explains how the parts fit together.

```bash
cargo test -p nubo-libretro       # The Core's tests, the exported functions in RetroArch's order, the exports and the info file
cargo build --release -p nubo-libretro   # target/release/libnubo_libretro.so (.dylib, nubo_libretro.dll)
cargo xtask libretro              # The zip players install, in target/bundle/ (xtask/src/libretro.rs)
```

## How It Fits Together

- **Safe and unsafe apart.** `core.rs` does everything with a safe `Core`; `ffi.rs` is the only `unsafe` code: the exported `retro_*` functions, the bindings written by hand from `libretro.h`, and turning pointers into slices. Keep it that way: new behavior goes into `Core`, with a test there, and `ffi.rs` only passes values through.
- **What RetroArch looks up.** RetroArch refuses a core that lacks any function of its `CORE_SYMBOLS` list (in its `runloop.c`), so all 25 are exported from the start; `tests/exports.rs` reads the built library and checks the list, and that nothing else named `retro_` is exported.
- **Time.** RetroArch calls `retro_run` once per frame of its screen; `Core::run` calls `Emulator::run_frame`, which runs to the next multiple of 70,224 dots since power-on, so calls never drift and a loaded state goes on exactly like the run it was saved from.
- **Battery saves** are RetroArch's: it fills the core's save RAM and clock buffers after the game loads and writes them to `.srm` and `.rtc`. The buffers never move or change size, and `Core` keeps them in step with the emulator in both directions (see `core.rs`). The `.rtc` is always the Nubo app's 17-byte layout on the wall clock, whichever clock the game follows.
- **Save states** are padded to one size (the first state's length plus a margin, `STATE_SIZE_MARGIN`), as run-ahead, rewind and netplay need. A test measures how much a state's length varies. The info file says `savestate_features = "deterministic"`: RetroArch only offers run-ahead and netplay for such cores. Everything is deterministic except an MBC3 clock on real time, which reads the wall clock; the Cartridge clock option tells netplay players to pick emulated time.
- **Memory for RetroAchievements** is copied after every frame into buffers the core owns, laid out as rcheevos expects (`MemorySnapshot::regions`), and handed over with `RETRO_ENVIRONMENT_SET_MEMORY_MAPS` when the game loads and again after each restart, which can change the model and so the map. Each region is a power of two long and starts at a multiple of its length, with an explicit `select`, so RetroArch's descriptor processing accepts it.
- **Options** are defined once in `options.rs` and handed to RetroArch in the newest form it understands (core options v2, v1 or the old variables). The model and the clock take effect on restart, rebasing the clock so it neither stops nor jumps.

## Keep in Mind

- **The info file** (`nubo_libretro.info`) is what RetroArch reads before loading the core; without it RetroArch turns rewind and run-ahead off. `tests/info_file.rs` checks that its `display_version` is Nubo's version, so the release that bumps the version updates it too.
- **No panic crosses into C.** Every exported function runs under `guard`, which catches a panic, logs it and fails the call.
- **Callbacks are global**, as a dynamic library's are: the tests in `ffi/tests.rs` take turns through a lock and play RetroArch's part with test callbacks.
- **Real RetroArch.** `.github/workflows/libretro.yml` plays a game in the RetroArch Ubuntu packages, headless, with run-ahead, rewind and an auto-saved state. To do the same locally, see its "Play a game in RetroArch" step: RetroArch needs the null drivers and a D-Bus session (`dbus-run-session`), or its GameMode support aborts.
