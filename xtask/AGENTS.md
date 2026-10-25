# xtask

Project tasks written in Rust, run as `cargo xtask <task>`. Each task's module docs explain it; `cargo xtask help` lists the tasks and `cargo xtask <task> --help` a task's options, read with clap (`Task` in `main.rs`, and the options' types in the tasks' modules). Only Rust is needed, except for the packaging tasks (their platform's tools), `compare-emulators` (git, a C/C++ compiler, make, RGBDS and CMake; Linux and macOS) and `screenshots` (GNOME Shell, Xwayland, xdotool, D-Bus's tools and the desktop portal; Linux).

```bash
# Web version (the Iced app built for the browser)
cargo xtask web               # Build it and serve it at http://127.0.0.1:8080, with the download page at /download/
cargo xtask build-web         # Build it and the download page into target/web/ (what deploy.yml publishes to GitHub Pages)
cargo xtask build-web --quick # Much faster to build, bigger and slower to run (what CI checks)

# Packages and the logo
cargo xtask macos             # Nubo.app into target/bundle/ (macOS)
cargo xtask flatpak           # Build, install for this user and export target/bundle/Nubo-<version>-linux-<arch>.flatpak (Linux, needs flatpak-builder)
cargo xtask windows           # The installer (an MSI) and a zip into target/bundle/ (Windows, needs the WiX Toolset); --nightly <date> names them as a nightly build, --as-version <x.y.z> builds another version (to test updates)
cargo xtask install           # Build Nubo for this computer and install it, closing a running Nubo first (macOS: /Applications; Linux: the Flatpak; Windows: the MSI, quietly); asks what to build unless the working tree is the newest origin/main
cargo xtask install --build main|checkout|0.12|<tag, branch or commit>   # Without asking; anything but the working tree is built in a worktree under target/install/
cargo xtask libretro          # The RetroArch core's zip into target/bundle/ (Linux, Windows; universal on macOS)
cargo xtask icon <path.png|path.svg|path.ico>   # Write the logo (images/icon.png and images/icon.svg in the README)
cargo xtask licenses <folder> # LICENSE, NOTICE and every crate's license for the desktop app
cargo xtask release-notes [v<version>]   # Check Cargo.toml, the newest AppStream release and the tag (if given) agree, and print the notes
cargo xtask rust-version      # Check no crate the app or the RetroArch core is built from needs a newer Rust than rust-version (the Flatpak's)

# Speed next to other emulators (docs/EMULATOR_COMPARISON.md)
cargo xtask compare-emulators [roms]          # Nubo, SameBoy, Gambatte, mGBA, binjgb and Gearboy, best of 3 rounds of 30 s
cargo xtask compare-emulators --instructions  # Instructions per emulated second under Valgrind instead (Linux)

# Nightly review (docs/NIGHTLY_REVIEW.md)
cargo xtask benchmark-roms    # Download the benchmark games into target/benchmark-roms
cargo xtask review-units list   # The parts the review splits the code into
cargo xtask review-units next --state rotation.json --date <YYYY-MM-DD> --commit <sha>   # Pick tonight's part
cargo xtask metrics append metrics.jsonl night.json   # Add a night's measurements to the log
cargo xtask metrics report metrics.jsonl   # Write the Nightly metrics wiki page from the log

# Screenshots (docs/SCREENSHOTS_PLAN.md; Linux, with GNOME Shell, Xwayland, xdotool, D-Bus's tools and the desktop portal)
cargo xtask screenshots [filter]  # Make the screenshots images/screenshots.toml lists from their scenes in a headless GNOME Shell
cargo xtask screenshots --check   # Write nothing: say which differ, with the new pictures and their differences in target/screenshots/check/
cargo xtask scene explore <game | scene> --script "wait 5s; press start; mark" [--every 2s] [--model sgb2]   # Candidate scenes and a contact sheet in target/scenes/
cargo xtask scene keep <candidate> <name>   # Copy a candidate into images/scenes/<name>/

# The GB Emulator Shootout's published table (docs/SHOOTOUT_PLAN.md, Part 5)
cargo xtask shootout-scoreboard <commit>   # Read its gh-pages branch at <commit> into tools/nubo-test-rom-runner/shootout_published.txt
```

## Keep in Mind

- `wasm-bindgen` and `wasm-bindgen-cli-support` are pinned to the same exact version in the root `Cargo.toml`; upgrade them together.
- The Flatpak is built with the freedesktop SDK's Rust and Flathub's newest runtime, as Flathub would build it; when either moves on, `rust-version` in the root `Cargo.toml` and `RUNTIME_VERSION` in `flatpak.rs` are raised by hand (see `docs/CI.md`).
- `APP_ID` (`io.github.mschoenm.nubo`) names the Linux files, the macOS bundle and the window; the Windows installer's `UpgradeCode` in `frontends/nubo/windows/nubo.wxs` must never change.
- Every package carries `LICENSE`, `LICENSE-APACHE`, `NOTICE` and `THIRD_PARTY_LICENSES.txt`; the licenses task fails when a crate has no license text it can use.
- `install` builds anything but the working tree with that version's own `cargo xtask install`, in a worktree under `target/install/` with a build folder of its own there, so only commits that have `xtask/src/install.rs` can be installed; renaming that file means changing `INSTALL_TASK_FILE`. When there is no terminal to ask in (an agent running it), it stops and asks for `--build` instead. It closes a running Nubo with SIGTERM on macOS and Linux, which Nubo handles like its close button, and with the window's close message on Windows.
- The RetroArch core's zip (`libretro.rs`) holds the core under the file name RetroArch looks for, `nubo_libretro.<so|dll|dylib>`, next to `nubo_libretro.info`; `.github/workflows/libretro.yml` builds it on every platform for the nightly build and releases.
- The other emulators of `compare-emulators` are only built and run locally, never distributed or ported from (Gambatte is GPL-2.0-only).
- The screenshots are made by the app itself: `screenshots.rs` runs its hidden `capture` command (`frontends/nubo/src/app/scenes.rs`) for each picture, in a headless GNOME Shell (`screenshots/gnome.rs`) that it starts as root in a mount namespace of its own, so a container without systemd or a screen can run it. The app draws with Iced's software renderer there, which makes every picture the same every time; `--check` relies on that. Pictures with GNOME's border depend on GNOME's version too (46 on Ubuntu 24.04). They aren't made in CI: the `screenshots` skill says when and how.
- The download page (`download_page.rs`) is one hand-written file, `frontends/nubo/web/download/index.html`, published at `download/` next to the web version. Its script puts the download for the visitor's computer first: the app names it in the address (`?for=windows-x64`, `linux-x86_64`, `linux-aarch64` or `macos`, and `&from=<version>`), otherwise the browser's description is used. It reads the version and the file sizes from GitHub's API, and links to the files of the latest release by the names `release.yml` gives them, so it needs no new build for a release; renaming a release file means changing the page, the release notes' table in `release.rs` and the README, and for the Windows zip also `.github/shootout/nubo.py` and the Shootout's copy of it (`.github/shootout/AGENTS.md`). The build copies each `images/` file the page names next to it and fails when one is missing.
- Releases: the `release` skill. CI and nightly runs: `docs/CI.md`.
