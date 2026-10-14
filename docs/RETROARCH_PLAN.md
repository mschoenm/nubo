# Plan: a RetroArch (libretro) core

[RetroArch](https://www.retroarch.com/) is a frontend that runs emulators packaged as libretro cores:
dynamic libraries with a small C interface defined by `libretro.h`. It reaches players Nubo's own app
doesn't: retro handhelds, Android, consoles running Linux, and people who keep every system in one
place. It also gives them features Nubo would otherwise have to build itself, such as
RetroAchievements, run-ahead (which runs frames ahead to hide input lag), rewind, netplay, shaders,
recording and button remapping.

The core itself stays thin. RetroArch already provides save state slots, fast forward, the cheats menu,
screenshots, playlists and box art, so none of the Iced app's versions of these are needed. The core
only has to:

- turn RetroArch's calls into calls on `nubo_core::Emulator`
- hand over frames, sound, battery saves and save states
- offer a few options

## Status

**Phases 1 to 5 are built** (`frontends/nubo-libretro`, with its `AGENTS.md`); Phase 6 is left. The maintainer asked
to build the core straight away, so Phase 0's review of this plan with Codex didn't happen as a pull request of its
own: the plan's open review findings were handled in the code instead (see below), and the core's pull request is
where the review happens.

Checked in a real RetroArch (1.18, the Ubuntu package, headless): the core loads, plays, saves the battery RAM and
the clock, saves and loads states (a loaded state goes on pixel for pixel like the run it was saved from), keeps
run-ahead (one instance and two) and rewind on, applies the options, and RetroArch accepts its memory maps and
achievements support. `.github/workflows/libretro.yml` repeats that check. Not checked yet, since they need a person
with a screen, a controller or an account: playing by hand on Windows and macOS, achievements with a
retroachievements.org account, and rumble on a controller. The maintainer has what these need and will test them
by hand; when isn't set yet.

**Phase 6 is on hold** until the maintainer picks it up, which they plan to do soon: nothing goes to RetroArch or
libretro before then.

Where the code differs from the plan below:

- **Run-ahead and netplay need `savestate_features = "deterministic"`.** RetroArch only offers them for such cores,
  and turns rewind and run-ahead off for a core without an info file. The plan picked `"serialized"` because a clock
  on real time reads the wall clock, but that would have turned run-ahead off for every game. Everything else is
  deterministic, Gambatte (whose clock follows real time too) declares `"deterministic"`, and SameBoy declares
  nothing, which RetroArch reads the same way; the Cartridge clock option tells netplay players to pick emulated
  time.
- **The clock's source is stored in save states** (`rtc_clock` in the MBC3's state; the clock of a state from before
  loads as it is, on the loading emulator's clock, as it did before), and nubo-core does the rebasing: `Emulator::rtc_state(clock)` and `set_rtc_state(state, clock)` give
  and take a clock state on either source, and loading a state made on the other source rebases it.
- **Rumble is a level, not a hook:** `Emulator::take_rumble_level` is the share of the time the motor was on since the
  last call, which games use to rumble more gently, and the core passes it on as the strength.
- **Save states are padded to the first state's length plus 16 KiB.** No length bound is proven; a test measures how
  much states vary (well under 2 KiB over thousands of states of busy test programs).
- **Saving and loading got faster** for run-ahead: lists of bytes are written and read in one go, and the power-on
  state a load starts from is made once per game. Saving went from about 1 ms to 60 µs and loading from 5-7 ms to
  0.45 ms (a frame takes about 0.5 ms), with the same bytes stored.
- **`CartridgeEffects: Send`**, so the emulator is `Send` and the core can keep it in a `static Mutex`.
- **Pixel format:** when the frontend refuses XRGB8888, the core converts to the default 0RGB1555.
- **Cheats:** setting a cheat first removes every code its index had, and a disabled cheat registers nothing (the
  last review finding on mschoenm/nubo#37). A restart keeps the cheats.
- **Restart** takes on the model and clock the options pick now (another finding there).
- **RetroArch's memory writes reach the game** (its memory editor, cheat search and RetroArch-handler cheats), not
  only the reads Phase 4 asked for: before each frame, the core writes the bytes RetroArch changed in its copies into
  the emulator with `Emulator::memory_mut`, which catches the lazy components up first. Checked in RetroArch 1.18
  with a game-specific cheat file. The hardware registers stay read-only (`RETRO_MEMDESC_CONST`).

Left: the maintainer's tests by hand (above), the AppStream release notes entry when the next version ships (the
`release` skill writes it), and Phase 6 (on hold).

## Decisions for the maintainer before starting

1. **An exception to "no `unsafe` code".** A libretro core exports C functions
   (`#[unsafe(no_mangle)] pub extern "C" fn retro_run()`) and reads raw pointers from RetroArch. The
   `unsafe_code` lint covers both, and the root `Cargo.toml` sets `unsafe_code = "forbid"`, which
   nothing can override. The plan is:
   - The new crate doesn't use `[lints] workspace = true`. It copies the workspace's lint table, with
     `unsafe_code = "deny"`.
   - All `unsafe` stays in one module (`ffi.rs`) under `#[expect(unsafe_code, reason = "...")]`.
   - The "No `unsafe`" rule in AGENTS.md names this one exception.

   A crate of libretro bindings wouldn't avoid this, because the exported functions are still
   `no_mangle`.
2. **Self-hosted first, official later?** The plan publishes the core with Nubo's own releases first
   (Phase 5). Getting it into RetroArch's own core downloader (Phase 6) means working with the libretro
   team and keeping a build on their CI. It is worth it, but it is outward-facing and long-term.
3. **Options and defaults.** By default the model follows the desktop app (Game Boy Color games on
   CGB-E, Super Game Boy games on SGB, others on DMG), with a gray palette, the Super Game Boy's
   border shown (the picture is then 256×224, and RetroArch is told whenever its size changes) and the
   cartridge clock on real time. Change these if needed.

## References

- `libretro.h` from [libretro/libretro-common](https://github.com/libretro/libretro-common)
  (`include/libretro.h`, MIT license) is the interface. Write the bindings by hand: the functions,
  structs and constants this core uses, in `ffi.rs`. No bindgen (it needs libclang) and no new
  dependency. Credit `libretro.h` at the top of the file, in `NOTICE` and in the README's Credits, as
  AGENTS.md asks for ported definitions.
- SameBoy's libretro core (`libretro/libretro.c` in [LIJI32/SameBoy](https://github.com/LIJI32/SameBoy),
  Expat license, already credited) is the closest example: a cycle-accurate Game Boy core with save RAM,
  RTC, memory maps for achievements and model options. Read it for how things are done, not to port.
  Gambatte's libretro core is GPL-2.0-only: only look at what it does, never copy from it (AGENTS.md).
- [rcheevos](https://github.com/RetroAchievements/rcheevos) `src/rc_libretro.c` and `rc_consoles.c`
  show how RetroAchievements finds Game Boy memory.
- The [libretro docs](https://docs.libretro.com/development/cores/developing-cores/) cover the call
  order and the core info file.

## What the core uses already

- `Emulator::new(model)`, `load_rom(bytes, Box<dyn CartridgeEffects>)`, `read_rom_header`
  (`CartridgeHeader` has `supports_cgb`, `has_battery`, `cartridge_type` and the other header fields).
- `set_sample_rate`. Exactly that many samples come out per emulated second, as `f32`, with left and
  right in separate slices.
- `frame_buffer()` and `take_frame()`: 160×144 RGBA. Original Game Boy games are drawn in
  `DMG_SHADES`, and the frontend swaps those for a palette.
- `press_key` and `release_key` with `Key`.
- `register_gameshark_cheat`, `register_gamegenie_cheat`, `unregister_cheat`, `validate_gameshark_code`
  and `validate_gamegenie_code`.
- `save_state()` and `load_state()`. A loaded state goes on exactly like the run it was saved from,
  which `cargo benchmark --check-save-states` checks.
- `set_cartridge_ram()` and `cartridge_ram()`. `CartridgeEffects::save_ram` is called after any call in
  which the game wrote battery RAM.
- `CartridgeEffects::current_time_millis` and `RtcClock::EmulatedTime` for the MBC3 clock.

## Phase 0: review the plan with Codex (one pull request)

Before any code, bring this plan up to date and have it reviewed thoroughly. Codex's reviews of the
pull request that added it kept finding real problems in how the libretro interface has to be used:
- the frame deadline across save states
- the exported symbols
- the clock across clock sources and resets
- the pixel format
- the achievements opt-in

The plan was merged before those rounds ran out.

1. Read the plan against the code as it is then (after the restructuring), against `libretro.h`,
   against RetroArch's loader (`runloop.c`) and against SameBoy's `libretro.c`, and update it.
2. Read the review comments on the pull request that added this plan (mschoenm/nubo#37). Make sure
   every finding is in the plan, including any posted after it was merged.
3. Open a docs-only pull request with the updated plan and ask Codex to review it (`@codex review`).
4. Fix what Codex finds, push, and ask again. Repeat until a review comes back without findings and
   you agree with the plan too. A finding you disagree with gets an answer on its thread instead of a
   change.
5. Merge it the usual way (the `merge` skill). Phase 1 starts after that.

**Done when:** a Codex review of the plan comes back clean, the maintainer has read the result, and it
is merged.

## Phase 1: core additions (one pull request)

The RetroArch core comes after the whole restructuring and the Shootout plan's Parts 1 to 3 (see
"Order and dependencies" in [RESTRUCTURE_PLAN.md](RESTRUCTURE_PLAN.md)).

Steps 1 and 2 of [RESTRUCTURE_PLAN.md](RESTRUCTURE_PLAN.md) come first. Step 2 added
`Emulator::run_frame(&mut left, &mut right)` (item 1 below, done: it appends the samples to the
caller's buffers, and `DOTS_PER_FRAME` is public) and moved the palettes and the model rule into
`nubo-frontend` (item 4, done), so this phase only does what is still left after them.

While the screen is off, the PPU still hands over a frame every 70,224 dots, holding what it last
drew, so `run_frame` normally finishes one frame per call either way, as the Nubo app sees too. A
call can overshoot the boundary by one CPU step (up to about 4,100 dots with a general-purpose VRAM
DMA), so when turning the screen on or off puts a frame inside that overshoot, one call finishes no
frame and the next two; the core should then show the last frame again (`retro_video_refresh` with
`NULL`).

All of these add to the public API without changing what the emulator does.
`cargo benchmark --compare`, `cargo test-roms` and `cargo save-states` must give the same results as
before.

1. **Run one frame's worth of time.** RetroArch calls `retro_run` once per frame and expects about
   1/59.7275 s of emulation: 70,224 ticks of the 4 MHz clock (17,556 M-cycles at normal speed, 35,112
   in double speed).
   - Add something like `Emulator::run_frame()`. It runs until the PPU's dot count since power-on (the
     counter `RtcClock::EmulatedTime` uses, which save states keep) reaches the next frame boundary,
     and hands back every sample made in that time. Frame boundaries are the multiples of 70,224 dots
     since power-on.
   - A step can't always stop exactly on a boundary: an instruction takes several M-cycles, and a
     halted CPU skips ahead in bulk stretches. If each call ran 70,224 dots from wherever the last one
     stopped, every overshoot would be added again and the game would run fast, most of all while the
     CPU is halted or the screen is off. Running to the next boundary instead means a call that
     overshoots makes the next one that much shorter.
   - The boundary comes from the dot count alone, so `run_frame` keeps no state of its own. A loaded
     save state (rewind, run-ahead, netplay) is followed by a frame that ends at the next boundary
     after the loaded dot count, exactly as it did in the run the state was saved from. A deadline
     kept in a field instead would have to be stored in save states, or a load would leave it in the
     future and the next call would emulate several frames.
   - Cap the halted CPU's bulk stretches at the boundary, so an overshoot is never more than one
     instruction.
   - It must count time, not frames, because no frames come while the game has the screen off.
   - Calling `step()` in a loop would end the bulk shortcuts on every call (see "Lazy components,
     exact results" in AGENTS.md), so it has to be one call.
   - Today's audio queue is filled up to `MAX_AUDIO_BUFFER_SIZE` (512) samples. At 48 kHz one frame is
     about 804 samples, so check that nothing is dropped.
   - While the screen stays on, each run of 70,224 dots contains exactly one VBlank, so every call
     normally has exactly one new frame. Find out what the completed frame holds while the screen is
     off and match what the Iced app shows.
   - Tests:
     - After N calls the dot count is within one CPU step (an instruction, an interrupt dispatch or a
       general-purpose VRAM DMA) past the N-th boundary and never drifts
       further, also while the CPU is halted and with the screen off.
     - Samples per emulated second equal the sample rate.
     - Saving a state after some calls, running on, loading it and running the same number of calls
       again gives the same frames, samples and dot counts.
2. **Set the clock state after loading.** RetroArch fills in the saved clock only after
   `retro_load_game`, but `CartridgeEffects::load_rtc_state` is only asked at `load_rom`. Add
   `Emulator::rtc_state()` and `Emulator::set_rtc_state(&RtcState)` (or similar), next to
   `set_cartridge_ram`.
   - **Keep the clock right across clock sources.** `RtcState::base_timestamp` is a time on whichever
     clock the cartridge follows, and `Mbc3CartridgeMapper::catch_up` subtracts it from the current
     time on that clock. A clock state from the other source would freeze the game's clock (wall-clock
     time read as emulated time) or jump it ahead by decades and wrap it (the other way round).
   - Let the core record in the save state which clock a state's `RtcState` follows (a new field, which
     older states lack and which then defaults to the wall clock). When a state or a clock state
     from the other source is loaded, rebase `base_timestamp` to the current time of the clock in use,
     keeping the clock's registers as they are.
   - The `.rtc` file, and RetroArch's RTC memory, always hold the wall-clock layout the Nubo app writes.
     On emulated time the core converts on load and on save.
   - Test: real time to emulated time and back, through a save state and through the `.rtc` layout,
     never freezes or jumps the clock.
3. **Share the `.rtc` file layout.** Move `encode_rtc`/`decode_rtc` (17 bytes, in `battery.rs` of
   `crates/nubo-frontend`) into nubo-core, for example as
   `RtcState::to_bytes`/`from_bytes`. Battery saves can then
   move between the Nubo app and RetroArch: `.sav` and `.srm` are both the raw cartridge RAM.
4. **Share the model rule and the palettes.** Done by step 2 of the restructuring plan:
   `nubo-frontend`'s `model` module has `ModelPreference::Automatic`'s rule (`supports_cgb` →
   `Model::CgbE`, else `Model::Dmg`), and its `palettes` module has the palette colors and `recolor`.
   The core uses `nubo-frontend` without default features, so neither Iced nor any platform library
   comes along.
5. **Read access to memory for achievements:** work RAM (all banks), HRAM and cartridge RAM as `&[u8]`.
   They are read-only, so no pointers into the emulator are needed (see Phase 4).
6. **Optional: a rumble hook.** MBC5 rumble cartridges are recognised, but nothing reports the motor.
   A default no-op `CartridgeEffects::set_rumble(bool)` would let the core drive RetroArch's rumble
   interface.

## Phase 2: a playable core (one pull request)

1. Create the crate `frontends/nubo-libretro`, package `nubo-libretro`, with
   `[lib] name = "nubo_libretro", crate-type = ["cdylib", "rlib"]` (the `rlib` is for tests).
   - Make it a workspace member but not a default member, like the other frontends.
   - It depends only on `nubo-core` and `nubo-frontend` (without default features). Write the C types
     with `core::ffi`, without `libc`.
2. Split it in two:
   - `core.rs`: a safe `Core` type with all the logic. It loads the game, runs a frame, converts video
     and audio, maps input, frames save states, syncs save RAM, handles options and parses cheats.
   - `ffi.rs`: the only `unsafe` code. It exports the `retro_*` functions, keeps RetroArch's callbacks,
     and turns pointers into slices and strings.

   Keep the state in a `static Mutex<Option<Core>>` or similar, so holding it needs no `unsafe`.
   - Wrap every entry point in `catch_unwind`. A panic must not cross into C: it fails the call (for
     example `retro_load_game` returns false) instead of aborting RetroArch.
   - Log through `RETRO_ENVIRONMENT_GET_LOG_INTERFACE`, calling its printf-style callback with `"%s"`.
3. **Every `retro_*` function, from the start.** RetroArch looks up the whole set when it loads a core
   (`CORE_SYMBOLS` in its `runloop.c`) and refuses a core that lacks one. So this phase exports all of
   them, and the ones later phases fill in start as harmless stubs:
   - `retro_serialize_size` returns 0, which tells RetroArch the core has no save states yet, and
     `retro_serialize` and `retro_unserialize` return false.
   - `retro_cheat_reset` and `retro_cheat_set` do nothing.
   - `retro_get_memory_data` returns null and `retro_get_memory_size` returns 0.
   - The callback setters (`retro_set_video_refresh`, `retro_set_audio_sample`,
     `retro_set_audio_sample_batch`, `retro_set_input_poll`, `retro_set_input_state`,
     `retro_set_environment`) store their callbacks.

   Check the list against `libretro.h` and RetroArch's `CORE_SYMBOLS`, and add a test that the built
   library exports every name on it.
4. What the calls do in this phase:
   - **System info:** `retro_api_version`, `retro_get_system_info` (name "Nubo", the version from
     `CARGO_PKG_VERSION`, extensions `gb|gbc`, `need_fullpath = false`), `retro_init` and `retro_deinit`.
   - **Audio and video info:** `retro_get_system_av_info` gives 160×144 at 4194304 / 70224 fps and a
     sample rate such as 48,000 Hz (any rate works: Nubo makes exactly that many samples per emulated
     second, and RetroArch adjusts for small differences).
   - **Pixel format:** ask for `RETRO_PIXEL_FORMAT_XRGB8888` with `RETRO_ENVIRONMENT_SET_PIXEL_FORMAT`
     in `retro_load_game`, where `libretro.h` says to ask, not in `retro_set_environment`. Each pixel
     is a native-endian `u32` holding `0x00RRGGBB`, so convert from RGBA with a pitch of 640 bytes. If
     the frontend says no, it keeps the default 0RGB1555 and would misread the frames, so either
     convert to that format instead or fail the load with a message. Test both answers.
   - **Load and unload:** `retro_load_game` picks the model, loads the ROM, and fails on a
     `LoadRomError`. Also `retro_unload_game`, `retro_reset` (a new `Emulator` with the same ROM and
     model, which keeps the cartridge's battery-backed state: the current save RAM and, on an MBC3
     with a clock, the current `RtcState`, handed to the new emulator before the buffers are synced
     as Phase 3 describes; a test resets an MBC3 game and checks that its save and clock survive), `retro_get_region` (NTSC), `retro_load_game_special` (false) and
     `retro_set_controller_port_device`.
   - **`retro_run`:**
     1. Poll input once and press or release changed buttons: A, B, Start, Select and the D-pad.
        Libretro's B and A are the bottom and right face buttons, which match the Game Boy's B and A.
     2. Run one frame.
     3. Apply the palette to original Game Boy games.
     4. Send the frame.
     5. Send the audio as interleaved `i16` through `audio_sample_batch`, clamped to the 16-bit range.
   - **Input descriptions:** set them with `RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS`, so RetroArch's
     remap menu shows Game Boy button names.

**Done when:**
- The core loads in RetroArch on a desktop, and a DMG game and a CGB game play with correct picture,
  sound and controls.
- The `Core` unit tests pass (frame timing, samples per second, pixel conversion, input).
- An FFI smoke test passes: an integration test calls the exported functions in RetroArch's order, with
  test callbacks, on a test cartridge (see `tools/nubo-save-state-check/src/cartridges.rs`), and checks
  that every `retro_run` sends one 160×144 frame and the right number of samples.

## Phase 3: saves, save states, options and cheats (one pull request)

1. **Battery saves.**
   - RetroArch keeps the `.srm` file itself. After `retro_load_game` it copies the file into the buffer
     that `retro_get_memory_data(RETRO_MEMORY_SAVE_RAM)` returns, and it reads that buffer when it
     saves. So the buffer has to belong to the core and stay at one address.
   - Keep a buffer of the cartridge RAM's size, but only when `has_battery` is set (size 0 otherwise, so
     no `.srm` is written for games without a battery).
   - At the start of each `retro_run`, if the buffer differs from what the core last copied into it,
     RetroArch has changed it. Call `set_cartridge_ram`.
   - After the frame, if `save_ram` was called, copy `cartridge_ram()` into the buffer.
   - Do the same for `RETRO_MEMORY_RTC`, using the `.rtc` layout from Phase 1 and `set_rtc_state`.
   - **After loading a save state** (`retro_unserialize`), and after `retro_reset`, copy the
     emulator's cartridge RAM and clock state into both buffers and into what the core last copied.
     A save state holds its own cartridge RAM and clock, and loading one doesn't call `save_ram` or
     `save_rtc_state`, so without this the buffers would keep the values from before the load. The
     next `retro_run` would then see nothing to copy either way, and RetroArch could later write the
     old save to disk.
   - Tests:
     - A buffer filled before the first `retro_run` is what the game reads.
     - The game's writes reach the buffer.
     - After loading a state whose cartridge RAM and clock differ from the current ones, both buffers
       hold the state's values, and a later `retro_run` doesn't copy the old ones back.
2. **Save states.** `retro_serialize_size` must give a size before any state is made, and run-ahead
   and netplay expect it not to change. Nubo's states vary in length, and `load_state` refuses bytes
   left over at the end.
   - Store a 4-byte little-endian length, then the state, then zero padding up to the size given.
   - Make the size the first state's length plus a margin.
   - Find out whether a game's state length ever changes. If it can grow past the margin, tell RetroArch
     with `RETRO_ENVIRONMENT_SET_SERIALIZATION_QUIRKS` (`RETRO_SERIALIZATION_QUIRK_CORE_VARIABLE_SIZE`).
   - Optionally also load plain states made by the Nubo app, which start with `NUBO SAVE STATE\0`.
   - Measure how long saving plus loading takes. Run-ahead does both every frame, and a game frame at
     20× real time is under 1 ms. Loading first rebuilds a just-started emulator, so check what that
     costs, and make it cheaper in the core if it dominates.
   - Test: save, run 100 frames, load, run 100 frames, and get identical frames and samples.
3. **Options** (`RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2`; check whether older RetroArch versions still
   need the older calls):
   - `nubo_model`: Automatic, or one of the models by name (`Model` and its `FromStr` names).
   - `nubo_palette`: the palettes from Phase 1, gray first.
   - `nubo_rtc`: real time (default) or emulated time. On real time, an MBC3 game's clock reads the wall
     clock, so a loaded state can see a different time than the run it was saved from, and netplay
     peers can see different times. Emulated time makes every run repeatable. The option's
     description says to choose it for netplay with games that have a clock.
   - Read changes with `RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE` at the start of `retro_run`. `nubo_model`
     and `nubo_rtc` take effect only on the next reset or game load, which say so in their
     descriptions. Switching `nubo_rtc` then goes through the rebasing of Phase 1 item 2, so the
     game's clock neither stops nor jumps.
4. **Cheats.** `retro_cheat_reset` turns all cheats off. `retro_cheat_set(index, enabled, code)` may get
   several codes joined with `+`: register each one, Game Genie or GameShark depending on which
   validator accepts it, under an id such as `"<index>-<n>"`.

**Done when:**
- In RetroArch: saves survive a restart, and a `.sav` from the Nubo app works as a `.srm`.
- Save states, rewind, and run-ahead with one frame work.
- Options apply, and a Game Genie and a GameShark cheat work.
- `Core` tests cover each of these.

## Phase 4: achievements and rumble (one pull request)

1. **Memory maps for RetroAchievements.**
   - Opt in first: call `RETRO_ENVIRONMENT_SET_SUPPORT_ACHIEVEMENTS` with `true` in `retro_load_game`,
     before the first `retro_run`, as `libretro.h` requires. The memory descriptors and the info file
     don't opt the core in on their own, and without it RetroArch treats the core as not supporting
     achievements (no hardcore mode).
   - rcheevos reads the Game Boy's address space through memory descriptors
     (`RETRO_ENVIRONMENT_SET_MEMORY_MAPS`), with Game Boy Color work RAM banks 2-7 after 0xFFFF. See
     `rc_consoles.c`, and SameBoy's `libretro.c` for one way to describe it.
   - rcheevos only reads, so after each `retro_run` copy work RAM, HRAM and cartridge RAM (Phase 1's
     accessors) into buffers the core owns, and point the descriptors at them. Copying about 64 KiB per
     frame is cheap.
   - Also return work RAM for `RETRO_MEMORY_SYSTEM_RAM`. Note in the docs that RetroArch's own memory
     editing doesn't reach the game this way.
   - Set `memory_descriptors = "true"` in the info file.
2. **Rumble**, if Phase 1 added the hook: `RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE`, strong motor on
   while the cartridge's motor bit is set.

**Done when:** a game with an achievement set on retroachievements.org shows its achievements in
RetroArch with a test account, and a rumble cartridge game (such as Pokémon Pinball) rumbles.

## Phase 5: packaging and Nubo's own releases (one pull request)

1. Write `frontends/nubo-libretro/nubo_libretro.info`, based on the info files of Gambatte and SameBoy in
   [libretro/libretro-core-info](https://github.com/libretro/libretro-core-info).
   - **Names:** `display_name = "Nintendo - Game Boy / Color (Nubo)"`, `corename = "Nubo"`,
     `systemname`, `systemid = "game_boy"`, `database = "Nintendo - Game Boy|Nintendo - Game Boy Color"`,
     `supported_extensions = "gb|gbc"`.
   - **Licence and firmware:** `license = "GPLv3"`, `firmware_count = 0` (the boot ROMs are built in).
   - **Features:** `savestate = "true"`, `savestate_features = "deterministic"` (see Status), `cheats = "true"`,
     `input_descriptors = "true"`, `libretro_saves = "true"`, `core_options = "true"`,
     `needs_fullpath = "false"`, `is_experimental = "true"` at first.
   - `"deterministic"` rather than `"serialized"`: RetroArch offers run-ahead and netplay only then (see Status).
   - A short `description`.
2. Add `cargo xtask libretro`. It builds the release library for the host and copies it to
   `target/bundle/` with RetroArch's file name, plus the info file and the license files
   (`licenses::write_license_files`). Cargo names the library `libnubo_libretro.so` on Linux and
   `libnubo_libretro.dylib` on macOS, but RetroArch expects `nubo_libretro.so`, `nubo_libretro.dylib` and
   `nubo_libretro.dll`.
3. CI already runs clippy and the tests for every workspace member on Linux, macOS and Windows. Make
   sure the crate builds with `cargo doc` without warnings, and that `cargo deny` stays clean.
4. In `release.yml` and `nightly-build.yml`, attach `Nubo-libretro-<os>-<arch>.zip` (core, info file,
   licenses) for Windows x64, Linux x86_64 and ARM64, and macOS.
5. Document it:
   - In the README: how to install the core by hand (copy the library to RetroArch's `cores` folder and
     the info file to its `info` folder), and what it does and doesn't support.
   - In AGENTS.md: the crate in Where Things Are, `cargo xtask libretro` in `xtask/AGENTS.md`, and the
     `unsafe` exception in Rules.
   - Add the release notes entry when the next version ships.

**Done when:** a release has the core zips, and installing one by hand on Windows, Linux and macOS
works.

## Phase 6: RetroArch's own core downloader (with the maintainer)

**On hold** until the maintainer picks it up (see Status).

This is outward-facing: only do it once the maintainer says to go ahead.

1. Benchmark first on the slow hardware RetroArch players use, such as a Raspberry Pi 4 or a handheld
   with an RK3326 or A133 chip. Use `cargo benchmark` with the homebrew games from
   `cargo xtask benchmark-roms`, and note the result in the README.
2. Contact the libretro team (their GitHub or Discord). Usually they mirror the core under the libretro
   organisation and build it on their GitLab CI with a `.gitlab-ci.yml` in the core's repository that
   includes their `libretro-infrastructure/ci-templates`. Those templates are written for Makefiles, so
   a small Makefile that calls cargo for each platform may be needed. Rust cores are rare: find one that
   is already on the buildbot (Holani, an Atari Lynx core, is believed to be in Rust) and copy its
   setup.
3. Open a pull request to `libretro/libretro-core-info` with `nubo_libretro.info`, and optionally a
   documentation page to [libretro/docs](https://github.com/libretro/docs).
4. Platforms: Windows, Linux (x86_64, ARM), macOS, Android and iOS/tvOS are realistic in Rust. Consoles
   (Switch, PS Vita, 3DS, Wii) are not.

**Done when:** "Nintendo - Game Boy / Color (Nubo)" can be installed from RetroArch's Online Updater.

## Order

Phase 0, the review of this plan, comes first. Phases 1 to 3 give a useful core and come next, as
three pull requests. Phase 5 can follow
right after Phase 3, so players can try it before achievements are done. Phase 4 comes after that, and
Phase 6 once the core has been in releases for a while.
