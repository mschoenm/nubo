# Iced frontend (`nubo`)

The app players use: a desktop app built with Iced, which also builds for the browser. Each module's docs explain that module; this guide explains how they fit together.

```bash
cargo play [rom] [--cgb | --model cgb-c]   # Run it
cargo test -p nubo                    # Its tests (not a default workspace member)
cargo clippy -p nubo --target wasm32-unknown-unknown   # Lint the browser-only files too
cargo xtask web                       # Build the browser version and serve it at http://127.0.0.1:8080
cargo run --release -p nubo -- --measure-efficiency rom.gb   # Wake-ups, audio buffers and frames per second as JSON (efficiency.rs)
```

## How It Runs

- **The emulator has a thread of its own** (`emulation/thread.rs`), paced by the sound card: it sleeps until the audio callback says the queue runs low or the UI sends a `Command`, and never polls. `emulation.rs` holds what the desktop and browser builds share (`Machine`, `Command`, `Playback`).
- **The window only redraws for new pictures.** The emulator thread reports each new, different frame over a channel (`Session::frame_listener`), which `subscriptions.rs` turns into a subscription; a still screen draws nothing. What changes with time alone (toasts, the fullscreen bar) is checked by `ticks::every`.
- **Only the UI code knows Iced**: `views/`, `ui/`, `app.rs` and `app/`, `main.rs`, `ticks.rs`, `input.rs` and `subscriptions.rs`. What the app keeps between runs (settings, controls, battery saves, save states, cheats, screenshots, and the game library with its scans, folder watcher, title screens and box art), reading ROM files, the notices for the player and `background` are already in `crates/nubo-frontend`; the app keeps only its own two settings (`settings.rs`, see Where Things Are Kept), the file dialogs that pick a ROM or a library folder (`rom_picker.rs`, with rfd) and the library's messages and tasks (`app/library_screen.rs`). The other modules are on their way there (`docs/RESTRUCTURE_PLAN.md`) and use neutral building blocks instead: `futures` channels and callbacks (such as the library's folder watcher) that `subscriptions.rs` turns into subscriptions, `nubo_frontend::time` for clocks, `nubo_frontend::picture::Picture` for pictures (drawn with one cached image handle each, `ui/pictures.rs`, so a picture is uploaded to the graphics card only once), and `nubo_frontend::controls::KeyCode` for keys (`input.rs` maps Iced's key codes to it).
- **Every command wakes the emulator thread**, so `Game::apply` only sends pause, speed and volume when they change. After every message, `app.rs` tells the game whether to run, how fast and how loud.
- **Slow work never runs on the UI thread or Iced's task pool**: scans, title screen captures, box art downloads and picture decoding go through `nubo_frontend::background::run`.
- **Messages come in groups** (`GameMessage`, `LibraryMessage`, `SettingsMessage`, `WindowMessage`), each handled in its own file under `app/`. Screens are in `views/`, styles and themes in `ui/`.

## Where Things Are Kept

`nubo-frontend` keeps the data, through its `Storage` trait; with its `desktop` feature, which this app turns on outside the browser, that is files (`storage/files.rs`). Settings (`settings.toml`, with the controls and this app's own two settings, `settings.rs`) are in the per-user config folder; everything else is in the per-user data folder, both picked by the `directories` crate: battery saves (`saves/`), the library cache (`library.toml`, `thumbnails/`, `title_screens/`, `box_art/`), save states (`states/<rom file name>/`) and cheats (`cheats/`). Screenshots go to `Pictures/Nubo` (or `screenshots/` in the data folder on a system without a Pictures folder).

## The Browser Build

- `emulation/web.rs` replaces the emulator thread: it emulates whole frames on each frame tick, and `emulation/pacing.rs` decides how many and keeps the audio queue steady by nudging the sample rate.
- Settings and battery saves go to localStorage instead of files: the app turns on `nubo-frontend`'s `web` feature for wasm32 (`storage/local_storage.rs` there names the keys). The library, save states, cheats, screenshots and box art are desktop only (`nubo-frontend`'s `desktop` feature); each of those modules has an `AVAILABLE` flag.
- Code both builds share takes `Instant`, `SystemTime` and `Duration` from `nubo_frontend::time`, which re-exports the `web-time` crate (std's panic on wasm32; `iced::time` is the same types, for the UI code), and text that sets a font uses `theme::BODY_FONT` or `theme::HEADING_FONT`: the browser has no system fonts, so `Font::DEFAULT` or another font of its own draws nothing there. Plain `text()` without `.font()` is fine, and so are the Lucide icons (`ui/icons.rs`), whose font the app loads itself.
- `deploy.yml` publishes the browser build to GitHub Pages on every push to main that passes CI.

## Packaging Files

`linux/` holds the menu entry and the AppStream metainfo, whose `<releases>` list holds the release notes. `windows/nubo.iss` is the Inno Setup script; its `AppId` must never change. `build.rs` gives `nubo.exe` its icon and file properties. They are built by the tasks in `xtask/` (see `xtask/AGENTS.md`).
