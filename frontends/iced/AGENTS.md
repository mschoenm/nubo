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
- **The window only redraws for new pictures.** The emulator thread reports each new, different frame through a subscription (`Session::new_frames`); a still screen draws nothing. What changes with time alone (toasts, the fullscreen bar) is checked by `ticks::every`.
- **Every command wakes the emulator thread**, so `Game::apply` only sends pause, speed and volume when they change. After every message, `app.rs` tells the game whether to run, how fast and how loud.
- **Slow work never runs on the UI thread or Iced's task pool**: scans, title screen captures, box art downloads and picture decoding go through `background::run`.
- **Messages come in groups** (`GameMessage`, `LibraryMessage`, `SettingsMessage`, `WindowMessage`), each handled in its own file under `app/`. Screens are in `views/`, styles and themes in `ui/`.

## Where Things Are Kept

Settings (`settings.toml`, with the controls) are in the per-user config folder; everything else is in the per-user data folder, both picked by the `directories` crate: battery saves (`saves/`), the library cache (`library.toml`, `thumbnails/`, `title_screens/`, `box_art/`), save states (`states/<rom file name>/`) and cheats (`cheats/`). Screenshots go to `Pictures/Nubo` (or `screenshots/` in the data folder on a system without a Pictures folder).

## The Browser Build

- `emulation/web.rs` replaces the emulator thread: it emulates whole frames on each frame tick, and `emulation/pacing.rs` decides how many and keeps the audio queue steady by nudging the sample rate.
- Settings and battery saves go to localStorage instead of files. The library, save states, cheats, screenshots and box art are desktop only; each of those modules has an `AVAILABLE` flag.
- Code both builds share takes `Instant`, `SystemTime` and `Duration` from `iced::time` (std's panic on wasm32), and text that sets a font uses `theme::BODY_FONT` or `theme::HEADING_FONT`: the browser has no system fonts, so `Font::DEFAULT` or another font of its own draws nothing there. Plain `text()` without `.font()` is fine, and so are the Lucide icons (`ui/icons.rs`), whose font the app loads itself.
- `deploy.yml` publishes the browser build to GitHub Pages on every push to main that passes CI.

## Packaging Files

`linux/` holds the menu entry and the AppStream metainfo, whose `<releases>` list holds the release notes. `windows/nubo.iss` is the Inno Setup script; its `AppId` must never change. `build.rs` gives `nubo.exe` its icon and file properties. They are built by the tasks in `xtask/` (see `xtask/AGENTS.md`).
