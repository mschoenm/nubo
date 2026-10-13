# xtask

Project tasks written in Rust: the web build, packaging, licenses, the speed comparison with other emulators and the nightly review's tooling. Releases are covered by the `release` skill (`.claude/skills/release/SKILL.md`), CI by `docs/CI.md`. The packaging tasks need their platform's tools, and `cargo xtask compare-emulators` builds other emulators from their C and C++ sources; nothing else needs them.

```bash
# Web version (the Iced frontend built for the browser)
cargo xtask web               # Build it and serve it at http://127.0.0.1:8080
cargo xtask build-web         # Build it into target/web/ (what deploy.yml publishes to GitHub Pages)
cargo xtask build-web --quick # Build it much faster, but bigger and slower to run (what CI checks)

# macOS app bundle, Flatpak, Windows installer and logo
cargo xtask bundle            # Build Nubo.app (release build, Dock icon) into target/bundle/
cargo xtask flatpak           # Build the app as a Flatpak, install it for this user and write target/bundle/Nubo-<version>-linux-<arch>.flatpak (Linux, needs flatpak-builder)
cargo xtask windows           # Build target/bundle/Nubo-<version>-windows-x64-setup.exe and a zip to run without installing (Windows, needs Inno Setup 6)
cargo xtask icon path.png     # Write the logo as a 256 px PNG (images/icon.png in the README)
cargo xtask icon path.svg     # Write the logo as an SVG (images/icon.svg)
cargo xtask icon path.ico     # Write the logo as a Windows icon with every size
cargo xtask licenses folder   # Write LICENSE, NOTICE and the crates' licenses for the desktop app into folder
cargo xtask release-notes v0.2.0   # Check the tag, Cargo.toml and the newest AppStream release agree, and print its notes

# Speed next to other emulators (see docs/EMULATOR_COMPARISON.md; Linux and macOS, needs git, a C/C++ compiler, make, RGBDS and CMake)
cargo xtask compare-emulators                  # Time Nubo, SameBoy, Gambatte, mGBA, binjgb and Gearboy on the benchmark games (best of 3 rounds of 30 s)
cargo xtask compare-emulators ~/Roms/GameBoy   # The same on other games
cargo xtask compare-emulators --instructions   # Count the instructions inside each emulator with Valgrind instead (Linux), the same however busy the machine is

# Nightly review (see docs/NIGHTLY_REVIEW.md)
cargo xtask benchmark-roms    # Download the free homebrew games the nightly CI benchmarks into target/benchmark-roms
cargo xtask review-units      # List the parts the nightly review splits the code into
cargo xtask review-units next --state rotation.json --date 2026-10-14 --commit <sha>   # Pick tonight's part
cargo xtask metrics report metrics.jsonl   # Write the Nightly metrics wiki page from the log
```

## Web build

`cargo xtask web` and `build-web` install the `wasm32-unknown-unknown` target if needed, build `nubo` with the `wasm-release` profile (with `--quick` the much faster `release` profile, which CI checks the browser build with), and generate the JavaScript glue with the `wasm-bindgen-cli-support` library next to `frontends/iced/web/index.html`, so neither wasm-pack nor Node.js is needed. `wasm-bindgen` and `wasm-bindgen-cli-support` are pinned to the same exact version in the root `Cargo.toml` (the Iced crate depends on the pinned `wasm-bindgen` for this) and must be upgraded together.

## Flatpak

- `flatpak` (`flatpak.rs`) writes a manifest (the freedesktop runtime with its Rust SDK extension, and the sandbox's permissions in `FINISH_ARGS`), every crate of `Cargo.lock` as a flatpak-builder source (Flatpak builds are offline; this is what Flathub's flatpak-cargo-generator writes), the SVG icon and the license files to `target/flatpak/`, then builds the working tree with flatpak-builder, installs it for the current user (adding a user Flathub remote for the runtime if needed) and exports it as a single file, `target/bundle/Nubo-<version>-linux-<arch>.flatpak`, that others install by opening it (it points to Flathub for the runtime).
- The menu entry and the AppStream metainfo live in `frontends/iced/linux/` (`LINUX_FILES`) and are named after `APP_ID` (`io.github.mschoenm.nubo`, also the macOS bundle identifier), which the Iced frontend sets as its window's app id: GNOME on Wayland ignores window icons and shows the icon of the installed menu entry that matches the window.
- The metainfo's screenshot, `images/flatpak/tobu-tobu-girl-deluxe.png`, is linked by its raw.githubusercontent.com address on `main`, so it only loads while the repository is public; it shows a CC BY 4.0 homebrew game, credited in the description and the README.

## Windows

`windows` (`windows.rs`) builds `nubo` in release mode and puts it with the license files into an Inno Setup installer (`frontends/iced/windows/nubo.iss`: per user, no administrator rights, Start menu entry, uninstaller, optional "Open with" for `.gb`/`.gbc`; its `AppId` must never change) and a zip; `frontends/iced/build.rs` gives `nubo.exe` the logo as its icon (`nubo_logo::ico`) and its file properties with `winresource`, and release builds use the Windows subsystem so no console window opens.

## Build commit and licenses

- Every cargo build xtask runs, the Flatpak's included, gets `NUBO_COMMIT`: the commit of the working tree (with `-dirty` after it when tracked files have uncommitted changes, `GITHUB_SHA` without git), which `nubo_core::BUILD_COMMIT` holds and save states store, so every release and nightly build's states say exactly which build made them; a plain `cargo build` leaves it empty.
- `bundle`, `flatpak`, `windows` and `build-web` put copies of `LICENSE`, `LICENSE-APACHE` and `NOTICE` (as `.txt`) next to the app, with `THIRD_PARTY_LICENSES.txt` written by `licenses.rs` from `cargo metadata`: the license files of every crate the app is built for that target from, or the standard text of their license for crates that ship none (it fails when a crate has neither).

## Nightly review tooling

`benchmark-roms`, `review-units` and `metrics` serve the nightly review (`docs/NIGHTLY_REVIEW.md`):

- the first downloads three homebrew games (Tobu Tobu Girl Deluxe, µCity, Geometrix) from the gbdev Homebrew Hub database at pinned commits with checksums;
- the second splits the Rust code into parts of at most 1,200 lines from the file tree as it is (a module with its folder when it fits, else its file and its children, small neighbours packed together) and picks the one whose least recently reviewed file was reviewed longest ago, remembering dates per file so regrouping loses nothing;
- the third keeps the metrics log (one JSON object per night) and renders its wiki page, flagging values worse than their 14-night median by more than their threshold.

## Emulator comparison

`compare-emulators` (`compare_emulators.rs`, its fetching and building in `compare_emulators/build.rs`) fetches SameBoy, Gambatte (gambatte-core), mGBA, binjgb and Gearboy with git at pinned commits into `target/emulator-comparison`, builds each with its own release flags together with a harness from `xtask/emulator_harnesses/` that runs a ROM the way nubo-benchmark runs it in Nubo (`harness.h`: 44,100 Hz audio in buffers of 512 samples, the same scripted buttons, every frame drawn, only the time inside `run_buffer` counted), and prints a table of each emulator's best round, or with `--instructions` the instructions callgrind counts inside each emulator per emulated second; Nubo runs in nubo-benchmark, also with the `eager` feature. It is the one task that needs C and C++ tools, and leaves out an emulator whose tools are missing. The other emulators are only built and run locally, never distributed or ported from (Gambatte is GPL-2.0-only).
