# Code review of October 2026

A review of the whole repository on 2026-10-16, at `main` 6db22a5 and then up to bae9e11
(after the nightly review #141 and #143): bugs, the rules in `AGENTS.md`, idiomatic Rust, the
code style, doc comments and the docs. What was obviously wrong and small was fixed, one commit per
fix; everything else is listed below and, where it needs a decision, filed as an issue labelled
`code-review`. It also takes in the findings of the nightly review of the same night (#137 to
#140 and the suggestions in #141).

## How it was done

- **Ten area reviews**, one per part of the code: the CPU and emulator API, the address bus
  (cartridges, DMA, cheats), the PPU, the APU and Super Game Boy, `nubo-frontend`, the app, the
  RetroArch core, the tools and small crates, xtask with the workflows, and the docs. Each read every
  file of its area in full and wrote down what it found with the path from a real input to the
  failure. They found 163 things: 53 bugs, 7 breaks of a rule in `AGENTS.md`, 26 idioms, 13 style
  points and 64 places where the docs were wrong.
- **Every finding was checked again** in the code before it was fixed or reported. None was wrong
  outright; one was half wrong (#159, whose RetroArch part didn't hold), and five that `main` had
  fixed or made moot meanwhile were skipped.
- **The fixes** went onto five branches by theme, each a pull request of its own, one commit per
  finding with a message saying why, and each branch passing what CI runs.
- **A second reviewer**, a separate Claude Code session that could not push, reviewed every
  commit in ten rounds and checked every finding that is only reported. It caught one regression (a
  fix for #139 that stopped Down in a partly filled row, fixed before any pull request), a
  half-done cleanup and four inaccurate doc sentences, all fixed. It also stress-tested the new
  save-state checks: 17.2 million states saved and loaded at random points of every test ROM and the
  benchmark games, with not one rejected.
- **Nothing changes what the emulator does** except where a fix says so: on every branch the
  test ROMs pass exactly as `passing.txt` says (6793 of 6824), the CPU tests pass, the kept save
  states load with the same layout, and every frame, sample and end state of the benchmark games
  and the test ROMs has the same fingerprint as `main`.

## The pull requests

Merge them in this order; each later one may need `main` merged in first (see Merging).

| Pull request | Branch | Commits | What |
| --- | --- | --- | --- |
| #163 Docs | `claude/nifty-albattani-79g95v` | 66 | Docs and doc comments that were wrong, stale or missing; this report |
| #164 Style | `claude/nifty-albattani-79g95v-style` | 18 | Missing doc comments, abbreviations, module placement, visibility |
| #165 Idioms | `claude/nifty-albattani-79g95v-idioms` | 21 | Small idiomatic cleanups and dead code, none changing behavior |
| #166 Bug fixes | `claude/nifty-albattani-79g95v-fixes` | 32 | Small bug fixes, each with a test where one is cheap; fixes #137, #138, #139, #140 |
| #167 Tooling | `claude/nifty-albattani-79g95v-tooling` | 16 | Bugs in the tools, xtask and the workflows |

### The bugs fixed

The most important first:

1. **MBC2 battery saves were never loaded.** `nubo-frontend` rounded a save's length down to 2 KiB
   to drop other emulators' clock footers, which turned MBC2's 512 bytes into nothing: games like
   Final Fantasy Adventure started with blank RAM and then wrote over the player's save. The
   rounding is now to 512 bytes.
2. **Damaged save states could crash the core**, against the first rule. Loading now rejects an
   OAM DMA past the end of OAM (a panic in release builds too), a PPU fetching an object when none
   is left (also in release builds), a PPU still drawing a finished line, an LYC line outside the
   frame, a serial transfer with more than 8 bits shifted, and a speed switch the console can't be
   in. Each has a test that crashed before. The corruption test of the PPU's state now corrupts all
   of it, not only its first 400 and last 200 bytes, and the random PPU scenes run on CGB-C too.
3. **Nightly issues #137 to #140**: changing a setting dropped the frame blending, Down in the
   library jumped to the wrong card, and the settings' focus outline could disappear. #137, the app
   growing about 10% in two nights, came mostly from Iced's `image` feature, which turns on every
   codec of the `image` crate, an AVIF encoder (`ravif` and `rav1e`, about 2.4 MiB of code) among
   them, although every picture the app shows is pixels `nubo-frontend` already decoded. With
   `image-without-codecs` the desktop app goes from 44.4 MB to 35.8 MB (-19%) and the browser's
   wasm from 12.1 MB to 10.4 MB (-13%), below where they were before the growth. (`opener`, which
   the nightly review suspected, brings `url` and ICU's tables on Linux and Windows whatever its
   features, but `cargo bloat` puts them far below the codecs.)
4. **The RetroArch core made a `&mut [u8]` over uninitialized memory** in `retro_serialize`
   (undefined behavior, if harmless today), and called the rumble callback with a lock held that
   the code's own rule says must not be.
5. **Smaller ones:** a Super Game Boy border color of exactly 0x8000 drew as transparent; RTC
   values from a `.rtc` file weren't cut to the registers' widths; cheat codes with a leading `+` or
   misplaced dashes were accepted; the header's ROM bank count was wrong for codes 0x09 and up; in
   the browser, fast forward or rewind made the picture skip a frame; a frame could go unannounced
   when the app starts listening; a written file wasn't on the disk before the rename that replaces
   the old one; a dialog whose game left the library stayed open but invisible; an absurd last
   played time, window size or browser file time could panic the app; the logo could panic on odd
   shapes.
6. **Tools and CI:** the save-state check sorted its sets as text (`-10` before `-2`); the
   benchmark accepted 0, negative or infinite `--seconds`; a test with nothing to compare passed;
   downloads wrote straight to their final path with no timeout; the nightly review's state file
   took any text as a commit and passed it to `git diff`; the Windows installer, the macOS app and
   the emulator comparison built without `--locked`; the Flatpak workflow didn't run when
   `rust-version` changed; the Shootout job kept its token while running third-party code; and the
   nightly review's three suggestions for scenes and screenshots.

## Found and not fixed

Each needs a decision or a change bigger than this review was allowed to make.

### Issues filed

| Issue | Where | What |
| --- | --- | --- |
| #144 | nubo-frontend | An unreadable `settings.toml`, cheats or library file is overwritten on the next save |
| #145 | Licensing | Where the Gambatte test runner's digit glyphs come from isn't written down |
| #146 | CI | `WIKI_TOKEN` is visible to every `cargo xtask` run in the nightly's wiki step |
| #147 | CI | The nightly's `timezone` key has no effect, so it runs at 01:17 UTC |
| #148 | CI | The nightly metrics leave out the Flatpak, Windows, RetroArch and Shootout jobs |
| #149 | CI | A `last_commit` no longer in main's history breaks the wiki update every night |
| #150 | App | Holding a Ctrl key bound to a Game Boy button blocks every other game key |
| #151 | App | `--model` and `--cgb` are ignored when the game has a Resume state |
| #152 | App | Starting a game by drop or Ctrl+O leaves menus open over it and a key capture running |
| #153 | App | Loading a state on another model loses the running game if the new session fails |
| #154 | App | The fullscreen and fast forward tooltips name F11 and Tab whatever is bound |
| #155 | nubo-frontend | With frame blending, the Green and Pocket palettes leave gray pixels |
| #156 | Core | The MBC3 clock loses time rolling over from an invalid value, and stands still when the wall clock goes back |
| #157 | Core | Save states don't cross-check the copies components keep of the same value |
| #158 | Core | Zip archives load their first `.gb` entry, even a macOS `__MACOSX/._` resource fork |
| #159 | Core | Cartridges without RAM still get a RAM buffer sized from the header |
| #160 | RetroArch | The `CORE` lock is held across callbacks into RetroArch (latent, no known trigger) |
| #161 | Benchmark | Frame fingerprints don't capture which audio buffer a frame arrives in |
| #162 | App | The efficiency measurement reuses battery saves between its runs |

The most important are #144 (players can lose their settings, cheats or library), #145 (a
licensing question only the maintainer can answer) and #146 (a token that can write to the
repository is exposed to code from dependencies).

### Smaller notes, without an issue

- **Pointers handed to RetroArch** (`ffi.rs`) come from short-lived `&mut` borrows, which Rust's
  aliasing rules formally invalidate when the buffer is borrowed again. Every Rust libretro core
  does this, and nothing can miscompile it today, but the `SAFETY` comments only argue that the
  buffers don't move.
- **Copies between the tools**: `scripted_keys`, `ALL_KEYS`, `hold_keys` and `NO_SAVES` are
  copied between the benchmark, the save-state check and the runners, and `panic_message` and
  `work_dir` between the two test runners. Moving them into a shared crate would stop them drifting;
  the save-state check's key script must stay exactly as it is.
- **`dialog_content` reads the disk in `view`**: the game menu checks for save states on every
  redraw while it is open.
- **`--record-scenes --speed`** shows the fast forward badge in a scene's preview that its capture
  won't show.
- **Cheat codes kept with misplaced dashes** (such as `CE-D56A`) no longer apply, now that the
  parser checks the dashes. Nubo itself always stores them in the right place.
- **A finite but absurd window size** (such as 1e30) in `settings.toml` still reaches the window;
  only non-finite ones are refused.
- **`fill_rest_of_desynced_line`** (PPU) never runs for a state Nubo writes, but a damaged state
  could reach it, so it stays as a guard and its doc says so.
- **`scroll_pause_menu` and `scroll_in_proportion`** differ only for a single row; merging them
  needs a parameter.
- **The commit "Give the ROM bank count of the 1.1 to 1.5 MiB header codes"** says codes that
  name no size came out as 65535; in fact codes 0x09 to 0x0E gave 1024 to 32768 and only 0x0F and
  up saturated. The code is right.
- **Box art that isn't really a PNG**: with the image codecs gone (#137), `nubo-frontend` decodes
  only PNG. libretro's thumbnails are PNG, but one that is a JPEG under a `.png` name now shows no
  box art instead of being decoded. Adding the `jpeg` feature to `nubo-frontend`'s `image` would
  cover it for about 100 KiB.
- **`generate_blep_table`** (nightly suggestion 6) is left alone: the table must stay bit for bit.
- **A RetroArch core test depends on the machine's speed**:
  `a_clock_filled_in_before_the_first_frame_is_what_the_game_reads` runs the boot on the wall clock
  and allows at most 4 real seconds for it. It fails on a slow build (it did under the coverage
  build, with 36 seconds read); a slow CI machine would fail it too. Measuring the real time it
  took and allowing that would make it steady.

## Tools and other checks

- **Spell checker** (`typos`): one real typo (`curent` in two test names, fixed); the rest are
  `.sav`, stored names that must keep their spelling, and Game Boy model codes.
- **Unused dependencies** (`cargo machete`): none. **Advisories** (`cargo deny check
  advisories`): none.
- **Every pedantic and nursery Clippy lint** on top of the curated set: 2,447 warnings, all taste
  (`use Self` 648 times, backticks in docs 461, `const fn` 344, `#[must_use]` 135, numeric casts
  about 300, `# Errors` sections 75). The few that can point at bugs were checked: the "infinite
  iterations" are searches for a free file name that always end, and the "formatting arguments" are
  template placeholders. Nothing to fix; the curated set is a good choice.
- **Fuzzing**: ROMs and zip files, save states, cheat codes and battery saves (any RAM length and any
  clock state, on every cartridge type) each ran for 15 minutes, and save states and ROMs for an
  hour more, about 300,000 runs in all, with no crash. The fuzzers didn't find the save-state crashes
  the reviewers found: each run plays whole frames, so they manage only 30 to 70 runs a second, and
  those crashes need two values changed together. Fuzzing is worth more with a faster target (one
  M-cycle instead of frames) and structure-aware mutations of save states; the code isn't in the
  repository.
- **Test coverage** of the unit tests alone (`cargo llvm-cov --workspace`, without the test ROM
  suites, test files left out): `nubo-core` 90% of its lines, the RetroArch core 94%, `nubo-logo`
  94%, `nubo-frontend` 74%, the save-state check 73%, the app 34%, xtask 33%, the test ROM runner
  25% (its suites run the rest), the benchmark 4%. The app is the gap that matters: most of the
  app bugs above live in `App::update` and the views' navigation, where a test drives a message
  and checks the state, as `app/tests.rs` already does in places.

## The nightly checks

The nightly workflow ran by hand on a branch that merges all five pull requests
(`claude/nifty-albattani-79g95v-all`, before three later commits: two doc sentences and the
codec change) and, at the same time, on `main`, so both ran on the same day's
machines ([the branch's run](https://github.com/mschoenm/nubo/actions/runs/38854124936),
[main's run](https://github.com/mschoenm/nubo/actions/runs/38854139055)). Every job passed on
both: the tests on macOS and Windows, the test ROMs with real boots, the save states of every test
ROM, the lazy components against every M-cycle on all test ROMs, 10,000 random PPU scenes, the
advisories, both Flatpaks, the Windows installer, the four RetroArch cores and the GB Emulator
Shootout's own harness on Windows.

| Metric | `main` | All five | Change |
| --- | --- | --- | --- |
| Test ROMs | 6793 of 6824 | 6793 of 6824 | none |
| GB Emulator Shootout | 261 of 264, rank 1 | 261 of 264, rank 1 | none |
| Instructions per emulated second | 78.6 M to 173.7 M | 78.5 M to 173.5 M | 0.10% to 0.22% fewer on every game |
| Efficiency (wake-ups and frames a second, playing, paused, fast forward, still screen) | | | identical |
| Desktop app / browser wasm | 44.4 MB / 12.05 MB | 44.4 MB / 12.05 MB | none (the codec change came after: 35.8 MB / 10.4 MB) |

The speed in time read 22% to 26% lower on the branch's run, on every game alike, while its
instruction counts went slightly down: the two runs got different shared machines, which the
nightly's own docs warn about (that is why it counts instructions). Run one after the other on
one machine, the two builds are within the noise of each other (two rounds of 30 seconds and three
of 60 on the games where they differed most).

## Merging

Pull requests that change the same file conflict once the first is merged. Two such places are
known, and were resolved the same way on the branch that merges all five:
`xtask/src/compare_emulators.rs` (Style documents `has_tool`, Idioms removes it: keep it removed)
and `crates/nubo-core/src/address_bus/mbc3.rs` (Style renames `elapsed_ms`, Bug fixes adds
`within_register_widths` above it: keep both). The pull requests are kept mergeable as the others
land.

## Recommendations

1. Decide #144, #145 and #146 first.
2. Give the RetroArch core's FFI a Miri run in CI for the parts that don't call into C, now that
   it has a `# Safety` story worth checking.
3. Write a structure-aware save-state fuzzer: save states are the one input where a damaged value
   still reaches the emulator, and every crash found here was there.
4. Before tightening any timing, look at #157: the copies of the CPU speed and the system counter
   are the one place where a loaded state can make the lazy and the eager runs differ.
