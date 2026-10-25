# Nubo

Nubo is a cycle-accurate Game Boy (DMG) and Game Boy Color (CGB) emulator in Rust, forked from RetroBoy (smparsons/retroboy). It runs through an Iced app, natively on the desktop and as WebAssembly in the browser. Nubo is GPL-3.0-or-later (`LICENSE`); the code from RetroBoy stays Apache-2.0 (`LICENSE-APACHE`), credited in `NOTICE`.

This file is for AI coding agents and people alike. Folders with their own `AGENTS.md` (listed under Where Things Are) explain how their code fits together: read it before you change code there, since not every agent loads it by itself (Claude Code does only when it opens a file in that folder with its Read tool). Facts about a single file are in that file's doc comments.

## Setup and Commands

You need the newest stable Rust, as `rust-toolchain.toml` asks (`rustup update`); the tools download the test ROMs and benchmark games themselves. Linux also needs `libasound2-dev`, `libudev-dev` and `pkg-config`. The browser build's lint needs `rustup target add wasm32-unknown-unknown`, and the license check needs `cargo install cargo-deny --locked`. The `cargo` aliases below are defined in `.cargo/config.toml`; packaging, the web build and the other project tasks are in `xtask/AGENTS.md`.

```bash
cargo build / cargo test           # The core crates only (the workspace's default members)
cargo test --workspace             # Every crate, frontends and xtask included
cargo test-roms [filter]           # The test ROM suites against tools/nubo-test-rom-runner/passing.txt (about 30 s)
cargo cpu-tests                    # Every CPU opcode but STOP, HALT, DI and EI, checked cycle by cycle
cargo save-states                  # The kept save states still load, and the stored layout is unchanged
cargo play [rom] [--cgb | --model cgb-c]   # The desktop app; options in frontends/nubo/src/command_line.rs
cargo screenshot rom.gb --seconds 10       # Headless, 5 emulated seconds by default; PNG in ./target/screenshots/
cargo benchmark <roms> --record a.txt      # Speed, and fingerprints of every frame, sample and end state (--compare a.txt)
cargo xtask install [--build main | checkout | 0.12 | <commit>]   # Build Nubo for this computer and install it
cargo xtask screenshots [--check]  # The README's and Flathub's screenshots, made again from their scenes
```

- `cargo play --model` takes `dmg-0`, `dmg`, `mgb`, `sgb`, `sgb2`, `cgb-0` to `cgb-e` and `agb`.
- Run `cargo screenshot` from the workspace root: it writes under the folder it runs from.
- `cargo xtask install` without `--build` stops to ask when the working tree isn't the newest `origin/main`. Then ask the maintainer which to build; don't pick for them.
- `cargo xtask screenshots` runs a headless GNOME Shell; the `screenshots` skill explains when and how.

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
9. If dependencies changed: `cargo deny check licenses bans sources` and `cargo xtask rust-version`, and commit `Cargo.lock`, since CI builds with `--locked`.
10. If the change must not alter output (an optimization, a refactor): compare fingerprints, as below.

CI runs steps 1 to 9 on every pull request, whatever changed; the slow checks run nightly. What runs where: `docs/CI.md`.

**Proving that output didn't change.** Record fingerprints on a build without the change (`git stash -u`, so new files go too), then compare with it:

- The benchmark games: `cargo xtask benchmark-roms` once, then `cargo benchmark target/benchmark-roms --record before.txt` and later `--compare before.txt`.
- The test ROMs: `cargo benchmark target/test-roms/v7.0 --seconds 4 --jobs <cores>` with the same flags.
- A change to the lazy components: also compare a normal build with an `eager` one (`tools/nubo-benchmark/AGENTS.md`).

## Rules

These keep Nubo exact, players' saves safe and the code free to share. Never break them; when a task seems to need it, stop and ask the maintainer.

- **User input never panics the core.** ROMs, zip files, save states and cheat codes return `LoadRomError`, `SaveStateError` or `CheatError`.
- **Lazy components, exact results.** The PPU and APU run behind the CPU while nothing they do can be seen, then catch up in one go, and plain M-cycles (one CPU step each) run in bulk. The result must be bit for bit what running every component one M-cycle at a time gives. Any new way for the CPU or another component to see or change hardware state must first run the waiting M-cycles and catch the PPU and APU up. How: `crates/nubo-core/AGENTS.md`.
- **Timing calibration moves only with the test ROMs.** The PPU's dot positions, each model's state after the boot ROM and the other timings are calibrated against test ROMs and other emulators. Change them only together with `cargo test-roms`.
- **Players never lose a save state to an update.** A field or variant's stored name is permanent: when you rename it in the code, keep the stored name with `#[serializable(name = "...")]` (hand-written implementations and the PPU store names differently). A field whose type or meaning changes gets a new stored name. Stored values are fixed-size and little-endian, never `usize`. How: `crates/nubo-core/AGENTS.md`.
- **Only GPLv3-compatible code and dependencies:** MIT, BSD, ISC, Zlib, Apache-2.0, MPL-2.0 and the like, never GPL-2.0-only.
  - **Gambatte is GPL-2.0-only:** use it as a reference only (its test ROMs, traces of a local build, its behavior), never port its code or tables.
  - **Its ideas reach Nubo only through a clean room.** One agent reads its source and writes a report of the techniques in its own words: no code, no pseudo-code that mirrors its functions, no tables, constants or identifiers. Whoever writes Nubo's code works from that report alone and never reads Gambatte's source. The reports are in `docs/speed-research/`.
  - **Credit what you port.** Code, tables or data from another project get its link and license at the top of the file, in `NOTICE` and in the README's Credits; SameBoy-derived files point to the Expat notice in `apu.rs`. Projects whose ideas reach Nubo through a clean-room report, or whose behavior Nubo's timing is measured against, are named in the README's Credits too.
- **Code the browser build shares works in the browser.**
  - It takes `Instant`, `SystemTime` and `Duration` from `nubo_frontend::time`, which re-exports the `web-time` crate, since std's panic on wasm32. The UI code may use `iced::time`, the same types.
  - Text that sets a font uses `theme::BODY_FONT` or `theme::HEADING_FONT`, never `Font::DEFAULT` or a font of its own: those draw nothing in the browser. Plain `text()` without `.font()` is fine, and so are the icons (`ui::icons`).
- **No `unsafe`.** The workspace lints forbid it. The one exception is the RetroArch core's exported C functions: they live in `frontends/nubo-libretro/src/ffi.rs` (and its tests), under one `#[expect(unsafe_code)]`, with that crate's own lint table (`unsafe_code = "deny"`) and the `unsafe` in as few places as possible. A later crate that binds Nubo to another language follows the same rule; every other crate keeps it forbidden.
- **Newest stable versions** of Rust, crates, GitHub Actions and tools, never a pre-release unless nothing else works; test data stays pinned. Two exceptions: the Flatpak builds with the freedesktop SDK's Rust (`docs/CI.md`), and the fuzz targets in `fuzz/` use nightly Rust, which `cargo-fuzz` needs. That folder is outside the Cargo workspace, nothing that ships or that CI builds depends on it, and only a full review runs it.

## Style

- **Doc comments on every struct, enum, trait and non-trivial function**: a short `///` comment a junior programmer can follow, saying what it is for and explaining Game Boy terms (such as "M-cycle" or "MBC") in plain words. Skip comments that only repeat the name, such as on simple getters.
- **Few inline comments.** Let names and structure explain the code; use `//` only for what the code can't say, such as hardware quirks or the reference something was ported from.
- **Whole words, not abbreviations.** Acronyms are words in type names (`RtcState`, `Mbc1CartridgeMapper`), getters have no `get_` prefix, and Pan Docs register names (`lcdc`, `nr52`, `svbk`) are fine.
- **Modules are declared at the bottom of the file.**
- **Docs move with the code.** Facts about a file live in its doc comments, and how files work together in the folder's `AGENTS.md`; update whatever your change makes wrong, in the same commit.
- **Clippy stays clean.** The workspace enables the default lints and a curated set of pedantic ones (root `Cargo.toml`); every crate opts in with `[lints] workspace = true`, except the RetroArch core, which copies the table to deny `unsafe` instead of forbidding it (keep its copy the same). To silence a lint, put `#[expect(clippy::..., reason = "...")]` on the smallest item possible, never `#[allow]`.
- **Plain US English** in docs, comments, commits and pull requests: short sentences and everyday words, as in this file.

## Pull Requests

- **Title:** the area, a colon and what changes, in plain words: `Core: give cartridge types without RAM no RAM buffer`, `App: save where you left off as you play`. Areas in use include `Core`, `APU`, `Frontend`, `Library`, `App`, `Browser build`, `RetroArch core`, `Test ROM runner`, `Benchmark`, `xtask`, `CI`, `Nightly`, `Docs` and `Issue triage`. Pull requests are squash-merged, so the title becomes the commit on `main`.
- **Fixes an open issue:** say `Fixes #<number>` in the description, so GitHub closes the issue when it merges.
- **See it through.** After opening a pull request, watch it without being asked (in Claude Code on the web, subscribe to its activity) and fix what comes up: red CI, a conflict with `main` and reviewers' comments, until CI is green. No automatic review runs on this repository, so there is none to wait for.
- **Merging waits for the maintainer's go**, then follows the `merge` skill.
- **Say what a number is.** When you name a pull request or an issue by its number in a chat message to the maintainer, or in a report such as the nightly review's pull request and comments or a full review's report, put "PR" or "issue" before the number and one or two words after it saying what it is: "PR #194 (triage setup)", "issue #156 (MBC3 clock)". Elsewhere on GitHub a bare number is fine, since GitHub shows the title.

## Sessions

Most work runs in Claude Code sessions in the cloud, each on a fresh clone of this repository: what isn't pushed doesn't exist for the next session. In a session with the maintainer (the nightly review and the issue-triage routine follow their own docs):

- **One task per session.** When the maintainer starts an unrelated topic, suggest a new session for it; they decide.
- **Speak up only when it matters.** Every message reaches the maintainer as a notification: write when something is merged, when you are stuck or when you need a decision, not about routine steps such as a wait on CI.
- **End every turn with a status line**, the last line of the message, starting with `**Status:**`: the work and where it stands, background tasks or subagents still running (or none), and whether the working tree is clean. Example: `**Status:** PR #194 (triage setup) open, CI green, waiting on the go to merge · no background tasks · tree clean`.
- **Keep late results apart.** When a background task or subagent reports back after the conversation has moved on, say so in a message of its own: which task it belongs to, what came out, and whether anything now needs a commit or a merge.

## Skills and Agent Files

Some tasks have steps of their own, in `.claude/skills/`. Follow the skill whenever its task comes up:

- `merge`: merging a pull request.
- `release`: making a release.
- `screenshots`: regenerating or collecting screenshots.
- `issue-triage`: working on the issue-triage page, or setting its page or routine up again.
- `full-review`: a full review of the repository, only when the maintainer asks for one, never on your own.

Claude Code and Codex both read these `AGENTS.md` files; keep them the only instruction files. Claude Code stops reading `AGENTS.md` as soon as a `CLAUDE.md` or `CLAUDE.local.md` exists in its working directory or above, so don't add one. Codex finds the skills through pointers in `.agents/skills/`: change a skill's steps only in `.claude/skills/`, and keep each skill's `name` and `description` the same in both places.

## Where Things Are

The root `Cargo.toml` is a virtual workspace whose default members are `nubo-core` and `nubo-serializable-derive`, so `cargo build` and `cargo test` don't build the apps, the tools or xtask. Libraries are in `crates/`, apps players use in `frontends/`, developer programs in `tools/`. Every package's name starts with `nubo` (except `xtask`), and every crate's folder is named after its package (except `fuzz/`, the name `cargo-fuzz` expects).

- `crates/nubo-core/` (`AGENTS.md`): the emulator, a plain library with no platform dependencies. `Emulator` in `emulator.rs` is its API.
- `crates/nubo-frontend/` (`AGENTS.md`): everything an app needs besides drawing, without a UI toolkit: settings, saves, the game library, running a game with its sound, game controllers, and what the RetroArch core shares with the app.
- `crates/nubo-serializable-derive/`: the derive macro behind save states.
- `crates/nubo-logo/`: the logo, drawn in code, for the app, its icons and installers, and the Super Game Boy screens in `nubo-core`.
- `crates/nubo-download/`: downloads test data and checks its checksums, for the tools and xtask.
- `frontends/nubo/` (`AGENTS.md`): the app, desktop and browser.
- `frontends/nubo-libretro/` (`AGENTS.md`): the RetroArch (libretro) core, a dynamic library; `cargo xtask libretro` packages it.
- `tools/nubo-test-rom-runner/` (`AGENTS.md`): `cargo test-roms`.
- `tools/nubo-json-test-runner/`: `cargo cpu-tests`.
- `tools/nubo-save-state-check/` (`AGENTS.md`): `cargo save-states`; the kept sets are in the repository's top-level `save-states/`.
- `tools/nubo-benchmark/` (`AGENTS.md`): `cargo benchmark`.
- `tools/nubo-headless/`: `cargo screenshot`.
- `xtask/` (`AGENTS.md`): the web build, packaging, licenses, the emulator comparison, the nightly review's tools and the screenshots.
- `.github/shootout/` (`AGENTS.md`): Nubo's class in the GB Emulator Shootout's harness, kept the same as the Shootout's copy.
- `fuzz/` (`AGENTS.md`): the fuzz targets of the full review, outside the Cargo workspace, on the nightly toolchain.
- `images/`: the logo, the README's buttons (`images/buttons/`) and the screenshots (the Flathub one in `images/flatpak/`, all listed in `images/screenshots.toml`), with the scenes they are made from in `images/scenes/`.

## More Documentation

For people:

- `README.md`: for players first. `BUILDING.md`: building, running and packaging Nubo, and where the code is. Keep both true when commands, packages or folders change.
- `docs/RETROARCH.md`: installing and using the RetroArch core; the README keeps the short version.

How things stand:

- `docs/ACCURACY.md`: the test ROM results suite by suite, and the tests that aren't run; the README keeps the summary.
- `tools/nubo-test-rom-runner/FAILING_TESTS.md`: every test that still fails, what was tried and what to try next.
- `docs/EMULATOR_COMPARISON.md`: speed next to other emulators.
- `docs/CI.md`: CI, the nightly run, nightly builds, toolchain updates.
- `docs/NIGHTLY_REVIEW.md`: what the nightly review does.
- `docs/ISSUE_TRIAGE.md`: the issue-triage page, kept up to date every hour, where the maintainer picks the open issues to solve.
- `docs/reviews/`: the report of each full review, with a list of them in its `README.md`; each report is a record of its day, not kept up to date.

Plans (update each one as its work moves on):

- `docs/ACCURACY_PLAN.md`: towards the most accurate emulator.
- `docs/SPEED_PLAN.md`: making Nubo faster without changing any result; `docs/speed-research/` holds the clean-room reports on other emulators it draws on.
- `docs/SCREENSHOTS_PLAN.md`: making every screenshot in the README and the Flathub listing again from recorded scenes.
- `docs/RETROARCH_PLAN.md`: the RetroArch core, what was built and what is left (getting it into RetroArch's core downloader).
- `docs/SHOOTOUT_PLAN.md`: done but for Nubo appearing in the Shootout's published table.
- `docs/RESTRUCTURE_PLAN.md`: done; why the layout (the shared `nubo-frontend` crate and the `tools/` folder) is what it is.
