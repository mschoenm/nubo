# Iced frontend (`nubo`)

The desktop app, built with Iced, which also builds for the browser. The rules for code the browser build shares are summarized in the root `CLAUDE.md`; this file has the details.

```bash
cargo play                            # Open a ROM from the toolbar
cargo play path/to/rom.gb             # Run a specific ROM
cargo play path/to/rom.gbc --cgb      # Run in CGB mode
cargo play path/to/rom.gb --model sgb  # Run on another hardware model (dmg-0, dmg, mgb, sgb, sgb2, cgb-0 to cgb-e, agb)
cargo run --release -p nubo -- --measure-efficiency path/to/rom.gb   # Measure the emulator thread's wake-ups, buffers and frames per second (no window), as JSON
cargo test -p nubo                    # Run this crate's tests (it isn't a default member)
cargo clippy -p nubo --target wasm32-unknown-unknown   # Also lint the browser-only files
cargo xtask web                       # Build the browser version and serve it at http://127.0.0.1:8080
```

## Emulator thread and frames

- Emulation runs on its own thread paced by the cpal audio queue (`emulation/thread.rs`'s `EmulatorThread`; what both builds share is in `emulation.rs`: `Machine` carries out each `Command`, and a session starts with the player's `Playback` of pause, fast forward and volume, so nothing plays before they apply): it sleeps until the sound card's callback wakes it as the queue runs low, or a command from the UI wakes it, and never polls.
- It tells the UI about each new frame through a subscription (`Session::new_frames`), skipping frames identical to the one before, so the window redraws for new pictures instead of on every refresh of the screen (not at all on still screens): about 60 times a second at normal speed on any screen, more when fast forwarding (the extra audio buffers are spread over the time one buffer plays, so their frames arrive evenly and fast screens can show them). The browser build keeps `window::frames()`, since its emulation runs on those ticks.
- Whatever changes with time alone (toasts expiring, the fullscreen bar hiding, problems reported from other threads) is checked by `ticks::every` (`App::check_interval`: four times a second while toasts or the bar need it, once a second while a game runs), since nothing else redraws a still screen.
- Each session's frame subscription is keyed by a session number, not an address, so a restarted game always gets a new one.
- While the game is paused the desktop build also pauses the sound stream, and `Game::apply` only sends pause, speed and volume when they change, since every command wakes the emulator thread.
- `game.rs` holds the running game and its overlay state (auto-hiding top bar, pause menu, fast forward); the emulator thread runs `speed` audio buffers per played one to fast forward. The picture (`Screen`) is copied once per new frame, and a copy as the emulator drew it is only kept while an original Game Boy game is shown in a palette other than gray.
- On Windows a minimized window reports a zero size and stops taking frames while the game plays on; macOS and Linux give no such signal.
- Problems on the emulator thread go through `notices.rs` to toasts instead of only stderr.

## Battery saves

Battery-backed RAM and RTC state are persisted by `battery.rs` as `<rom name>.sav` and `<rom name>.rtc` in the per-user data folder picked by the `directories` crate (`~/Library/Application Support/Nubo/saves` on macOS, `%APPDATA%\Nubo\data\saves` on Windows, `$XDG_DATA_HOME/nubo/saves` on Linux), written once the game's save writes have settled for 250 ms (at most 1 s after the first one), when the game pauses and when the session ends. Iced 0.14 has no hook for quitting, so on macOS Cmd+Q and Dock > Quit probably end the app without `stop_game` (unverified): battery saves are on disk by then, but the Resume slot and thumbnail aren't written.

## Input

- Controllers are read with gilrs on a thread of their own that blocks until a controller reports something, with gilrs's force feedback off (`controllers.rs`, on every frame in the browser build) and reach the app as a subscription, so they work on every screen; the app forwards their state to the emulator thread, hiding buttons pressed while the game isn't running until they are released (`menu_presses.rs`), so the A that picks "Resume" doesn't reach the game. gilrs's Windows backend polls every 8 ms on its own thread whatever the app does.
- The player binds one key and one controller button per Game Boy button and shortcut (`controls.rs`, saved in `settings.toml`); Game Boy buttons go to the core as a `ControllerMapping`, shortcuts such as the pause menu are handled by the app.
- The bound D-pad, A, B, Start and Select keys and buttons also move through the library, the pause menu and the settings window (`App::navigate`); the settings rows the D-pad visits are listed by `views::settings::rows`, in the order the page shows them, and the view outlines the focused one; the library's column count and card positions are worked out in `views/library.rs` to match Iced's grid, so the selected card can be scrolled into view.

## App structure and UI

`app.rs` routes between the library screen and the game screen (`views/`); its messages come in groups (`GameMessage`, `LibraryMessage`, `SettingsMessage`, `WindowMessage`), each handled in its own file under `app/` (`playing.rs`, `library_screen.rs`, `settings_window.rs`, `window_events.rs`, and `player_input.rs` for keys and controllers), and after every message the game is told whether to run, how fast and how loud (`Game::apply` only sends what changed), with the settings window, drop target and toasts layered on top; widget styles and the light/dark themes live in `ui/` (icons are Lucide, via the `lucide-icons` font). Settings are saved as `settings.toml` in the config folder (`settings.rs`).

The logo (a pixel buddy puffed into a cloud, on a purple rounded square) is defined once as rounded rectangles in `crates/nubo-logo`; the app draws it with a canvas in its theme's accent (`ui/logo.rs`), renders it as the window icon, and `cargo xtask bundle` renders it into the `.icns` of `Nubo.app` (macOS only, using `iconutil`, signed ad hoc).

## Library

- The library (`library.rs`) scans the user's ROM folders in the background with `Emulator::read_rom_header`, caches what it found in `library.toml` and keeps a PNG of each game's last screen in `thumbnails/`, both in the data folder.
- At startup a quick check (`scan::check`, existence only) forgets games whose file was deleted before the full scan runs. A game whose library folder (or, outside the folders, whose own folder) is gone, such as on an unplugged drive, is kept but hidden as unavailable, so it comes back intact.
- `library/watcher.rs` watches the folders with `notify-debouncer-mini` (one second of quiet before reporting, ROMs and folders only) and rescans on changes; the Rescan buttons, the watcher and folder changes all go through `App::rescan`, and only one scan runs at a time, with requests during it leading to one more scan after it.
- Title screens and box art saved after the ROM last changed are reused instead of being made again.
- Library cards show each game's title screen, captured by `title_screens.rs`: after a scan, games without one are run for 10 emulated seconds with no window or sound (about half a second each, two at a time) and the frame with the most detail is kept in `title_screens/` in the data folder.
- With "Download box art" on (on by default, Settings > Library), `box_art.rs` also looks up each game in the libretro thumbnail collection, four at a time with `ureq`: by file name, then through the No-Intro game lists from the libretro database (downloaded once to `box_art/lists`) by file name and CRC32. Pictures are shrunk to 256 px and kept in `box_art/`; a failed download stops the queue until the next scan.
- Cards show box art, then the title screen, then the last screen played. Card pictures are drawn at a whole-number scale on a dark frame, and DMG pictures use the chosen palette.
- Slow work (scans, title screen captures, box art lookups, reading the game lists and decoding saved pictures) runs on threads of its own through `background::run` (a thread that can't start or work that panics comes back as an error, so no job slot stays taken), never on Iced's shared task pool or the UI thread, and pictures reach the view already decoded (`Library::load_pictures` loads the one picture each card shows); the game lists are only read when a game still needs its box art looked up.

## Save states, cheats, screenshots and palettes

- Save states (`save_states.rs`) live in `states/<rom file name>/` in the data folder as `<slot>.state` plus a `<slot>.png`; with "Continue games where you left off" on (off by default, Settings > Playing), the `resume` slot is written when a game stops and loaded when it starts again. A `.state` file is exactly the core's save state; the model comes from its header. A state that can't be loaded is renamed to `<slot>.kept-<number>.state` (and its picture with it), so the next save to the slot, such as the `resume` slot when the game stops, can't overwrite it.
- Cheats are kept per game in `cheats/<rom file name>.toml` (`cheats.rs`) and sent to the emulator thread when they change.
- Screenshots go to `Pictures/Nubo` (`screenshots.rs`).
- DMG palettes (`palettes.rs`) are applied by the frontend, which swaps the four grays of DMG frames (`nubo_core::DMG_SHADES`) for the chosen colors, so the core always draws gray.

## Browser build

- It also builds for wasm32 (`cargo xtask web`), which `deploy.yml` publishes to GitHub Pages on every push to main that passes CI: there `emulation/web.rs` replaces the emulator thread and emulates the frames due on each frame tick, one whole frame at a time (the browser's sound takes 2,048 samples at a time, so filling the queue whenever it ran low emulated frames in bursts and showed only the last of each).
- `emulation/pacing.rs` decides how many are due: on screens within 1% of 60, 120 or 240 Hz frames are locked to the refreshes (one every 1, 2 or 4, or two per refresh when a browser saving power draws 30 times a second), counted from the `requestAnimationFrame` timestamps `web/refresh_clock.rs` records (Iced's ticks reach the app milliseconds later by varying amounts), so the game runs about 0.45% fast there; on other screens they go by the average time between ticks.
- The audio queue, sized from the sound buffer cpal reports, never changes when a frame runs: `SampleRateControl` keeps its average length at its target by setting the emulator's sample rate up to 1% off (besides making up for a locked game's speed).
- ROMs are read as bytes through rfd's browser file picker, and battery saves go to localStorage (`battery/local_storage_slot.rs`, keyed by ROM file name) instead of files (`battery/file_slot.rs`), flushed as soon as a burst of save writes settles.
- The browser has no system fonts, so the browser build only has the Fira Sans that Iced bundles; text that sets a font must use `theme::BODY_FONT` or `theme::HEADING_FONT`, never `Font::DEFAULT`, or it draws nothing there.
- Code the browser build shares must take `Instant`, `SystemTime` and `Duration` from `iced::time`, since std's panic on wasm32-unknown-unknown.
- The library, save states, cheats, screenshots and box art are desktop only (each module has an `AVAILABLE` flag).

## Efficiency measurement

`efficiency.rs` (desktop only, `nubo --measure-efficiency <rom>`) plays a game without a window or sound while playing, paused, fast forwarding and on a still screen, and prints the emulator thread's wake-ups, audio buffers and frames per second as JSON (`Session::activity` counts them), keeping battery saves in a temporary folder; the nightly metrics track it as the battery cost.

## Packaging files

`linux/` holds the menu entry and the AppStream metainfo (which also holds the release notes), `windows/nubo.iss` the Inno Setup script (its `AppId` must never change), and `build.rs` gives `nubo.exe` its icon and file properties. How they are built is described in `xtask/CLAUDE.md`.
