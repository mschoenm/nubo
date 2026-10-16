# Plan: restructuring the repository

**Status: signed off by the maintainer on 2026-10-13, and done on 2026-10-14: all seven steps are merged.** Parts 1 to 3 of the Shootout plan are done too (Part 3's run on Windows on 2026-10-14), and the RetroArch core is built; what is left from "Order and dependencies" below is its item 7.

## What is planned and what is only kept possible

- **The Iced app** stays Nubo's main app, on the desktop and in the browser. Its package stays `nubo`,
  so its program stays `nubo` (`nubo.exe` on Windows).
- **The RetroArch core** ([RETROARCH_PLAN.md](RETROARCH_PLAN.md)) is built on this structure.
- **An iOS app, a GTK app and a web UI in React** are not planned. The restructuring must keep them
  possible without another restructuring, but it creates no crates, folders or steps for them. See
  "Options kept open" below. The iOS app has a draft plan of its own since: [IOS_PLAN.md](IOS_PLAN.md).

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
4. Every package's name starts with `nubo`, and every crate's folder is named after its package
   (`frontends/nubo`, `tools/nubo-test-rom-runner`), so a package is always found in the folder of its
   name. `xtask` keeps its name, as the `cargo xtask` convention expects. Commands such as
   `cargo test-roms` or `cargo xtask web` don't change: the cargo aliases name packages, not folders.
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
| `crates/serializable_derive` | `serializable_derive` | `crates/nubo-serializable-derive`, package renamed `nubo-serializable-derive` |
| `frontends/iced` | `nubo` | `frontends/nubo`; the package stays `nubo` (it is the main app, and the name gives the program `nubo`) |
| `frontends/benchmark` | `nubo-benchmark` | `tools/nubo-benchmark` |
| `frontends/headless` | `nubo-headless` | `tools/nubo-headless` |
| `frontends/json_test_runner` | `json_test_runner` | `tools/nubo-json-test-runner`, package renamed `nubo-json-test-runner` |
| `frontends/save_state_check` | `nubo-save-state-check` | `tools/nubo-save-state-check` |
| `frontends/test_rom_runner` | `test_rom_runner` | `tools/nubo-test-rom-runner`, package renamed `nubo-test-rom-runner` |
| `xtask` | `xtask` | Unchanged |
| (new) `crates/nubo-frontend` | `nubo-frontend` | Step 2 |
| (new) `frontends/nubo-libretro` | `nubo-libretro` | The RetroArch plan |

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
  nubo-core/                  the emulator (unchanged)
  nubo-frontend/              everything an app needs that isn't drawing (new)
  nubo-logo/
  nubo-serializable-derive/   the save state derive macro
frontends/                    apps players use
  nubo/                       the main app, built with Iced (views, theme, Iced input and subscriptions)
  nubo-libretro/              the RetroArch core (RETROARCH_PLAN.md)
tools/                        developer programs
  nubo-benchmark/  nubo-headless/  nubo-json-test-runner/
  nubo-save-state-check/  nubo-test-rom-runner/
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
plan expects is the RetroArch core's exported C functions. They go in `frontends/nubo-libretro/src/ffi.rs`,
with the crate's own lint table and the `unsafe` in as few places as possible (see the RetroArch plan).
AGENTS.md's style rule will say exactly that. Any later crate that binds Nubo to another language
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
  - **Since then:** [IOS_PLAN.md](IOS_PLAN.md) drafts it with a C header written by hand instead of
    UniFFI, and with the emulator paced by the sound card as on the desktop, since iOS has threads;
    `CADisplayLink` only draws.
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

### Step 1: tools to `tools/`, the `nubo` prefix and folders named after packages

**Done.**

1. Move `benchmark`, `headless`, `json_test_runner`, `save_state_check` and `test_rom_runner` from
   `frontends/` to `tools/`, each in a folder named after its package (see the table above).
2. Rename `frontends/iced` to `frontends/nubo` and `crates/serializable_derive` to
   `crates/nubo-serializable-derive`.
3. Rename the packages that don't start with `nubo`:
   - `json_test_runner` becomes `nubo-json-test-runner`
   - `test_rom_runner` becomes `nubo-test-rom-runner`
   - `serializable_derive` becomes `nubo-serializable-derive`. Its `use serializable_derive::...` lines
     in nubo-core become `nubo_serializable_derive`. The derive writes field and variant names, not
     its own crate name, so save states don't change. `cargo save-states` checks that.
4. Update everything that names the old paths or packages:
   - **Workspace:** the root `Cargo.toml` (members, default members, `[workspace.dependencies]`) and
     the cargo aliases in `.cargo/config.toml` (`cargo test-roms`, `cargo cpu-tests`). The commands
     themselves stay the same.
   - **CI:** `.github/actions/test-roms/action.yml` (the cache key's `hashFiles` paths), the path
     triggers and the metainfo path in `flatpak.yml` and `windows.yml` (`frontends/iced/linux/**`,
     `frontends/iced/windows/**`, `frontends/iced/build.rs`), and any `--package` in the workflows.
   - **xtask:** `xtask/src/review_units.rs` (`CRATE_FOLDERS` gets `"tools"`), and the Iced app's paths
     in `main.rs` (`LINUX_FILES`, the web `index.html`) and `windows.rs` (`INSTALLER_SCRIPT`).
   - **The tools' own messages:** in `test_rom_runner/src/download.rs`,
     `json_test_runner/src/download.rs` and `save_state_check/src/main.rs`.
   - **Anything else** a search for `frontends/` and the old folder names turns up.
   - **Docs:** `save-states/README.md`, the README (links to `passing.txt` and `FAILING_TESTS.md`),
     AGENTS.md, and the plans in `docs/`.
5. **Nightly review:** its rotation remembers review dates by file path. Give `cargo xtask review-units`
   a way to carry dates over to renamed paths. For example, when it finds a path in the state that no
   longer exists, it looks it up with `git diff --name-status -M <last_commit> HEAD` and moves the
   date to the new path. That way moved files keep their place in the rotation, and later moves are
   covered too. Add a unit test with a moved file.

**Done when:**
- CI passes.
- `cargo test-roms`, `cargo cpu-tests`, `cargo save-states`, `cargo benchmark` and `cargo play` work as
  before.
- The Flatpak and Windows workflows still run on changes to the app's packaging files, and build.
- The test ROM cache key still hits.
- `cargo xtask review-units next` keeps the review dates of the moved files.

### Step 2: what the RetroArch core needs

**Done.**

1. Add `Emulator::run_frame()` to the core, as described in the RetroArch plan.
2. Create `crates/nubo-frontend` with only `palettes` and `model`, moved from the Iced crate. The Iced
   crate depends on it. RETROARCH_PLAN.md already expects them there.

After this step the RetroArch core can be built without waiting for the rest.

**Done when:** CI passes, `run_frame` has its tests, and the Iced app shows the same palettes and picks
the same models.

### Step 3: neutral building blocks

**Done.** `time` and `picture` are already in `nubo-frontend`; `KeyCode` is in `controls.rs` and moves
with it. The channels are `futures` channels, which the Iced crate's `subscriptions.rs` turns into
subscriptions; new frames still come over a channel (`Session::frame_listener`) until step 6.

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

**Done.** What was decided along the way:

- **`storage`** is a `Storage` trait that reads and writes whole `Entry`s (the settings, a game's
  battery RAM and clock, its cheats), with `Files` (`desktop` feature), `LocalStorage` (`web`) and
  `NoStorage` (neither, for the RetroArch core). `storage::platform()` picks the build's own; with
  both features `desktop` wins. The entries keep their file paths and localStorage keys. Save states
  and screenshots aren't entries: they are desktop only and need more than whole entries (renaming,
  file times, pictures), so they use `storage::files` directly, as the library will.
- **`desktop`** brings `directories` and `image` so far, **`web`** `web-sys` and `base64`; the rest
  of the plan's list comes with steps 5 and 6. `background` keeps `cfg(target_arch = "wasm32")`,
  since that is about having threads, not about where data goes.
- **`settings`:** the Iced crate's `Settings` is `appearance`, the shared settings under
  `#[serde(flatten)]`, then `integer_scaling`. TOML writes plain values before tables either way,
  so `settings.toml` comes out byte for byte the same; the fixture test runs in both crates. It
  reads its own two settings and the shared ones separately from the same text
  (`Settings::from_toml`), since reading through flatten reports a mistake in the shared settings
  at line 1 instead of where it is. `nubo_frontend::settings::load` takes that parse function, and
  `save` any type that holds the shared settings.
- **`rom`:** the file dialog (`rom::pick`, with rfd) stays in the Iced crate as `rom_picker.rs`,
  since picking a file is the app's UI. The screen size (`SCREEN_WIDTH`, `SCREEN_HEIGHT`) moved to
  `picture`, for the PNG pictures.
- **An existing data folder:** `crates/nubo-frontend/tests/existing_data_folder/` was written by the
  step 3 build through the app's own save functions, and the tests read all of it back and write it
  again byte for byte. The hand test on the desktop and in the browser is still to do.

Move `storage`, `settings`, `controls`, `cheats`, `battery`, `save_states`, `screenshots`, `rom`, `notices`
and `background` into `nubo-frontend`, with their tests. Split `settings.rs` so the two Iced-only
settings stay with the Iced app but in the same file.

**Done when:** CI passes and an existing data folder (settings, saves, states, cheats) works unchanged
with the new build. Test this by hand on the desktop and in the browser.

### Step 5: move the library

**Done.** What was decided along the way:

- **Where:** `title_screens` and `box_art` (with `box_art/download`) are submodules of
  `nubo_frontend::library`, as the module table has it. The folder picker (`pick_folder`, with rfd)
  stayed in the Iced crate's `rom_picker.rs`, next to the ROM picker, since picking a folder is the
  app's UI; the library's messages and tasks stay in `app/library_screen.rs`.
- **`desktop`** now also brings `crc32fast`, `notify-debouncer-mini` and `ureq` (moved from the Iced
  crate, which no longer names them or `image`). The library's files, the watcher and box art
  downloads are behind it; without it the library keeps nothing, `library::AVAILABLE` and
  `box_art::AVAILABLE` are false, and title screens can still be captured (they only need the core).
- **`Cache`:** where the library keeps `library.toml` and its pictures. `Library::load()` uses the
  player's data folder as before; `Library::load_from(Cache::in_folder(...))` keeps a library in
  another folder, for tests, and every save of the library goes there too (the box art game lists
  stay in the player's data folder). The work the app runs in the background
  for a game's title screen or box art (the saved picture if it is still current, else capture or
  download it) moved with it as `Cache::title_screen` and `Cache::box_art`.
- **Watcher:** `library::watch(folders, games, report)` (`games`, added later, are the recently
  played games outside the folders, whose files it watches too) calls `report` only on the watcher's own thread with
  each `WatchEvent`, and returns `Watching`: a `Watcher` that watches until it is dropped (`None` if
  watching can't start) and the failures from starting, which the caller passes on itself. So
  `report` may block until the app takes the event, while the subscription that empties the channel
  never blocks. `subscriptions::library_changes` keeps its `Subscription::run_with` on the folders,
  sends the startup failures with `.await`, has the watcher's thread send its events into the
  stream's channel and holds the `Watcher` while the subscription runs.
- **`LibraryEntry::example`**, which the Iced crate's library screen tests use, is public now, since
  `cfg(test)` doesn't reach another crate.
- **An existing library:** `tests/existing_data_folder/data/` now also has a `library.toml`, two
  thumbnails, two title screens and a box art, written by the step 4 build through the library's own
  functions. The tests read them back, write them again byte for byte, check that only the game never
  looked at is queued for a title screen and box art, and that a scan keeps the cached entries of
  files whose size and time didn't change instead of reading them again. The hand test on the desktop
  is still to do.

Move `library` (scan, cache, watcher, jobs), `title_screens` and `box_art` into `nubo-frontend`. The
watcher reports through a callback, and the Iced crate wraps it in a subscription.

**Done when:** CI passes, and a library with folders, thumbnails, title screens and box art loads
without rescanning everything.

### Step 6: move running a game

**Done.** What was decided along the way:

- **`session`:** `GameSession` is the game half of `game.rs` (start, playback with fast forward,
  cheats, palette, the save state slot listing with its `slot_listing` numbers and thumbnails, and the
  screen picture). `Session` runs the emulator under it and takes the same `Command`s as before, paced
  by the sound card (`session/sound_card.rs`, the old `emulation/thread.rs`, on every target but
  wasm32) or by the screen's refreshes (`session/screen_refresh.rs`, the old `emulation/web.rs`, on
  wasm32), with `pacing.rs` and `audio.rs` beside them. Which one a build gets still follows
  `target_arch`, as before: the sound card one needs threads, the screen-refresh one the browser's
  `requestAnimationFrame` clock.
- **`run_due_frames`:** the browser session's `advance` is now `Session::run_due_frames()` (and
  `GameSession::run_due_frames`), the name this plan gives it. It takes no timestamp: the session
  keeps reading the refresh times from its own `requestAnimationFrame` clock (`refresh_clock.rs`),
  since Iced's ticks come too unevenly to count refreshes with. A later app with a clock of its own
  (iOS's `CADisplayLink`) would add a timestamp then.
- **Features:** `session` is there with `desktop` (cpal for the desktop's sound) or `web` (cpal's
  WebAudio backend, `cpal/wasm-bindgen`, and `web-sys` for the refresh clock); without either there is
  no sound output and so no session, as the RetroArch core needs none. **`gamepads`** brings gilrs and
  `futures`' executor (the desktop's controller thread waits on the app's channel). The Iced crate no
  longer names cpal, gilrs, `futures` or `web-sys`, and turns on `desktop` and `gamepads` on the
  desktop, `web` and `gamepads` in the browser.
- **New frames:** `FrameListener::listen(callback)` calls the callback on the emulator thread for
  each new, different frame, and once as it starts listening; `FrameListener` still hashes as the
  session's number. `subscriptions::new_frames` keeps its `Subscription::run_with` on the listener,
  and its callback only `try_send`s into a channel of 0, so it never blocks and calls the app hasn't
  handled yet merge into one, exactly as the channel the session used to hold.
- **`gamepads`** keeps `ControllerEvent`, `read_on_thread` (a `futures` channel, which the Iced crate's
  subscription makes) and `next_event` (the browser, once per frame).
- **The overlay** stays in the Iced crate's `game.rs`: `Game` holds the `GameSession` (as its public
  `session` field) with the pause menu page and its highlighted row, the cheat being typed and the
  auto-hiding bar. Opening a save or load page asks the session to list the slots
  (`GameSession::list_slots`), which starts a new listing as the page opening did before.
- **The efficiency harness** (`nubo measure-efficiency`) stays in the Iced crate's `efficiency.rs`,
  where the nightly run calls it, and uses `nubo_frontend::session`. Its numbers are the same before
  and after this step: on `tobudx.gb` and `ucity.gbc --model cgb-e`, 86.2 wake-ups and buffers a
  second while playing (6.0 and 22.8 frames), none while paused, and 344.6 buffers a second while
  fast forwarding (62.8 and 122.0 frames).
- **Tests:** the session's command tests keep their battery saves in a storage in memory instead of
  files, so they run with `web` alone too.

This is the largest step.

1. Split `game.rs` into the game session (start, playback, cheats, palette, save slots, the screen
   picture), which moves, and the overlay (bar, pause menu, mouse), which stays.
2. Move `emulation/`, `audio` and `controllers` (as `gamepads`) into `nubo-frontend`'s `session` and
   `gamepads`.
3. Make new frames reach the app through a callback.

Before starting, record the efficiency harness's numbers (`nubo measure-efficiency`). This step
must not add wake-ups, buffers or frames.

**Done when:**
- CI passes.
- The efficiency numbers are the same.
- On the desktop and in the browser, playing, pausing, fast forward, sound, controllers, saves and
  states all work as before.
- The Iced crate holds only UI: views, theme, input translation, subscriptions and the overlay.

### Step 7: finish the docs

**Done.** `crates/nubo-frontend` has an `AGENTS.md` of its own (how it fits together, the feature
combinations to check, and the fixtures that keep players' data readable); the root `AGENTS.md` points
to it and states the `unsafe` rule with its one planned exception; `BUILDING.md` (once the README) says where the app,
`nubo-frontend` and the tools live; `docs/NIGHTLY_REVIEW.md` names the new folder.

Each step keeps AGENTS.md true as it goes. This step checks the whole picture:

- **AGENTS.md:** the layout, `nubo-frontend`'s modules and features, the pacing, and the `unsafe` rule
  with its one exception.
- **README (now `BUILDING.md`):** where the app and tools live.
- **`docs/NIGHTLY_REVIEW.md`:** any paths it names.
- **This plan:** marked as done.

## Order and dependencies

The order of all three plans, agreed with the maintainer on 2026-10-13:

1. The pull request that adds the plans.
2. Restructuring steps 1 and 2.
3. Part 5 of [SHOOTOUT_PLAN.md](SHOOTOUT_PLAN.md): the Shootout's tests in CI and the nightly run, and
   the score and rank on the wiki. Done.
4. Restructuring steps 3 to 6, then step 7.
5. Parts 1 to 3 of the Shootout plan: the app's options, the harness class, and the maintainer's run on
   Windows. Done.
6. Phases 0 to 5 of [RETROARCH_PLAN.md](RETROARCH_PLAN.md), starting with a review of that plan
   with Codex. The maintainer moved the core ahead of the Shootout's Parts 1 to 3 and asked for it
   straight away; Phases 1 to 5 are built (see that plan's Status).
7. When their conditions are met and the maintainer says to go ahead:
   - the pull request to the Shootout (its Part 4), once Nubo would rank first
   - RetroArch's own core downloader (its Phase 6), once the core has been in a few releases

Why this order:

- **Shootout Part 5 before steps 3 to 6.** It only touches the test ROM runner, the nightly workflow
  and `cargo xtask metrics`, none of which those steps move. It needs step 1, so it is built straight
  into `tools/nubo-test-rom-runner`.
- **Steps 3 to 6 in one go.** They must go in order, and nothing else should change the Iced app's
  non-UI modules meanwhile, or the moves will conflict.
- **Shootout Parts 1 to 3 after the restructuring.** Part 1's `--fresh` turns off the storage, saves
  and library that step 4 moves, so it is built once, on the final structure. The Windows run in
  Part 3 only matters to confirm the score before submitting, and Part 5 already shows the score
  every night.
- **Shootout Parts 1 to 3 before the RetroArch core.** They are small, and they finish the Shootout up
  to the submission, so it can go in as soon as Nubo ranks first.
- **The RetroArch core after the restructuring.** Step 2 already makes `run_frame()` and
  `nubo-frontend`'s palettes and model rule. The core then builds on the finished structure.

## Decisions taken

- The layout and steps are signed off (2026-10-13).
- The Iced app stays the main app. Its package and program stay `nubo`.
- The RetroArch core is planned, after the restructuring and the Shootout's Parts 1 to 3. iOS, GTK and
  a React web UI are only kept possible.
- Every package starts with `nubo`, and every crate's folder is named after its package
  (`frontends/iced` becomes `frontends/nubo`). `xtask` keeps its name.
- Developer programs go in a `tools/` folder, each its own crate.
- All steps are scheduled, in the order under "Order and dependencies", which also places the
  Shootout and RetroArch plans.
- Review dates carry over to moved paths in the nightly review.
