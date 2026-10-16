# Plan: an iOS app

**Status: a draft, for the maintainer to review.** Only the C interface is written so far:
[`crates/nubo-ffi/include/nubo.h`](../crates/nubo-ffi/include/nubo.h), with nothing behind it yet. No
crate, no Swift and no CI job exist for it. Nothing here changes what players of the desktop app, the
browser build or the RetroArch core get.

An iOS app reaches players Nubo doesn't reach today: iPhone and iPad owners, who can't run the desktop
app and for whom the browser build has no touch controls. Since 2024 the App Store accepts emulators, as
long as they bring no commercial games, and Nubo needs no BIOS files: its boot ROMs are its own.

## Goals

1. **The app is a native Swift app** (SwiftUI, with UIKit where needed), not the Iced app: Iced has no
   iOS support, and its widgets are made for a mouse and a keyboard.
2. **It plays exactly as the desktop app does.** It runs `nubo-frontend`'s `GameSession`, so the
   emulation, the timing, rewind, cheats and the Super Game Boy are the same code, and every rule in
   `AGENTS.md` holds for it.
3. **It keeps players' data in the same formats** as the desktop app (`settings.toml`, `.sav`, `.rtc`,
   `.state`, `cheats/*.toml`, `library.toml`), so `nubo-frontend`'s tests on existing data folders
   cover it too, and a save could move between the desktop and a phone.
4. **The Swift code lives in its own repository** (`nubo-ios`); Nubo's repository keeps only the Rust
   side and the C header both sides build against.
5. **The core stays forbidden `unsafe`.** The C functions are the only `unsafe` code, in one module of
   one crate, as `AGENTS.md` allows for a crate that binds Nubo to another language.

## Two repositories

| | Nubo (`mschoenm/nubo`) | `nubo-ios` (new) |
|---|---|---|
| Holds | `crates/nubo-ffi`: the C functions over `nubo-frontend` and the header `include/nubo.h`; the changes to `nubo-frontend` that iOS needs; `cargo xtask xcframework` | The Xcode project, the SwiftUI views, touch controls, the GameController framework, the Files app and document picker, the audio session, signing and TestFlight and App Store uploads |
| Builds | `nubo-ffi` for `aarch64-apple-ios` and `aarch64-apple-ios-sim`, packaged as `NuboCore.xcframework` with the header and a module map | The app, linking the XCFramework |
| CI | A macOS job on every pull request: builds the XCFramework and runs `nubo-ffi`'s tests | A macOS job: builds the XCFramework from its Nubo checkout, builds the app and runs its tests |
| Rules | `AGENTS.md`, unchanged | Its own; the license and the App Store exception (below) |

`nubo-ios` holds Nubo as a **git submodule**, pinned to a commit or a release tag. An Xcode build step
runs `cargo xtask xcframework` in it, so Rust and Swift can be changed side by side on one Mac, and
moving to a newer Nubo is moving the submodule.

**Why the C side lives in Nubo.** `nubo-frontend` has no stable API: nearly every pull request changes
it. With `nubo-ffi` in the workspace, the pull request that breaks the interface is the one that fails,
as it is for the RetroArch core, instead of the next submodule update in `nubo-ios`. And the header is
the contract between the two repositories, so it is reviewed where the code behind it is.

**Why the Swift side doesn't.** Xcode projects, Swift tooling, signing secrets and the App Store's
review timing don't belong in a Cargo workspace whose CI and rules are written for Rust, and the app's
license needs an exception the rest of Nubo doesn't (below).

**Left out for now:** a third repository holding only a Swift package with a prebuilt XCFramework, as
Mozilla's application-services and the Matrix Rust SDK publish. It pays off only once someone besides
`nubo-ios` uses the interface, and can be added later without changing Nubo's side.

## The interface: a C header written by hand

`crates/nubo-ffi/include/nubo.h` is written by hand, and `crates/nubo-ffi/src/ffi.rs` implements it with
`extern "C"` functions, the way `frontends/nubo-libretro/src/ffi.rs` implements `libretro.h`:

- **Safe and unsafe apart.** `ffi.rs` only turns pointers into references and slices and passes values
  through; a safe layer beside it (one module per object: settings, library, game) does the work and
  holds the tests. `ffi.rs` sits under one `#[expect(unsafe_code)]`, with the crate's own lint table
  (`unsafe_code = "deny"`), copied from `nubo-libretro`'s.
- **No panic crosses into C.** Every function that does more than return a constant runs under a
  `guard` that catches a panic, logs it and fails the call, as `nubo-libretro`'s does.
- **The header and the library match.** A test reads the built library and checks that it exports
  every function `nubo.h` declares and nothing else named `nubo_` (as `nubo-libretro`'s
  `tests/exports.rs` does), and that `NUBO_API_VERSION` is what `nubo_api_version()` returns. The
  header's version goes up whenever a function's meaning or signature changes; adding a function
  doesn't need it.
- **Its enums are kept forever.** `NuboModel`'s numbers may be stored by the app, so like a save state's
  stored names they never change; new models get new numbers.

What the header holds, from the top:

- the rules every function follows: UTF-8 strings ending with a NUL byte, who frees what
  (`nubo_string_free` for every `char *` Nubo hands over), `false` or `NULL` with a message in
  `char **error` on failure, a `_free` function for each object, any thread allowed;
- versions: `NUBO_API_VERSION`, `nubo_api_version`, `nubo_version`;
- where data is kept: `nubo_set_data_folder`, called once at start with the app's Application Support
  folder (`nubo_frontend::storage::files::use_folder`);
- the Game Boy's buttons, models and save state slots (`NuboButton`, `NuboModel`, `NuboSlot`);
- pictures copied into a buffer the app owns (`NuboPicture`; `NUBO_PICTURE_MAX_BYTES` fits the largest,
  a Super Game Boy screen with its border, so one buffer made at start does for every frame);
- the settings (`NuboSettings`): load, save, and a getter and setter for each setting the app shows;
- the library (`NuboLibrary`): load, a blocking scan to call off the main thread, lists of games
  (`NuboGameList`) and their pictures, and the model rule (`nubo_library_start_model`,
  `nubo_library_resume_model`);
- a running game (`NuboGame`): start and free, buttons, pause, suspending when the app goes to the
  background, fast forward and rewind held, the settings applied, the frame callback, the screen, save
  state slots, cheats and the problems to show.

**Why not UniFFI.** UniFFI would write the C functions and the Swift classes itself, and was the first
idea (`docs/RESTRUCTURE_PLAN.md`). A header written by hand wins here: it is the pattern the repository
already has for the RetroArch core, it adds no dependency, the contract between the two repositories is
a file a person can read, and its `unsafe` code is ours to keep small. What it costs is a small Swift
wrapper in `nubo-ios` (classes around the handles, `deinit` calling the `_free` functions, errors turned
into `throws`), a few hundred lines.

## How the app runs a game

- **Pacing by the sound card, as on the desktop.** iOS has threads, so it gets `nubo-frontend`'s
  `session/sound_card.rs`, not the browser's screen-refresh pacing: the emulator runs on a thread of its
  own, which CoreAudio's queue (through cpal) wakes. `run_due_frames` isn't needed, and needs no
  timestamp.
- **Drawing on the screen's refreshes.** The frame callback (`NuboFrameCallback`) runs on the emulator
  thread while the frame is locked, so it only sets a flag; the app's `CADisplayLink` sees the flag and
  copies the picture with `nubo_game_screen` into a buffer it turns into a texture or a `CGImage`.
- **The app's lifecycle.** Going to the background calls `nubo_game_suspend`, which pauses and writes
  the battery saves still waiting (otherwise written up to a second late) and where the player left off:
  iOS may end a suspended app without a word, and `nubo_game_free` would never run.
- **Sound.** The app sets its `AVAudioSession` category before starting a game (playback, so a game
  keeps its sound with the ring switch on silent, or ambient, so it follows the switch; a setting
  could pick).
- **Controls.** On-screen buttons drawn by the app, and controllers through Apple's GameController
  framework; both call `nubo_game_press` and `nubo_game_release`. gilrs doesn't run on iOS, so
  `nubo-ffi` builds `nubo-frontend` without its `gamepads` feature.
- **Games.** The app's games folder (in its Documents folder, shown in the Files app) is the library's
  only folder; the document picker copies games into it. No folder watcher is needed: the app scans
  when it comes to the foreground.

## Changes `nubo-frontend` needs

1. **The model rule moves out of the Iced app.** `App::model_for` and `App::preference_starting_on` in
   `frontends/nubo/src/app.rs` pick the model a game starts on from the library and the settings; the
   iOS app needs the same rule, so it moves to `nubo-frontend` (beside `model::ModelPreference`) and the
   Iced app calls it there.
2. **Starting where the player left off** (reading `Slot::Resume`, the question when its model differs,
   writing it when the game stops) is in the Iced app's `app/playing.rs`; the parts that aren't drawing
   move to `GameSession`, so `nubo_game_start`, `nubo_game_suspend` and `nubo_game_free` share them.
3. **A storage feature that builds for iOS.** `desktop` brings `opener` (opening links and folders) and
   `notify` (the library's folder watcher), which may not build for iOS and aren't needed there. If they
   don't, `desktop` is split so `nubo-ffi` can take only the files and the sound.
4. **Writing battery saves at once** (`BatterySaves::flush` from a running session), for
   `nubo_game_suspend`.

Each is a pull request of its own, before or with the first one for `nubo-ffi`, and none changes the
emulation: no test ROM, save state or fingerprint run changes.

## Licensing

Nubo is GPL-3.0-or-later. The App Store's terms add restrictions the GPL doesn't allow (VLC left the App
Store over this in 2011), so the app needs an **App Store exception**: an additional permission under
section 7 of the GPLv3 that allows distributing Nubo through the App Store under Apple's terms. It has
to cover Nubo's own code too, since the app links it. The maintainer is the only author of Nubo's GPL
code, so they can grant it; the code from RetroBoy is Apache-2.0, and every dependency is under a
permissive license, which need nothing more. The exception is written in `nubo-ios`'s license and in
Nubo's `NOTICE`, before the first upload. Contributions from others from then on need their agreement
to the exception.

## Steps

1. **This plan and the header**, for review. Done once merged.
2. **`nubo-frontend`'s changes** listed above, one pull request each.
3. **`crates/nubo-ffi`**: the crate, `ffi.rs` and its safe layer, the exports test, and its own
   `AGENTS.md`; tests run on Linux like the rest of the workspace.
4. **`cargo xtask xcframework` and the macOS CI job**: build for `aarch64-apple-ios` and
   `aarch64-apple-ios-sim`, package `NuboCore.xcframework` with the header and a module map, on every
   pull request (`docs/CI.md`).
5. **`nubo-ios`**: the repository with the submodule, the Swift wrapper, a first screen that plays a
   game from the Files app with touch controls, then the library, the settings and the pause menu.
6. **TestFlight**, then the App Store, after the license exception.

## Open questions

- Does the `desktop` feature build for iOS as it is (change 3)? Answered by step 4's first CI run, or a
  build on a Mac.
- Which `AVAudioSession` category is the default (playback or ambient)?
- Does the iOS app need the browser build's library that keeps games itself
  (`library::KEEPS_GAMES`), or is a games folder in Documents enough? The plan assumes the folder.
- iPad: the same app with a larger layout, or later?
