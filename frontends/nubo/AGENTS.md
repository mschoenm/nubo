# Iced frontend (`nubo`)

The app players use: a desktop app built with Iced, which also builds for the browser. Each module's docs explain that module; this guide explains how they fit together.

```bash
cargo play [rom] [--cgb | --model cgb-c]   # Run it
cargo play rom.gb --scale 1 --fresh --speed 4   # As the GB Emulator Shootout runs it (command_line.rs)
cargo test -p nubo                    # Its tests (not a default workspace member)
cargo clippy -p nubo --target wasm32-unknown-unknown   # Lint the browser-only files too
cargo xtask web                       # Build the browser version and serve it at http://127.0.0.1:8080
cargo run --release -p nubo -- --measure-efficiency rom.gb   # Wake-ups, audio buffers and frames per second as JSON (efficiency.rs)
```

## How It Runs

- **The game runs in `nubo_frontend::session`.** The window's `Game` (`game.rs`) holds a `GameSession` (the game itself: playback, cheats, palette, save state slots and the picture on screen) and what the window draws over it: the auto-hiding bar, the pause menu (its Super Game Boy page is `views/game/sgb.rs`, which changes the paused game's colors and border through `GameSession` and shows the new frame the emulator draws for it; the choices are settings, applied whenever a game starts, since save states don't keep them) and the cheat being typed. Under the `GameSession`, the emulator has a thread of its own on the desktop (`session/sound_card.rs` in `nubo-frontend`), paced by the sound card: it sleeps until the audio callback says the queue runs low or the app sends a `Command`, and never polls.
- **The window only redraws for new pictures.** The emulator thread calls back for each new, different frame (`FrameListener::listen`); the callback `subscriptions::new_frames` passes in only tries to send into a channel that holds one message, which its subscription reads, so it never blocks and a still screen draws nothing. What changes with time alone (toasts, the fullscreen bar) is checked by `ticks::every`.
- **This crate is only UI**: the views (`views/`), styles and widgets (`ui/`), the app and its messages (`app.rs` and `app/`), `main.rs`, input translation (`input.rs`, which maps Iced's key codes to `nubo_frontend::controls::KeyCode`, and `menu_presses.rs`), subscriptions (`subscriptions.rs` and `ticks.rs`) and the overlay (`game.rs`), plus the file dialogs (`rom_picker.rs`, with rfd), its own two settings (`settings.rs`, see Where Things Are Kept) and the efficiency harness (`efficiency.rs`). Everything else is in `crates/nubo-frontend`: what the app keeps between runs, the game library, running a game (`session`) and the controllers (`gamepads`). It reports through `futures` channels and callbacks (the frames, the controllers, the library's folder watcher), which `subscriptions.rs` turns into subscriptions, uses `nubo_frontend::time` for clocks and `nubo_frontend::picture::Picture` for pictures (drawn with one cached image handle each, `ui/pictures.rs`, so a picture is uploaded to the graphics card only once).
- **The command line** (`command_line.rs`) can start the app for test harnesses such as the GB Emulator Shootout's. With `--scale` the window shows only the game's picture (`views::game::screen_only`): the Game Boy screen, or on a Super Game Boy the 256×224 picture with its border unless the settings hide it, sized to exact display pixels once it opens (`app::screen_only_size`), and only the Game Boy's buttons and fast forward work. With `--fresh` the app reads and writes nothing kept between runs: default settings that aren't saved, no library, battery saves discarded with the cartridge clock on emulated time (`nubo_frontend::session::Saves::Discarded`), and no save states, cheats or screenshots (`Game::keeps_data`). Either one prints toasts on stderr instead (`Toasts::printed`).
- **Every command wakes the emulator thread**, so `GameSession::apply` only sends pause, speed and volume when they change. After every message, `app.rs` tells the game whether to run, how fast and how loud.
- **Slow work never runs on the UI thread or Iced's task pool**: scans, title screen captures, box art downloads and picture decoding go through `nubo_frontend::background::run`.
- **Messages come in groups** (`GameMessage`, `LibraryMessage`, `SettingsMessage`, `WindowMessage`), each handled in its own file under `app/`. Screens are in `views/`, styles and themes in `ui/`.

## Where Things Are Kept

`nubo-frontend` keeps the data, through its `Storage` trait; with its `desktop` feature, which this app turns on outside the browser, that is files (`storage/files.rs`). Settings (`settings.toml`, with the controls and this app's own two settings, `settings.rs`) are in the per-user config folder; everything else is in the per-user data folder, both picked by the `directories` crate: battery saves (`saves/`), the library cache (`library.toml`, `thumbnails/`, `title_screens/`, `box_art/`), save states (`states/<rom file name>/`) and cheats (`cheats/`). Screenshots go to `Pictures/Nubo` (or `screenshots/` in the data folder on a system without a Pictures folder).

## The Browser Build

- `nubo-frontend`'s session paced by the screen's refreshes (`session/screen_refresh.rs`) replaces the emulator thread: on each frame tick the app calls `run_due_frames`, which emulates the whole frames due by the refresh its own `requestAnimationFrame` clock saw, and `session/pacing.rs` decides how many and keeps the audio queue steady by nudging the sample rate. Controllers are read once per frame (`gamepads::next_event`).
- Settings and battery saves go to localStorage instead of files, and the sound goes through cpal's WebAudio backend: the app turns on `nubo-frontend`'s `web` feature for wasm32 (`storage/local_storage.rs` there names the keys). The library, save states, cheats, screenshots and box art are desktop only (`nubo-frontend`'s `desktop` feature); each of those modules has an `AVAILABLE` flag.
- Code both builds share takes `Instant`, `SystemTime` and `Duration` from `nubo_frontend::time`, which re-exports the `web-time` crate (std's panic on wasm32; `iced::time` is the same types, for the UI code), and text that sets a font uses `theme::BODY_FONT` or `theme::HEADING_FONT`: the browser has no system fonts, so `Font::DEFAULT` or another font of its own draws nothing there. Plain `text()` without `.font()` is fine, and so are the Lucide icons (`ui/icons.rs`), whose font the app loads itself.
- `deploy.yml` publishes the browser build to GitHub Pages on every push to main that passes CI.

## Packaging Files

`linux/` holds the menu entry and the AppStream metainfo, whose `<releases>` list holds the release notes. `windows/nubo.iss` is the Inno Setup script; its `AppId` must never change. `build.rs` gives `nubo.exe` its icon and file properties. They are built by the tasks in `xtask/` (see `xtask/AGENTS.md`).
