# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository. It holds what every change needs; the details of each part live next to the code (see Where Things Are).

## Project Overview

Nubo (formerly RetroBoy, a fork of smparsons/retroboy) is a cycle-accurate Game Boy (DMG) and Game Boy Color (CGB) emulator written in Rust. The core emulator runs through an Iced frontend, natively on the desktop and compiled to WebAssembly in the browser. Nubo is licensed under GPL-3.0-or-later (`LICENSE`); the code that came from RetroBoy remains Apache-2.0 (`LICENSE-APACHE`), and `NOTICE` keeps its attribution.

## Build Commands

Everything is driven by cargo and works on Windows, macOS, and Linux with only a Rust toolchain installed (the newest stable, which `rust-toolchain.toml` asks for; `rustup update` gets it; Linux additionally needs `libasound2-dev`, `libudev-dev` and `pkg-config` for audio and controllers). The commands below are aliases defined in `.cargo/config.toml`. Packaging, the web build, the emulator comparison and the nightly review tooling are listed in `xtask/CLAUDE.md`.

```bash
cargo build                   # Build the core crates (default workspace members)
cargo test                    # Run core emulator tests
cargo check --workspace       # Check every crate, including frontends and xtask
cargo clippy --workspace --all-targets   # Lint every crate; must stay free of warnings
cargo clippy -p nubo --target wasm32-unknown-unknown   # Also lint the browser-only files of the Iced frontend
cargo test -p nubo            # Run the Iced frontend's tests (it isn't a default member)
cargo fmt --all               # Format with rustfmt
cargo test-roms               # Run the test ROM suites and compare with the list of passing tests
cargo test-roms mooneye       # Only run tests whose name contains "mooneye" (e.g. "cgb blargg", "dmg mealybug")
cargo test-roms --update      # Record the results of this run in the list of passing tests
cargo cpu-tests               # Run the JSON CPU tests (every opcode, checked cycle by cycle)
cargo save-states             # Check that the kept save states (save-states/) still load, and that this version stores the fields and variants the newest set lists
cargo save-states --record    # Keep a new set of save states, after a change to the fields or variants save states store
cargo play path/to/rom.gb     # Run a ROM in the desktop app (--cgb, or --model dmg-0, dmg, mgb, sgb, sgb2, cgb-0 to cgb-e, agb)
cargo screenshot path/to/rom.gb 5   # Run 5 emulated seconds headless and save a screenshot to target/screenshots/ (--cgb and --model as for play)
cargo benchmark ~/Roms/GameBoy --record before.txt   # Record fingerprints of frames, audio and end state
cargo benchmark ~/Roms/GameBoy --compare before.txt  # Check a later build produces exactly the same
```

## Where Things Are

The root `Cargo.toml` is a virtual workspace. `default-members` covers only the core crates so `cargo build`/`cargo test` at the root don't build the GUI frontends or the xtask tooling. The folders with a `CLAUDE.md` of their own describe their code in detail; read it before changing code there.

- `crates/nubo-core/` (`CLAUDE.md`): The emulator itself, a plain Rust library with no platform dependencies. `emulator.rs` is the public API (re-exported from `lib.rs`), with `cpu/`, `address_bus/` (memory map, cartridges and their controllers, DMA, write timing, cheats), `gpu/` (the PPU), `apu/`, `joypad.rs`, `controller.rs`, `serial.rs`, `model.rs` (the emulated hardware revisions), `clock.rs`, `bios.rs` (boot ROMs, sources in `boot_roms/`), `rom_archive.rs` and `emulator/save_state.rs`
- `crates/nubo-logo/`: The logo as a few rounded rectangles, with a small anti-aliased rasterizer, an SVG export and a Windows `.ico` export; used by the Iced frontend (its window icon, and `nubo.exe`'s icon through `build.rs`) and by `cargo xtask bundle`, `flatpak` and `windows`
- `crates/serializable_derive/`: Derive macro for save-state serialization (described in `crates/nubo-core/CLAUDE.md`)
- `frontends/*`: One crate per frontend, each depending on `nubo-core`:
  - `frontends/iced/` (`CLAUDE.md`): The desktop app, built with Iced, which also builds for the browser
  - `frontends/headless/`: No-display frontend for running ROMs and capturing screenshots. It runs the given number of emulated seconds with a real-time clock that counts emulated time, so two runs give the same PNG
  - `frontends/benchmark/` (`CLAUDE.md`): Speed, and fingerprints of every frame, sample and end state that show whether a change altered any output
  - `frontends/save_state_check/` (`CLAUDE.md`): Keeps players' save states loading across updates (`cargo save-states`)
  - `frontends/json_test_runner/` (`CLAUDE.md`): The JSON CPU tests (`cargo cpu-tests`)
  - `frontends/test_rom_runner/` (`CLAUDE.md`): The test ROM suites (`cargo test-roms`)
- `xtask/` (`CLAUDE.md`): Project tasks written in Rust: web build, packaging, licenses, emulator comparison, nightly review tooling

## Style Guidelines

- **No `unsafe` code.** The workspace lints forbid it in every crate.
- **Keep clippy clean.** The workspace enables clippy's default lints plus a curated set of pedantic ones in the root `Cargo.toml`; every crate opts in with `[lints] workspace = true`. If a lint must be silenced, use `#[expect(clippy::..., reason = "...")]` on the smallest item possible.
- **Doc comments on types and functions.** Give every struct, enum, trait and non-trivial function a short `///` comment that a junior programmer can follow: say what it is for and explain Game Boy terms (such as "M-cycle" or "MBC") in plain words. Skip doc comments that only repeat the name, such as on simple getters.
- **Few inline comments.** Code should explain itself through clear names and structure; use `//` comments only for things the code can't say, such as hardware quirks or links to the reference they were ported from.
- **Name things after what they are.** Use whole words over abbreviations, treat acronyms as words in type names (`RtcState`, `Mbc1CartridgeMapper`), and don't prefix getters with `get_`. Hardware register names from Pan Docs (`lcdc`, `nr52`, `svbk`) are fine.
- **Define modules at the bottom of the file**, not the top.
- **Newest stable versions.** Rust (the `stable` channel), crates, GitHub Actions (pinned to commits, which Dependabot moves to each release), and tools such as cargo-deny and Flathub's linter all follow their newest stable release, and never a pre-release unless nothing else works. When an update breaks something, the nightly run fails, and that is where it gets fixed. The exception is what others build from Nubo's source: the Flatpak is built with exactly what Flathub would build it with (Flathub's newest freedesktop runtime, its SDK's Rust and `Cargo.lock`), and its build fails when either moves on. Test data (the test ROMs, the benchmark games and the emulators compared against) stays pinned, since results and kept save states depend on its exact content.
- **Credit ported code.** When code, tables or data are ported from another project, say so in a comment at the top of the file with the project's link and license (SameBoy-derived files point to the Expat notice in `apu.rs`), and add the project to `NOTICE` and the Credits section of the README. Only port code, and only add dependencies, under licenses compatible with GPLv3 (MIT, BSD, ISC, Zlib, Apache-2.0, MPL-2.0 and the like; not GPL-2.0-only). Gambatte is GPL-2.0-only: use it only as a reference (its test ROMs, traces of a local build, what it does), implement what you learn in Nubo's own model, and never port its code or tables.

## Rules for Every Change

- **User input never panics the core.** ROMs, zip files, save states and cheat codes from users return `LoadRomError`, `SaveStateError` or `CheatError` instead.
- **Lazy components, exact results.** The PPU and APU are left behind while nothing they do can be seen and catch up in one go, and whole stretches of plain M-cycles run at once; the result must be bit for bit what stepping every component one M-cycle at a time gives. Any new way for the CPU or another component to see or change the hardware's state must run the waiting M-cycles and catch the PPU and APU up first. Show that an optimization changes nothing with `cargo benchmark --compare` (and against the `eager` feature, which runs every M-cycle on its own). Details: `crates/nubo-core/CLAUDE.md`.
- **Timing calibration moves only with the test ROMs.** The PPU's dot positions, the post-boot state of each model and similar timings are calibrated against test ROMs and other emulators; change them only together with `cargo test-roms`.
- **Players never lose a save state to an update.** Stored names of fields and variants are permanent: a field renamed in the code keeps its stored name with `#[serializable(name = "...")]`, a field whose type or meaning changes is stored under a new name, values are fixed-size and little-endian (never `usize`). When what save states store changes, `cargo test` and `cargo save-states` fail until a new set is kept with `cargo save-states --record` in the same pull request. Details: `crates/nubo-core/CLAUDE.md` and `frontends/save_state_check/CLAUDE.md`.
- **Code the browser build shares** must take `Instant`, `SystemTime` and `Duration` from `iced::time` (std's panic on wasm32-unknown-unknown), and text that sets a font must use `theme::BODY_FONT` or `theme::HEADING_FONT`, never `Font::DEFAULT`, or it draws nothing there.
- **Merging pull requests.** When the maintainer asks Claude to merge a pull request, follow the `merge` skill: wait until CI is green and Codex's review has completed, and handle its findings first.

## Verifying a Change

- `cargo test-roms` runs every test ROM suite in about 30 seconds (pass a filter such as `gambatte/oamdma` to run part of it) and compares the results with `frontends/test_rom_runner/passing.txt`. It fails both when a listed test stops passing and when an unlisted test starts passing, so after fixing an accuracy issue run `cargo test-roms --update` and commit the updated list. Use `--verbose` to see why each test fails; failing screenshot tests write the emulator's screen and a `.diff.png` to `target/test-roms/failures/<model>/`.
- `frontends/test_rom_runner/FAILING_TESTS.md` explains every test that still fails, what was tried and what to try next; keep it up to date when a fix makes one of them pass. `docs/ACCURACY_PLAN.md` is the plan towards claiming to be the most accurate emulator (comparison with other emulators, hardware checks, missing hardware); update it when a phase moves on.
- For a ROM outside the suites, `cargo screenshot path/to/test_rom.gb 10` runs it headless and saves a screenshot to `target/screenshots/`.
- CI runs fmt, both clippy runs, `cargo doc` with warnings as errors, `cargo deny`, `cargo test --workspace`, `cargo test-roms`, `cargo cpu-tests`, `cargo save-states` and the quick web build on every pull request; the slow checks run nightly. What runs where: `docs/CI.md`.

## More Documentation

- `docs/CI.md`: CI, the nightly workflow, nightly builds and how Rust and Flathub updates are handled
- `docs/NIGHTLY_REVIEW.md`: what the nightly Claude review does
- `docs/ACCURACY_PLAN.md`: the plan towards the most accurate emulator
- `docs/EMULATOR_COMPARISON.md`: speed next to other emulators
- `docs/SHOOTOUT_PLAN.md` (adding Nubo to the GB Emulator Shootout), `docs/RETROARCH_PLAN.md` (a libretro core for RetroArch) and `docs/RESTRUCTURE_PLAN.md` (a shared `nubo-frontend` crate and a `tools/` folder, signed off, with the order of the work) are plans for work not done yet; update them as their parts get done
- `.claude/skills/merge/SKILL.md` (the `merge` skill): merging a pull request
- `.claude/skills/release/SKILL.md` (the `release` skill): making a release
