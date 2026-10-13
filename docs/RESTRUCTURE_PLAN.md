# Plan: restructuring the repository for more frontends

**Status: draft, waiting for the maintainer's sign-off. Don't implement any of it before then.**

Nubo has one real app today, the Iced frontend, which runs on the desktop and in the browser. Planned
or considered next are:

- a RetroArch core ([RETROARCH_PLAN.md](RETROARCH_PLAN.md))
- an iOS app
- a GTK app
- maybe a web UI in React

Each of them needs much of what the Iced crate holds today besides its views: running a game at the
right pace, battery saves, save states, cheats, palettes, settings, key bindings and the library. This
plan moves that into a shared crate, so a new app only has to draw and handle input.

## Goals

1. Every app builds on `nubo-core` (the emulator) and a new **`nubo-frontend`** crate (everything an app
   needs that isn't drawing), and nothing else of Nubo's.
2. `nubo-frontend` doesn't depend on Iced or any other UI toolkit. How a game is paced is something the
   app plugs in.
3. Apps players use live in `frontends/`. Tools for developers live in `tools/`.
4. Nothing changes for players:
   - The same files are kept in the same places, in the same formats: `settings.toml`, `library.toml`,
     `cheats/*.toml`, `.sav`, `.rtc`, `.state` and the pictures. The folders stay the ones
     `ProjectDirs::from("", "", "Nubo")` picks.
   - The same app id (`io.github.mschoenm.nubo`), the same `nubo.exe` and the same installer `AppId`.
   - The same behaviour.
5. Every step is one pull request that leaves `main` working, with nothing half moved.

Not goals: changing the emulator, changing how the Iced app looks or behaves, or building a new app.
Those come after this plan, each with a plan of its own.

## Where things are today

Counted per file in `frontends/iced/src` (about 13,000 lines with tests):

- **UI code, which stays in the Iced crate:** `views/`, `ui/`, `app.rs` and `app/`, `main.rs`, `ticks.rs`
  and `input.rs`. That is about a third of the lines.
- **Already free of Iced:** `palettes`, `cheats`, `rom`, `notices`, `screenshots`, `storage`,
  `title_screens`, `box_art/download`, `library/scan`, `library/jobs`, `menu_presses`, `efficiency`,
  `emulation/web.rs` and the slots behind `battery`, `settings`, `cheats`, `library` and `save_states`
  (`file_*`, `local_storage_*`, `no_store`).
- **Using only small things from Iced:**

  | What | Where | Replacement |
  |---|---|---|
  | `iced::time` (`Instant`, `SystemTime`, `Duration` that also work in the browser) | `battery`, `audio`, `emulation`, `pacing`, `game`, `save_states`, `library` | The [`web-time`](https://crates.io/crates/web-time) crate (MIT/Apache), which is what makes these work in the browser in the first place |
  | `iced::futures` channels and `Subscription` | `emulation/thread.rs` (new frames), `controllers/thread.rs`, `library/watcher.rs`, `background.rs` | Callbacks or plain channels in the shared crate; the Iced crate turns them into subscriptions |
  | `iced::widget::image::Handle` | `library` (thumbnails, title screens, box art), `save_states`, `game` (the screen) | A neutral `Picture` (width, height, RGBA bytes); the Iced crate makes and caches one `Handle` per picture |
  | `iced::keyboard::key::Code` | `controls` (key bindings, stored in `settings.toml` by name) | Nubo's own `KeyCode` enum that stores the same names, so existing `settings.toml` files read the same |

- **Mixed:** `game.rs` holds both the running game (playback, cheats, palette, save slots) and the
  overlay (auto-hiding bar, pause menu, mouse). `settings.rs` holds both app-wide settings and two that
  only matter to the Iced app (`appearance`, `integer_scaling`).

## Target layout

```
crates/
  nubo-core/            the emulator (unchanged)
  nubo-frontend/        everything an app needs that isn't drawing (new)
  nubo-logo/
  serializable_derive/
  nubo-ffi/             bindings for Swift (and Kotlin) through UniFFI, for the iOS app (later)
  nubo-wasm/            bindings for JavaScript/TypeScript through wasm-bindgen, for a web UI (later, if at all)
frontends/              apps players use
  iced/                 the main app: views, theme, Iced input and subscriptions
  libretro/             the RetroArch core
  gtk/                  (later)
  ios/                  Swift app on nubo-ffi (later)
tools/                  for developers, not players
  benchmark/  headless/  json_test_runner/  save_state_check/  test_rom_runner/
xtask/
```

### `nubo-frontend`

One crate with modules, not several small crates: they change together, and one crate is easier to
review and version. It can be split later if a part grows a life of its own.

| Module | From | Notes |
|---|---|---|
| `time` | `iced::time` | Re-exports `web-time` |
| `storage` | `storage/`, `*/file_store.rs`, `*/local_storage*.rs`, `*/no_store.rs` | A `Storage` trait with files (desktop), localStorage (browser) and the app's sandbox (iOS, later) |
| `settings` | `settings.rs` | App-wide settings as data, read and written as `settings.toml` exactly as now. Settings only one app uses (`appearance`, `integer_scaling`) stay in a section that app owns, so the file keeps its shape |
| `controls` | `controls.rs` | Actions, bindings and Nubo's own `KeyCode` and `PadButton` |
| `palettes` | `palettes.rs` | Also used by the RetroArch core |
| `model` | `ModelPreference` in `settings.rs` | The rule that picks a game's model. Also used by the RetroArch core |
| `cheats`, `battery`, `save_states`, `screenshots`, `rom`, `notices` | Same names | |
| `library` | `library.rs`, `library/` | Scan, cache, folder watcher (behind `desktop`), title screens, box art (behind `desktop`). Pictures as `Picture` |
| `session` | `emulation.rs`, `emulation/`, `audio.rs`, the game half of `game.rs` | Running a game. See "Pacing" below |
| `gamepads` | `controllers.rs`, `controllers/` | gilrs, behind a feature, since iOS has its own controller API |
| `background` | `background.rs` | Runs slow work on a thread of its own and hands the result back as a future |

Feature flags keep platform dependencies out of builds that can't use them:

- **`desktop`:** cpal, directories, notify-debouncer-mini, ureq, image, crc32fast.
- **`gamepads`:** gilrs.
- **`web`:** localStorage and the browser's sound.

The Iced crate turns on what it uses today. The RetroArch core turns on none: it only needs `palettes`
and `model`.

### Pacing

Today the desktop's emulator thread is paced by the sound card, and the browser build runs the frames
due on each screen refresh. The new apps add a third way:

| Pacing | Who uses it | What drives it |
|---|---|---|
| **Sound card** | Iced on the desktop (as now), GTK | The cpal callback wakes a thread when its queue runs low |
| **Screen refresh** | Iced in the browser (as now), iOS, GTK, a React UI | The app calls `run_due_frames(timestamp)` on each refresh (`requestAnimationFrame`, `CADisplayLink`, GTK's frame clock); `pacing.rs` decides how many frames are due |
| **Host** | RetroArch | RetroArch calls `retro_run` once per frame and does its own timing |

The first two become two implementations behind one `Session` that takes the same `Command`s as today.
The third needs no session, only `Emulator::run_frame()` in the core (Phase 1 of the RetroArch plan),
which the screen-refresh pacing can use too. A new frame is announced through a callback the app
passes in, not an Iced `Subscription`. The Iced crate's callback pushes into the channel its
subscription reads, as now.

### The `unsafe` rule

`nubo-core`, `nubo-frontend` and the apps written in Rust keep `unsafe_code = "forbid"`. Code that talks
to other languages can't do without it:

- the RetroArch core's exported C functions
- the code UniFFI and wasm-bindgen generate

That code goes in `frontends/libretro/src/ffi.rs`, `nubo-ffi` and `nubo-wasm` only, each with its own
lint table and the `unsafe` in as few places as possible. CLAUDE.md's style rule would say exactly
that. Whether the generated code trips the lint needs checking when those crates are made.

## Steps

Each step is one pull request. Each must pass the full CI and leave every fingerprint the same:
`cargo benchmark --compare` on the games and test ROMs, `cargo test-roms`, `cargo save-states`, and
the Iced app's tests. The emulator doesn't change in any of them except step 2's added method.

### Step 1: tools to `tools/`

Move `benchmark`, `headless`, `json_test_runner`, `save_state_check` and `test_rom_runner` from
`frontends/` to `tools/`. Package names don't change, so the cargo aliases (`cargo test-roms` and the
rest) keep working. Then update everything that names the old paths:

- the root `Cargo.toml` members
- `xtask/src/review_units.rs` (`CRATE_FOLDERS` gets `"tools"`)
- `.github/actions/test-roms/action.yml` (the cache key's `hashFiles` paths)
- the messages in `test_rom_runner/src/download.rs`, `json_test_runner/src/download.rs` and
  `save_state_check/src/main.rs`
- `save-states/README.md`, the README (links to `passing.txt` and `FAILING_TESTS.md`), CLAUDE.md, and
  the other plans in `docs/`

**Nightly review:** its rotation remembers review dates by file path, so moved files look unreviewed and
come up again sooner. Either accept that, or give `review-units` a one-off list of renamed paths
(`git diff --name-status -M` between the commits) and carry their dates over. Recommended: carry them
over, since it is a few lines.

**Done when:** CI passes, `cargo xtask review-units` lists the moved crates, and the test ROM cache key
still hits.

### Step 2: the shared pieces the RetroArch core needs

1. Add `Emulator::run_frame()` to the core, as described in the RetroArch plan.
2. Create `crates/nubo-frontend` with only `palettes` and `model`, moved from the Iced crate. The Iced
   crate depends on it.

This lets the RetroArch work start without waiting for the rest. Update RETROARCH_PLAN.md's Phase 1 to
take palettes and the model rule from `nubo-frontend` instead of `nubo-core`.

**Done when:** CI passes and the Iced app shows the same palettes and picks the same models.

### Step 3: neutral building blocks

Without moving modules yet, take Iced out of the parts that don't draw:

- `web-time` instead of `iced::time`
- `futures` channels (or callbacks) instead of `iced::futures`
- `Picture` instead of `image::Handle` outside the views, with one `Handle` cached per picture in the
  Iced crate
- `KeyCode` instead of `iced::keyboard::key::Code` in `controls.rs`, with a mapping in `input.rs`

Add a test that a `settings.toml` written by the current version (a fixture in the tests) reads back to
the same settings and writes back byte for byte.

**Done when:** `grep -rl iced` in the non-UI modules finds nothing, CI passes, the browser build works
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

### Step 7: update the docs

- **CLAUDE.md:** the new layout, `nubo-frontend`'s modules and the `unsafe` rule.
- **README:** where the app lives.
- **`docs/NIGHTLY_REVIEW.md`:** if it names paths.

This can ride along with each step instead. Each step should at least leave CLAUDE.md true.

## Order and dependencies

- Start after the CI rebuild in progress has landed. Steps 1 and 6 touch workflow and action files it
  is changing.
- Steps 1 and 2 are small and independent, and unblock the RetroArch core.
- Steps 3 to 6 must go in order. Nothing else should change the Iced app's non-UI modules meanwhile,
  or the moves will conflict.
- **Shootout plan:** Part 1's options (`--scale`, `--fresh`, `--speed`) can go before step 3 or after
  step 6, but not in the middle. Before is simpler: they are small, and `--fresh` then moves with
  `storage` like everything else.
- New apps (GTK, iOS, React) start after step 6, each with its own plan. The first one will show what
  `nubo-frontend` is still missing, so its API isn't designed further ahead than these steps need.

## Decisions for the maintainer

1. **Sign off on the layout and steps above**, or say what to change.
2. **Names:**
   - `nubo-frontend` for the shared crate (alternatives: `nubo-app`, `nubo-player`).
   - `tools/` for the developer programs.
3. **"EOS":** this plan assumes iOS. If it means something else (elementary OS is itself a GTK
   desktop), the `nubo-ffi` part changes.
4. **React:** the Iced app already runs in the browser, so a React UI would be a second UI for the same
   platform. It is worth it for a site that should feel like a website (links, search engines,
   accessibility, fast first load). Otherwise `nubo-wasm` can stay out of the plan.
5. **GTK:** is it meant as the Linux app, replacing the Iced Flatpak there, or as an alternative next to
   it? That decides its app id, Flatpak and AppStream files.
6. **Nightly review:** carry review dates over to the moved paths (recommended), or accept that moved
   files come up for review again.
