# Nubo

Nubo is a cycle-accurate Game Boy (DMG) and Game Boy Color (CGB) emulator in Rust, forked from RetroBoy (smparsons/retroboy). It runs through an Iced app, natively on the desktop and as WebAssembly in the browser. Nubo is GPL-3.0-or-later (`LICENSE`); the code from RetroBoy stays Apache-2.0 (`LICENSE-APACHE`), credited in `NOTICE`.

Folders with their own `AGENTS.md` (listed under Where Things Are) explain their code; read it before you change code there, since not every agent loads it by itself. Facts about a single file are in that file's doc comments.

## Setup and Commands

You need the newest stable Rust, as `rust-toolchain.toml` asks (`rustup update`), and `curl`, which downloads the test ROMs and benchmark games. Linux also needs `libasound2-dev`, `libudev-dev` and `pkg-config`. The browser build's lint needs `rustup target add wasm32-unknown-unknown`, and the license check needs `cargo install cargo-deny --locked`. The `cargo` aliases below are defined in `.cargo/config.toml`; packaging, the web build and the other project tasks are in `xtask/AGENTS.md`.

```bash
cargo build / cargo test      # The core crates only (the workspace's default members)
cargo test --workspace        # Every crate, frontends and xtask included
cargo test-roms [filter]      # The test ROM suites, compared with tools/nubo-test-rom-runner/passing.txt (about 30 s)
cargo cpu-tests               # Every CPU opcode, checked cycle by cycle
cargo save-states             # The kept save states still load, and the stored layout is unchanged
cargo play [rom] [--cgb | --model cgb-c]   # Run the desktop app (models: dmg-0, dmg, mgb, sgb, sgb2, cgb-0 to cgb-e, agb; more options in frontends/nubo/src/command_line.rs)
cargo screenshot rom.gb 5     # Run 5 emulated seconds headless, save a PNG to target/screenshots/
cargo benchmark <roms> --record a.txt / --compare a.txt   # Speed, and fingerprints of every frame, sample and end state
```

## Before You Push

Run what CI runs, so a pull request is green the first time:

1. `cargo fmt --all`
2. `cargo clippy --workspace --all-targets -- -D warnings` and `cargo clippy -p nubo --target wasm32-unknown-unknown -- -D warnings`
3. `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items` (broken links in private docs fail CI too)
4. `cargo test --workspace`
5. If the Iced app changed: `cargo xtask build-web --quick`, as CI does; `cargo xtask web` serves it to look at in a browser.
6. If emulation may have changed: `cargo test-roms`. When results change on purpose, run `cargo test-roms --update` and commit `passing.txt`; when a test in `tools/nubo-test-rom-runner/FAILING_TESTS.md` starts passing, update that file too.
7. If the CPU changed: `cargo cpu-tests`.
8. If what save states store changed: `cargo save-states` (it needs the benchmark games: `cargo xtask benchmark-roms` once), and keep a new set with `cargo save-states --record` in the same pull request.
9. If dependencies changed: `cargo deny check licenses bans sources`.
10. If the change must not alter output (an optimization, a refactor): record fingerprints on a build without it (`git stash -u`, so new files go too), then compare with it. Do this on the benchmark games (`cargo xtask benchmark-roms`, then `cargo benchmark target/benchmark-roms --record before.txt` and later `--compare before.txt`) and on the test ROMs (`cargo benchmark target/test-roms/v7.0 --seconds 4 --jobs <cores>` with the same flags). For a change to the lazy components, also compare a normal build with an `eager` one (`tools/nubo-benchmark/AGENTS.md`).

The slow checks run nightly. What runs where: `docs/CI.md`.

## Rules

These keep Nubo exact, and players' saves safe. Never break them.

- **User input never panics the core.** ROMs, zip files, save states and cheat codes return `LoadRomError`, `SaveStateError` or `CheatError`.
- **Lazy components, exact results.** The PPU and APU run behind the CPU while nothing they do can be seen, then catch up in one go, and plain M-cycles (one CPU step each) run in bulk. The result must be bit for bit what running every component one M-cycle at a time gives. Any new way for the CPU or another component to see or change hardware state must first run the waiting M-cycles and catch the PPU and APU up. How: `crates/nubo-core/AGENTS.md`.
- **Timing calibration moves only with the test ROMs.** The PPU's dot positions, each model's state after the boot ROM and the other timings are calibrated against test ROMs and other emulators. Change them only together with `cargo test-roms`.
- **Players never lose a save state to an update.** A field or variant's stored name is permanent: when you rename it in the code, keep the stored name with `#[serializable(name = "...")]` (hand-written implementations and the PPU store names differently; see `crates/nubo-core/AGENTS.md`). A field whose type or meaning changes gets a new stored name. Stored values are fixed-size and little-endian, never `usize`. How: `crates/nubo-core/AGENTS.md`.
- **Only GPLv3-compatible code and dependencies** (MIT, BSD, ISC, Zlib, Apache-2.0, MPL-2.0 and the like, never GPL-2.0-only). Gambatte is GPL-2.0-only: use it as a reference only (its test ROMs, traces of a local build, its behavior) and never port its code or tables. When you port code, tables or data, credit the project with its link and license at the top of the file, in `NOTICE` and in the README's Credits; SameBoy-derived files point to the Expat notice in `apu.rs`.
- **Code the browser build shares** takes `Instant`, `SystemTime` and `Duration` from `nubo_frontend::time`, which re-exports the `web-time` crate (std's panic on wasm32; the UI code may use `iced::time`, the same types), and text that sets a font uses `theme::BODY_FONT` or `theme::HEADING_FONT`, never `Font::DEFAULT` or another font of its own, which draw nothing in the browser. Plain `text()` without `.font()` is fine, and so are the icons (`ui::icons`).
- **No `unsafe`.** The workspace lints forbid it. The one planned exception is the RetroArch core's exported C functions (`docs/RETROARCH_PLAN.md`): they will live in `frontends/nubo-libretro/src/ffi.rs`, with that crate's own lint table and the `unsafe` in as few places as possible. Any later crate that binds Nubo to another language follows the same rule; every other crate, `nubo-core`, `nubo-frontend` and the apps included, keeps it forbidden.
- **Newest stable versions** of Rust, crates, GitHub Actions and tools, never a pre-release unless nothing else works; test data stays pinned. Details, and the Flatpak's exception: `docs/CI.md`.
- **Merging a pull request:** follow the `merge` skill (CI green, Codex's review done and handled). **Making a release:** follow the `release` skill.

## Style

- **Doc comments on every struct, enum, trait and non-trivial function**: a short `///` comment a junior programmer can follow, saying what it is for and explaining Game Boy terms (such as "M-cycle" or "MBC") in plain words. Skip comments that only repeat the name, such as on simple getters.
- **Few inline comments.** Let names and structure explain the code; use `//` only for what the code can't say, such as hardware quirks or the reference something was ported from.
- **Whole words, not abbreviations.** Acronyms are words in type names (`RtcState`, `Mbc1CartridgeMapper`), getters have no `get_` prefix, and Pan Docs register names (`lcdc`, `nr52`, `svbk`) are fine.
- **Modules are declared at the bottom of the file.**
- **Docs move with the code.** Facts about a file live in its doc comments, and how files work together in the folder's `AGENTS.md`; update whatever your change makes wrong, in the same commit.
- **Clippy stays clean.** The workspace enables the default lints and a curated set of pedantic ones (root `Cargo.toml`); every crate opts in with `[lints] workspace = true`. To silence a lint, put `#[expect(clippy::..., reason = "...")]` on the smallest item possible.

## Where Things Are

The root `Cargo.toml` is a virtual workspace whose default members are `nubo-core` and `nubo-serializable-derive`, so `cargo build` and `cargo test` don't build the apps, the tools or xtask. Libraries are in `crates/`, apps players use in `frontends/`, developer programs in `tools/`. Every package's name starts with `nubo` (except `xtask`), and every crate's folder is named after its package.

- `crates/nubo-core/` (`AGENTS.md`): the emulator, a plain library with no platform dependencies. `Emulator` in `emulator.rs` is its API.
- `crates/nubo-frontend/` (`AGENTS.md`): everything an app needs that isn't drawing, without a UI toolkit: what it keeps between runs (settings, controls, battery saves, save states, cheats, screenshots), the game library, running a game with its sound (`session`, paced by the sound card on the desktop and by the screen's refreshes in the browser), game controllers (`gamepads`), and the palettes and model rule the RetroArch core shares. Its features `desktop`, `web` and `gamepads` bring each platform's storage, sound and controllers.
- `crates/nubo-serializable-derive/`: the derive macro behind save states.
- `crates/nubo-logo/`: the logo, drawn from rounded rectangles, for the app's icons.
- `frontends/nubo/` (`AGENTS.md`): the app, desktop and browser.
- `tools/nubo-test-rom-runner/` (`AGENTS.md`): `cargo test-roms`.
- `tools/nubo-json-test-runner/`: `cargo cpu-tests`.
- `tools/nubo-save-state-check/` (`AGENTS.md`): `cargo save-states`, and the kept sets in `save-states/`.
- `tools/nubo-benchmark/` (`AGENTS.md`): `cargo benchmark`.
- `tools/nubo-headless/`: `cargo screenshot`.
- `xtask/` (`AGENTS.md`): the web build, packaging, licenses, the emulator comparison and the nightly review's tools.

## Agent Instructions

Claude Code and Codex both read these `AGENTS.md` files; keep them the only instruction files. Claude Code ignores `AGENTS.md` in a folder that also has a `CLAUDE.md`, so don't add one. The `merge` and `release` skills are written in `.claude/skills/`, where Claude Code reads them; `.agents/skills/` holds pointers to them for Codex, so change the steps only in `.claude/skills/`. Each skill's `name` and `description` appear in both places; keep them the same.

## More Documentation

- `tools/nubo-test-rom-runner/FAILING_TESTS.md`: every test that still fails, what was tried and what to try next.
- `docs/ACCURACY_PLAN.md`: the plan towards the most accurate emulator; update it when a phase moves on.
- `docs/SHOOTOUT_PLAN.md`, `docs/RETROARCH_PLAN.md` and `docs/RESTRUCTURE_PLAN.md` (a shared `nubo-frontend` crate and a `tools/` folder, signed off, with the order of the work): plans for work not done yet; update them as their parts get done.
- `docs/CI.md`: CI, the nightly run, nightly builds, toolchain updates.
- `docs/NIGHTLY_REVIEW.md`: what the nightly review does.
- `docs/EMULATOR_COMPARISON.md`: speed next to other emulators.
