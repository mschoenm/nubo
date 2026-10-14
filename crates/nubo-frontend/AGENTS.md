# nubo-frontend

Everything a Nubo app needs that isn't drawing, so each app (the Iced app in `frontends/nubo` today; the RetroArch core, and maybe an iOS, GTK or web UI later, see `docs/RESTRUCTURE_PLAN.md`) only has to draw and handle input. `src/lib.rs` lists the modules and features; each module's docs explain it. This guide explains how they fit together.

```bash
cargo test -p nubo-frontend --all-features            # Every test, files and localStorage keys included
cargo test -p nubo-frontend --no-default-features      # What the RetroArch core builds: no storage, no session
cargo clippy -p nubo-frontend --all-features --all-targets -- -D warnings
cargo clippy -p nubo-frontend --no-default-features --features web,gamepads --target wasm32-unknown-unknown -- -D warnings
```

`cargo test --workspace` builds it with the features the Iced app turns on for the host (`desktop` and `gamepads`); check the other combinations above when a change touches a feature's code.

## How It Fits Together

- **No UI toolkit.** It never names Iced, so nothing an app shows depends on one. It reports to the app through `futures` channels (controllers) and callbacks (new frames, the library's folder watcher), which the app turns into its own events; the Iced app does that in `frontends/nubo/src/subscriptions.rs`. A callback may run on a thread of this crate's (the emulator thread, the watcher's thread) and must never block a thread the app's executor polls.
- **Features per platform.** `desktop` keeps the data in files and plays the sound through cpal, `web` keeps settings and battery saves in localStorage and plays through WebAudio, and `gamepads` reads controllers with gilrs. Without `desktop` or `web` it keeps nothing (`storage::NoStorage`) and has no `session`, which is how the RetroArch core builds it. Which way a game is paced follows the target: by the sound card on every target with threads (`session/sound_card.rs`), by the screen's refreshes in the browser (`session/screen_refresh.rs`, with `pacing.rs`). A host like RetroArch that calls once per frame needs no session and calls `Emulator::run_frame` itself.
- **Slow work** (scans, title screen captures, box art downloads, picture decoding) goes through `background::run`. On the desktop it runs the work on a thread of its own, so apps keep it off their UI thread; in the browser, which has a single thread, it runs the work in place when the future is first polled, so the work still holds up the page while it runs.
- **Pictures are `picture::Picture`**: plain RGBA pixels that an app turns into its own kind of image once, keeping it with the picture (`Picture::drawn`).

## Players' Data Is Permanent

What this crate stores must read back the same in every later version, as `docs/RESTRUCTURE_PLAN.md`'s goals ask: the same files in the same folders (`ProjectDirs::from("", "", "Nubo")`, `storage/files.rs`), the same `settings.toml`, `library.toml`, `cheats/*.toml`, `.sav`, `.rtc` and `.state` formats, and the same localStorage keys (`storage/local_storage.rs`). The tests guard it:

- `src/settings/tests/settings.toml` is a settings file written by an earlier version; it must read back equal and write back byte for byte.
- `tests/existing_data_folder/` is a whole data folder written by an earlier version (settings, battery saves with a clock, save states with pictures, cheats, the library cache with its pictures); `tests/existing_data_folder.rs` reads it back through the public API and writes it again byte for byte.
- Both are compared byte for byte, so `.gitattributes` keeps Git from changing their line endings on Windows. Never regenerate them with newer code to make a test pass: a failing test there means players would lose data.
