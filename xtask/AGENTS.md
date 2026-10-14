# xtask

Project tasks written in Rust, run as `cargo xtask <task>`. Each task's module docs explain it. Only Rust is needed, except for the packaging tasks (their platform's tools) and `compare-emulators` (git, a C/C++ compiler, make, RGBDS and CMake; Linux and macOS).

```bash
# Web version (the Iced app built for the browser)
cargo xtask web               # Build it and serve it at http://127.0.0.1:8080
cargo xtask build-web         # Build it into target/web/ (what deploy.yml publishes to GitHub Pages)
cargo xtask build-web --quick # Much faster to build, bigger and slower to run (what CI checks)

# Packages and the logo
cargo xtask bundle            # Nubo.app into target/bundle/ (macOS)
cargo xtask flatpak           # Build, install for this user and export target/bundle/Nubo-<version>-linux-<arch>.flatpak (Linux, needs flatpak-builder)
cargo xtask windows           # The installer and a zip into target/bundle/ (Windows, needs Inno Setup 6)
cargo xtask libretro          # The RetroArch core's zip into target/bundle/ (Linux, Windows; universal on macOS)
cargo xtask icon <path.png|path.svg|path.ico>   # Write the logo (images/icon.png and images/icon.svg in the README)
cargo xtask licenses <folder> # LICENSE, NOTICE and every crate's license for the desktop app
cargo xtask release-notes [v<version>]   # Check Cargo.toml, the newest AppStream release and the tag (if given) agree, and print the notes

# Speed next to other emulators (docs/EMULATOR_COMPARISON.md)
cargo xtask compare-emulators [roms]          # Nubo, SameBoy, Gambatte, mGBA, binjgb and Gearboy, best of 3 rounds of 30 s
cargo xtask compare-emulators --instructions  # Instructions per emulated second under Valgrind instead (Linux)

# Nightly review (docs/NIGHTLY_REVIEW.md)
cargo xtask benchmark-roms    # Download the benchmark games into target/benchmark-roms
cargo xtask review-units [next --state rotation.json --date <YYYY-MM-DD> --commit <sha>]   # The parts the review splits the code into, or tonight's
cargo xtask metrics append metrics.jsonl night.json   # Add a night's measurements to the log
cargo xtask metrics report metrics.jsonl   # Write the Nightly metrics wiki page from the log

# The GB Emulator Shootout's published table (docs/SHOOTOUT_PLAN.md, Part 5)
cargo xtask shootout-scoreboard <commit>   # Read its gh-pages branch at <commit> into tools/nubo-test-rom-runner/shootout_published.txt
```

## Keep in Mind

- `wasm-bindgen` and `wasm-bindgen-cli-support` are pinned to the same exact version in the root `Cargo.toml`; upgrade them together.
- The Flatpak is built with the freedesktop SDK's Rust and Flathub's newest runtime, as Flathub would build it; when either moves on, `rust-version` in the root `Cargo.toml` and `RUNTIME_VERSION` in `flatpak.rs` are raised by hand (see `docs/CI.md`).
- `APP_ID` (`io.github.mschoenm.nubo`) names the Linux files, the macOS bundle and the window; the Windows installer's `AppId` in `frontends/nubo/windows/nubo.iss` must never change.
- Every package carries `LICENSE`, `LICENSE-APACHE`, `NOTICE` and `THIRD_PARTY_LICENSES.txt`; the licenses task fails when a crate has no license text it can use.
- The RetroArch core's zip (`libretro.rs`) holds the core under the file name RetroArch looks for, `nubo_libretro.<so|dll|dylib>`, next to `nubo_libretro.info`; `.github/workflows/libretro.yml` builds it on every platform for the nightly build and releases.
- The other emulators of `compare-emulators` are only built and run locally, never distributed or ported from (Gambatte is GPL-2.0-only).
- Releases: the `release` skill. CI and nightly runs: `docs/CI.md`.
