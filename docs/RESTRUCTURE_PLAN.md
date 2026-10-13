# Plan: restructuring the repository

**Status: signed off by the maintainer on 2026-10-13. No step has been done yet.**

## What is planned and what is only kept possible

- **The Iced app** stays Nubo's main app, on the desktop and in the browser. Its package stays `nubo`,
  so its program stays `nubo` (`nubo.exe` on Windows).
- **The RetroArch core** ([RETROARCH_PLAN.md](RETROARCH_PLAN.md)) is one of the next things to pick up.
- **An iOS app, a GTK app and a web UI in React** are not planned. The restructuring must keep them
  possible without another restructuring, but it creates no crates, folders or steps for them. See
  "Options kept open" below.

Today everything the Iced app needs besides its views lives inside the Iced crate: running a game at
the right pace, battery saves, save states, cheats, palettes, settings, key bindings and the library.
This plan moves that into a shared crate, `nubo-frontend`. The RetroArch core can then take the
palettes and the model rule from there. A later app would only have to draw and handle input.

## Goals

1. Every app builds on `nubo-core` (the emulator) and `nubo-frontend` (everything an app needs that
   isn't drawing), and nothing else of Nubo's.
2. `nubo-frontend` doesn't depend on Iced or any other UI toolkit. How a game is paced is something the
   app plugs in.
3. Apps players use live in `frontends/`. Developer programs live in `tools/`.
4. Every package's name starts with `nubo`. Folder names stay short (`frontends/iced`,
   `tools/test_rom_runner`). `xtask` keeps its name, as the `cargo xtask` convention expects.
5. Nothing changes for players:
   - **Files:** the same files in the same places and formats (`settings.toml`, `library.toml`,
     `cheats/*.toml`, `.sav`, `.rtc`, `.state` and the pictures), in the folders
     `ProjectDirs::from("", "", "Nubo")` picks.
   - **Identity:** the same app id (`io.github.mschoenm.nubo`), the same program name (`nubo`), the same
     installer `AppId`.
   - **Behaviour:** the same.
6. Every step is one pull request that leaves `main` working, with nothing half moved.

Not goals: changing the emulator (beyond the one method step 2 adds), changing how the Iced app looks or
behaves, or building a new app.

## Where things are today

### Packages

| Folder | Package | After this plan |
|---|---|---|
| `crates/nubo-core` | `nubo-core` | Unchanged |
| `crates/nubo-logo` | `nubo-logo` | Unchanged |
| `crates/serializable_derive` | `serializable_derive` | Package renamed `nubo-serializable-derive` |
| `frontends/iced` | `nubo` | Unchanged (it is the main app, and the name gives the program `nubo`) |
| `frontends/benchmark` | `nubo-benchmark` | Moves to `tools/benchmark` |
| `frontends/headless` | `nubo-headless` | Moves to `tools/headless` |
| `frontends/json_test_runner` | `json_test_runner` | Moves to `tools/json_test_runner`, package renamed `nubo-json-test-runner` |
| `frontends/save_state_check` | `nubo-save-state-check` | Moves to `tools/save_state_check` |
| `frontends/test_rom_runner` | `test_rom_runner` | Moves to `tools/test_rom_runner`, package renamed `nubo-test-rom-runner` |
| `xtask` | `xtask` | Unchanged |
| (new) `crates/nubo-frontend` | `nubo-frontend` | Step 2 |
| (new) `frontends/libretro` | `nubo-libretro` | The RetroArch plan |

### The Iced crate

Counted per file in `frontends/iced/src` (about 13,000 lines with tests):

- **UI code, which stays in the Iced crate:** `views/`, `ui/`, `app.rs` and `app/`, `main.rs`, `ticks.rs`
  and `input.rs`. That is about a third of the lines.
- **Already free of Iced:**
  - `palettes`, `cheats`, `rom`, `notices`, `screenshots`, `storage`, `title_screens`
  - `box_art/download`, `library/scan`, `library/jobs`
  - `menu_presses`, `efficiency`, `emulation/web.rs`
  - the slots behind `battery`, `settings`, `cheats`, `library` and `save_states` (`file_*`,
    `local_storage_*`, `no_store`)
- **Using only small things from Iced:**

  | What | Where | Replacement |
  |---|---|---|
  | `iced::time` (`Instant`, `SystemTime`, `Duration` that also work in the browser) | `battery`, `audio`, `emulation`, `pacing`, `game`, `save_states`, `library` | The [`web-time`](https://crates.io/crates/web-time) crate (MIT/Apache), which is what makes these work in the browser in the first place |
  | `iced::futures` channels and `Subscription` | `emulation/thread.rs` (new frames), `controllers/thread.rs`, `library/watcher.rs`, `background.rs` | Callbacks or `futures` channels in the shared crate; the Iced crate turns them into subscriptions |
  | `iced::widget::image::Handle` | `library` (thumbnails, title screens, box art), `save_states`, `game` (the screen) | A neutral `Picture` (width, height, RGBA bytes); the Iced crate makes and caches one `Handle` per picture |
  | `iced::keyboard::key::Code` | `controls` (key bindings, stored in `settings.toml` by name) | Nubo's own `KeyCode` enum that stores the same names, so existing `settings.toml` files read the same |

- **Mixed:**
  - `game.rs` holds both the running game (playback, cheats, palette, save slots) and the overlay
    (auto-hiding bar, pause menu, mouse).
  - `settings.rs` holds both app-wide settings and two that only matter to the Iced app (`appearance`,
    `integer_scaling`).

## Target layout

```
crates/
  nubo-core/            the emulator (unchanged)
  nubo-frontend/        everything an app needs that isn't drawing (new)
  nubo-logo/
  serializable_derive/  package nubo-serializable-derive
frontends/              apps players use
  iced/                 package nubo: the main app (views, theme, Iced input and subscriptions)
  libretro/             package nubo-libretro: the RetroArch core (RETROARCH_PLAN.md)
tools/                  developer programs
  benchmark/  headless/  json_test_runner/  save_state_check/  test_rom_runner/
xtask/
```

### `nubo-frontend`

One crate with modules, not several small crates: they change together, and one crate is easier to
review and version. It can be split later if a part grows a life of its own.

| Module | From | Notes |
|---|---|---|
| `time` | `iced::time` | Re-exports `web-time` |
| `storage` | `storage/`, `*/file_store.rs`, `*/local_storage*.rs`, `*/no_store.rs` | A `Storage` trait with files (desktop) and localStorage (browser); another platform adds its own |
| `settings` | `settings.rs` | App-wide settings as data, read and written as `settings.toml` exactly as now. The two Iced-only settings (`appearance`, `integer_scaling`) stay in the Iced crate but in the same file, so it keeps its shape |
| `controls` | `controls.rs` | Actions, bindings and Nubo's own `KeyCode` and `PadButton` |
| `palettes` | `palettes.rs` | Also used by the RetroArch core |
| `model` | `ModelPreference` in `settings.rs` | The rule that picks a game's model. Also used by the RetroArch core |
| `cheats`, `battery`, `save_states`, `screenshots`, `rom`, `notices` | Same names | |
| `library` | `library.rs`, `library/` | Scan, cache, folder watcher (behind `desktop`), title screens, box art (behind `desktop`). Pictures as `Picture` |
| `session` | `emulation.rs`, `emulation/`, `audio.rs`, the game half of `game.rs` | Running a game. See "Pacing" below |
| `gamepads` | `controllers.rs`, `controllers/` | gilrs, behind a feature |
| `background` | `background.rs` | Runs slow work on a thread of its own and hands the result back as a future |

Feature flags keep platform dependencies out of builds that can't use them:

- **`desktop`:** cpal, directories, notify-debouncer-mini, ureq, image, crc32fast.
- **`gamepads`:** gilrs.
- **`web`:** localStorage and the browser's sound.

The Iced crate turns on what it uses today. The RetroArch core turns on none: it only needs `palettes`
and `model`.

### Pacing

Today the desktop's emulator thread is paced by the sound card, and the browser build runs the frames
due on each screen refresh. The RetroArch core adds a third way:

| Pacing | Used by | What drives it |
|---|---|---|
| **Sound card** | Iced on the desktop (as now) | The cpal callback wakes a thread when its queue runs low |
| **Screen refresh** | Iced in the browser (as now) | The app calls `run_due_frames(timestamp)` on each refresh; `pacing.rs` decides how many frames are due |
| **Host** | The RetroArch core | RetroArch calls `retro_run` once per frame and does its own timing |

The first two become two implementations behind one `Session` that takes the same `Command`s as today.
The third needs no session, only `Emulator::run_frame()` in the core (step 2), which the screen-refresh
pacing can use too. A new frame is announced through a callback the app passes in, not an Iced
`Subscription`. The Iced crate's callback pushes into the channel its subscription reads, as now.

### The `unsafe` rule

`nubo-core`, `nubo-frontend` and the Iced app keep `unsafe_code = "forbid"`. The only exception this
plan expects is the RetroArch core's exported C functions. They go in `frontends/libretro/src/ffi.rs`,
with the crate's own lint table and the `unsafe` in as few places as possible (see the RetroArch plan).
CLAUDE.md's style rule will say exactly that. Any later crate that binds Nubo to another language
follows the same rule.

## Options kept open

None of these is planned. The design above is what keeps each possible later without another
restructuring:

- **iOS:**
  - **What it would add:** a Swift app over a bindings crate (for example `nubo-ffi`, made with
    UniFFI) that exposes `nubo-frontend`.
  - **What already makes it possible:** no UI toolkit in `nubo-frontend`; screen-refresh pacing (iOS's
    `CADisplayLink`); `Storage` as a trait (the app's sandbox); gamepads behind a feature (iOS has its
    own controller API); cpal supports iOS.
- **GTK:**
  - **What it would add:** a `frontends/gtk` crate with gtk4-rs, mostly for Linux.
  - **What already makes it possible:** either pacing (the sound card, or GTK's frame clock); everything
    stored is shared; key bindings use Nubo's own `KeyCode`.
- **A web UI in React:**
  - **What it would add:** a bindings crate (for example `nubo-wasm`, made with wasm-bindgen) and a
    JavaScript/TypeScript app.
  - **What already makes it possible:** screen-refresh pacing (`requestAnimationFrame`); the `web`
    feature with localStorage; `web-time`.

To keep these open, `nubo-frontend` must not rely on anything only the Iced app has, such as a
`Subscription`, an `image::Handle` or an Iced key code. Since it doesn't depend on Iced at all, the
compiler enforces that. The browser build checks the `web` feature.

## Steps

Each step is one pull request. Each must pass the full CI and leave every fingerprint the same:
`cargo benchmark --compare` on the games and test ROMs, `cargo test-roms`, `cargo save-states`, and
the Iced app's tests. The emulator doesn't change in any of them except step 2's added method.

### Step 1: tools to `tools/`, and the `nubo` prefix

1. Move `benchmark`, `headless`, `json_test_runner`, `save_state_check` and `test_rom_runner` from
   `frontends/` to `tools/`.
2. Rename the packages that don't start with `nubo`:
   - `json_test_runner` becomes `nubo-json-test-runner`
   - `test_rom_runner` becomes `nubo-test-rom-runner`
   - `serializable_derive` becomes `nubo-serializable-derive`. Its `use serializable_derive::...` lines
     in nubo-core become `nubo_serializable_derive`. The derive writes field and variant names, not
     its own crate name, so save states don't change. `cargo save-states` checks that.
3. Update everything that names the old paths or packages:
   - **Workspace:** the root `Cargo.toml` (members, default members, `[workspace.dependencies]`) and
     the cargo aliases in `.cargo/config.toml` (`cargo test-roms`, `cargo cpu-tests`). The commands
     themselves stay the same.
   - **CI:** `.github/actions/test-roms/action.yml` (the cache key's `hashFiles` paths), and any
     `--package` in the workflows.
   - **xtask:** `xtask/src/review_units.rs` (`CRATE_FOLDERS` gets `"tools"`).
   - **The tools' own messages:** in `test_rom_runner/src/download.rs`,
     `json_test_runner/src/download.rs` and `save_state_check/src/main.rs`.
   - **Docs:** `save-states/README.md`, the README (links to `passing.txt` and `FAILING_TESTS.md`),
     CLAUDE.md, and the plans in `docs/`.
4. **Nightly review:** its rotation remembers review dates by file path. Give `cargo xtask review-units`
   a way to carry dates over to renamed paths. For example, when it finds a path in the state that no
   longer exists, it looks it up with `git diff --name-status -M <last_commit> HEAD` and moves the
   date to the new path. That way moved files keep their place in the rotation, and later moves are
   covered too. Add a unit test with a moved file.

**Done when:**
- CI passes.
- `cargo test-roms`, `cargo cpu-tests`, `cargo save-states` and `cargo benchmark` work as before.
- The test ROM cache key still hits.
- `cargo xtask review-units next` keeps the review dates of the moved files.

### Step 2: what the RetroArch core needs

1. Add `Emulator::run_frame()` to the core, as described in the RetroArch plan.
2. Create `crates/nubo-frontend` with only `palettes` and `model`, moved from the Iced crate. The Iced
   crate depends on it. RETROARCH_PLAN.md already expects them there.

After this step the RetroArch core can be built without waiting for the rest.

**Done when:** CI passes, `run_frame` has its tests, and the Iced app shows the same palettes and picks
the same models.

### Step 3: neutral building blocks

Without moving modules yet, take Iced out of the parts that don't draw:

- `web-time` instead of `iced::time`
- `futures` channels (or callbacks) instead of `iced::futures`
- `Picture` instead of `image::Handle` outside the views, with one `Handle` cached per picture in the
  Iced crate
- `KeyCode` instead of `iced::keyboard::key::Code` in `controls.rs`, with a mapping in `input.rs`

Add a test that a `settings.toml` written by the current version (a fixture in the tests) reads back to
the same settings and writes back byte for byte.

**Done when:** the non-UI modules no longer use Iced, CI passes, the browser build works
(`cargo xtask web`), and the fixture test passes.

### Step 4: move the stored data

Move `storage`, `settings`, `controls`, `cheats`, `battery`, `save_states`, `screenshots`, `rom`, `notices`
and `background` into `nubo-frontend`, with their tests. Split `settings.rs` so the two Iced-only
settings stay with the Iced app but in the same file.

**Done when:** CI passes and an existing data folder (settings, saves, states, cheats) works unchanged
with the new build. Test this by hand on the desktop and in the browser.

### Step 5: move the library

Move `library` (scan, cache, watcher, jobs), `title_screens` and `box_art` into `nubo-frontend`. The
watcher reports through a callback, and the Iced crate wraps it in a subscription.

**Done when:** CI passes, and a library with folders, thumbnails, title screens and box art loads
without rescanning everything.

### Step 6: move running a game

This is the largest step.

1. Split `game.rs` into the game session (start, playback, cheats, palette, save slots, the screen
   picture), which moves, and the overlay (bar, pause menu, mouse), which stays.
2. Move `emulation/`, `audio` and `controllers` (as `gamepads`) into `nubo-frontend`'s `session` and
   `gamepads`.
3. Make new frames reach the app through a callback.

Before starting, record the efficiency harness's numbers (`nubo --measure-efficiency`). This step
must not add wake-ups, buffers or frames.

**Done when:**
- CI passes.
- The efficiency numbers are the same.
- On the desktop and in the browser, playing, pausing, fast forward, sound, controllers, saves and
  states all work as before.
- The Iced crate holds only UI: views, theme, input translation, subscriptions and the overlay.

### Step 7: finish the docs

Each step keeps CLAUDE.md true as it goes. This step checks the whole picture:

- **CLAUDE.md:** the layout, `nubo-frontend`'s modules and features, the pacing, and the `unsafe` rule
  with its one exception.
- **README:** where the app and tools live.
- **`docs/NIGHTLY_REVIEW.md`:** any paths it names.
- **This plan:** marked as done.

## Order and dependencies

- **The order agreed with the maintainer:**
  1. step 1
  2. step 2
  3. Part 5 of [SHOOTOUT_PLAN.md](SHOOTOUT_PLAN.md) (the Shootout's tests in CI, the nightly run and
     the metrics)
  4. steps 3 to 6
  5. step 7

  Shootout Part 5 goes before steps 3 to 6 because it only touches the test ROM runner, the nightly
  workflow and `cargo xtask metrics`, none of which those steps move. It needs step 1, so it is built
  straight into `tools/test_rom_runner`.
- **RetroArch:**
  - Steps 1 and 2 come first. Step 2 is what the RetroArch core needs.
  - The RetroArch plan's Phases 2 to 6 only touch `frontends/libretro`, so they can run alongside
    steps 3 to 6.
- **Steps 3 to 6** must go in order. Nothing else should change the Iced app's non-UI modules meanwhile,
  or the moves will conflict.
- **Shootout plan:**
  - Part 1's options (`--scale`, `--fresh`, `--speed`) can go before step 3 or after step 6, but not in
    the middle.
  - Before is simpler: they are small, and `--fresh` then moves with `storage` like everything else.

## Decisions taken

- The layout and steps are signed off (2026-10-13).
- The Iced app stays the main app. Its package and program stay `nubo`.
- The RetroArch core is planned next. iOS, GTK and a React web UI are only kept possible.
- Every package starts with `nubo`. Folder names stay as they are. `xtask` keeps its name.
- Developer programs go in a `tools/` folder, each its own crate.
- All steps are scheduled, in the order under "Order and dependencies", with Shootout Part 5 between
  steps 2 and 3.
- Review dates carry over to moved paths in the nightly review.
