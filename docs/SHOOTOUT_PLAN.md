# Plan: adding Nubo to the GB Emulator Shootout

The [GB Emulator Shootout](https://gbdev.io/GBEmulatorShootout/) runs about 265 test ROMs on 18 Game Boy
emulators and publishes a table of their results. Getting Nubo into it is step 2 of Phase 6 in
[ACCURACY_PLAN.md](ACCURACY_PLAN.md), and running its harness locally on all emulators also covers much
of Phase 1 (measuring other emulators on the same tests).

The work has five parts:

- Parts 1 and 2 can be done in a cloud session.
- Part 3 needs a Windows PC and is done by the maintainer. **Done** with 0.6.0: all 261 tests
  the harness judges pass (see "What Part 3 found"). Every nightly run, and so every release, now
  runs the harness again on a GitHub Windows machine (see "Part 3 in CI").
- Part 4 is a pull request to another project's repository. It waited until Nubo would land at the top
  of the published table (Part 5 shows where it stands), until the maintainer had reviewed Nubo
  themselves and the app (its UI above all) was ready to go public, and then for the maintainer's go.
  **Open** since 2026-10-15 as
  [gbdev/GBEmulatorShootout#102](https://github.com/gbdev/GBEmulatorShootout/pull/102), tested with
  0.9.2; it is done once Nubo appears in the published table (see Part 4).
- Part 5 adds the Shootout's tests to `cargo test-roms`, so CI and the nightly run check them, and the
  nightly metrics on the wiki show where Nubo would rank. It doesn't depend on the other parts and goes
  first, right after steps 1 and 2 of [RESTRUCTURE_PLAN.md](RESTRUCTURE_PLAN.md). **Done**: Nubo passes
  261 of the 264 published tests, which would rank it first, ahead of DocBoy's 255: 258 since it draws
  evenly spaced grays (see "What Part 5 found"), and the three Super Game Boy tests since it emulates
  the Super Game Boy (`crates/nubo-core/src/sgb.rs`). The other three only report information. Parts 1 to 3 followed
  once the restructuring was finished, and are done: Part 1's options shipped in 0.6.0, and Part 3's run
  on Windows confirmed the 261 with that release. That plan's "Order and dependencies" has the whole order the
  maintainer agreed to.

## Where the code lives

- **In this repository, in the Iced app (`frontends/nubo`).** The Shootout downloads the Windows zip of
  Nubo's latest release and takes screenshots of the real `nubo.exe`, so the options it needs
  (`--scale`, `--fresh`, `--speed`, Part 1) must be in the app Nubo ships, not in a separate program.
  - The options are parsed in `command_line.rs`.
  - The screen-only window is in the window settings and `views/game.rs`.
  - The steps of [RESTRUCTURE_PLAN.md](RESTRUCTURE_PLAN.md) leave the options in the Iced app. What
    `--fresh` turns off (storage, battery saves) moves to `nubo-frontend` in its step 4. Do Part 1
    before step 3 of that plan or after step 6, not in the middle.
- **In the Shootout's repository (`gbdev/GBEmulatorShootout`).** The harness class
  `emulators/nubo.py`, its entry in `main.py` and the README line belong to that project and go in
  through a pull request from the maintainer's fork (Part 4). Until then the class's draft lives in
  `.github/shootout/nubo.py` and the `EMULATOR_SPECS` entry in `.github/workflows/shootout.yml`, which
  CI runs (Part 3), never in the Nubo crates: ACCURACY_PLAN.md keeps harnesses for other emulators
  out of them.
- **In the test ROM runner (`tools/nubo-test-rom-runner`), the nightly workflow and `cargo xtask metrics`.** Part 5 runs
  the Shootout's tests headless with its reference images and tolerance, and compares the result with
  the published table. It needs no new tool: the runner already runs daid's and CasualPokePlayer's
  Shootout tests this way (`Check::GrayscaleScreen`).

Everything below about the Shootout was read from its repository,
[gbdev/GBEmulatorShootout](https://github.com/gbdev/GBEmulatorShootout), at commit `38b926b` (July
2026). Check that it still holds before starting, since the harness changes now and then.

## How the Shootout tests an emulator

- The harness is a set of Python scripts that run on **Windows only**. It needs pywin32, pyautogui and
  Pillow (`requirements-core.txt`, `requirements.txt`) and 7-Zip to unpack downloads.
- Each emulator is a class in `emulators/<name>.py` that subclasses `Emulator` (`emulator.py`):
  - `setup()` downloads the emulator's **latest GitHub release** with
    `downloadGithubRelease(repo, file, filter=...)`, unpacks it into `emu/<name>` with `extract`, and
    calls `setDPIScaling(exe)`, which marks the program as aware of high-DPI screens so Windows doesn't
    stretch its window. Both `downloadGithubRelease` and `extract` do nothing when their target already
    exists, so an unreleased build can be tested by putting it in `emu/<name>/` by hand.
  - `startProcess(rom, model=..., required_features=...)` starts the program with the ROM and returns
    the `subprocess.Popen`, or `None` when the emulator can't run that model. The test then counts as
    not run rather than failed.
  - It is listed in `EMULATOR_SPECS` in `main.py` (`factory`, `keywords`, `name`, `url`) and in the
    README's "Tested emulators" list.
- For every test, `Emulator.run`:
  1. deletes `<rom>.sav` next to the ROM,
  2. starts the program and waits up to 30 seconds for a window whose title passes `title_check`
     (by default: the title starts with the emulator's name),
  3. every 0.1 s takes a screenshot of the inside of that window (`util.getScreenshot`:
     `GetClientRect` plus a pyautogui screenshot of that part of the screen) and compares it with the
     test's pass image, then with its `.fail.png` if there is one,
  4. stops at the first match, or after `runtime / speed + startup_time + 5` seconds, when the test
     fails (or counts as "info" when it has no reference image).
- `util.compareImage` turns both images into 8-bit grayscale and calls them equal when no pixel differs
  by more than 50. Pillow compares only the area both images cover, starting at the top-left corner,
  so **the inside of the window must start with the game screen at 1×**. `measureStartupTime` (run
  with `--get-startuptime`) even needs the inside of the window to be exactly 160×144 pixels.
- Because these are screenshots of the real screen, anything on top of the emulator's window spoils
  them. Nothing else can be used on the PC during a run.
- Models are `DMG`, `CGB` and `SGB`, with no hardware revisions. The only feature a test can require
  is `PCM`: the PCM12/PCM34 registers, which some SameSuite tests read.
- An emulator can set `self.speed` when it runs faster than real time. GameRoy runs at 15×.
- `main.py` writes `<emulator>.json` with each test's result and screenshot; `build.py` turns those
  files into the HTML table.

## Where Nubo stands

What already works:

- Releases have a Windows zip, `Nubo-windows-x64.zip`. It unpacks into a `Nubo` folder, so the program
  is `Nubo/nubo.exe` (`xtask/src/windows.rs`). The release also has `Nubo-windows-x64.msi`, which
  `downloadGithubRelease`'s default filter (`"win" in name`) would match too, so the harness class
  must filter on the zip's name.
- `nubo.exe <rom> --model dmg|cgb` (`frontends/nubo/src/command_line.rs`). `dmg` is `Model::Dmg`
  and `cgb` is `Model::CgbE` (`crates/nubo-core/src/model.rs`).
- While a game runs, the window title is `<game> — Nubo` (`App::title` in `frontends/nubo/src/app.rs`).
  The class's `title_check` has to be `title.endswith("Nubo")`.
- The default palette for original Game Boy games is plain gray (`DmgPalette::Gray`): the emulator's
  evenly spaced grays (white, 0xAA, 0x55, black, `DMG_SHADES`), the same as the references'.
- Nubo has its own boot ROMs, so the harness doesn't need to download any.

What was missing before Part 1 (which added all three):

1. **The window shows more than the game screen.** It opens at 4× with 24 px of padding and a 52 px
   top bar, and can't be made smaller than 480×420 (`frontends/nubo/src/main.rs`, `DEFAULT_SCALE_FACTOR`,
   `SCREEN_PADDING`, `TOP_BAR_HEIGHT`, `MINIMUM_SIZE`). The game view (`frontends/nubo/src/views/game.rs`)
   also draws the auto-hiding bar, toasts, the pause menu and the fast forward badge.
2. **Runs aren't independent.** Settings (`settings.toml`), battery saves (`<rom>.sav`, `<rom>.rtc`), the
   library and its pictures live in the per-user folders (`ProjectDirs::from("", "", "Nubo")` in
   `crates/nubo-frontend/src/storage/files.rs`, under `%APPDATA%\Nubo` on Windows). The harness only deletes a
   `.sav` next to the ROM, so the MBC and clock tests (rtc3test, CasualPokePlayer's tests) would start
   from the save of the previous run. A player's own settings, such as a green palette, would also
   change the results.
3. **No way to run faster than real time from the command line.** The suite would take about 265 ×
   (runtime + a few seconds) at normal speed.

## Expected results

Part 5 replaced this estimate with a real count: all 261 tests the harness class would run pass,
the three Super Game Boy tests included since Nubo emulates the Super Game Boy. The other three only
report information (see "What Part 5 found"). The estimate as it was:

This is an estimate: each Shootout test was matched by ROM file name and model against
`tools/nubo-test-rom-runner/passing.txt`. Nubo's own runner checks registers exactly where the
Shootout checks screenshots, so a real run may differ.

| Suite | Tests | Already passing in Nubo's runner | Notes |
|---|---|---|---|
| Mooneye | 112 | 97 | The other 15 test one revision (`boot_regs-sgb`, `-dmg0`, `-mgb`, `-A`, `-C`, ...) but run on a plain DMG or CGB, where every emulator fails them |
| SameSuite | 76 | 67 | 7 test one CGB revision (`-cgb0B`, `-A`, ...); 2 need the Super Game Boy |
| Blargg | 50 | 3 by name | Nubo's runner only runs the combined ROMs (`cpu_instrs`, `dmg_sound`, `cgb_sound`, `oam_bug`, `mem_timing`) and passes them all, so the separate ones should pass too |
| Acid | 5 | 3 | `which.gb` on two models only reports which model it sees |
| daid | 9 | 8 | `rom_and_ram.gb` only reports |
| CasualPokePlayer | 4 | 3 | `sgb-ext-test` needs the Super Game Boy |
| rtc3test | 3 | 3 | Nubo's runner uses an emulated clock; see `--fresh` below |
| Ashiepaws | 3 | 2 | The Shootout runs Bully on DMG; Nubo's runner only runs it on CGB because real DMGs fail it too |
| Mealybug | generated | most | The Shootout runs both its CGB-C and CGB-D references on one CGB model; as CGB-E, Nubo should match the CGB-D ones |

The estimate assumed Nubo didn't emulate the Super Game Boy, so the class would return `None` for
`SGB` and those tests would count as not run. It now does, and the class starts it with `--model sgb`.

## Part 1: command-line options in the desktop app

One pull request in this repository, following AGENTS.md (doc comments, clippy, both clippy runs,
`cargo test -p nubo`). All three options are desktop-only: the browser build must still compile and
must ignore or leave them out.

1. **`--scale <n>`**: a window that shows only the game screen at `n`× (1 to 8, say).
   - The inside of the window is exactly 160·n × 144·n **physical** pixels, whatever the Windows display
     scaling is. Iced and winit size windows in logical pixels and multiply them by the monitor's scale
     factor, so either cancel that factor (check whether Iced 0.14's application builder has
     `scale_factor`) or divide the size by it. The harness marks `nubo.exe` as high-DPI aware, so
     Windows itself won't stretch the window.
   - No top bar, padding, toasts, pause menu or fast forward badge, and nothing the mouse or keyboard
     can bring up. The picture starts at the top-left corner, with nearest-neighbour scaling. There's
     no minimum window size.
   - Keys and controllers still play the game, and closing the window ends the program.
   - A ROM is required. Without one, print an error and exit with a non-zero code.
   - Suggested shape: a screen-only flag on `App` from `parse_args`, honoured by the window settings in
     `main.rs` and by `views/game.rs`. Put the size calculation in a function with a unit test.
2. **`--fresh`**: start like a new install and leave nothing behind.
   - Use default settings: don't read or write `settings.toml`.
   - Read and write no battery saves. The core's `NoSaves` effects or a temporary folder will do.
   - Don't use the resume slot, the library, the folder watcher, title screens, box art or thumbnails.
   - Make the cartridge clock follow emulated time (`RtcClock::EmulatedTime`, as the test ROM runner
     does). This makes runs repeatable, and it keeps rtc3test working with `--speed`: on the wall clock,
     a game running at 4× would see its clock run at a quarter of the expected speed.
   - Report problems on stderr instead of in toasts.
3. **`--speed <n>`**: start with fast forward on at `n`×, using the existing fast forward path. Check
   which speeds `FastForwardSpeed` allows.
4. Document the options. Update the doc comment at the top of `main.rs`, the commands in
   `frontends/nubo/AGENTS.md`, and the README if it lists command-line options.
5. Add unit tests for `parse_args` (new options, missing or invalid values) and for the window size.

A cloud session has no display, so the window itself can only be checked on a real desktop. Ask the
maintainer to try `nubo.exe <rom> --scale 1 --fresh --speed 4` on Windows at 100% and 150% display
scaling, and to take a screenshot of the window to confirm it is exactly 160×144.

Then a release (the `release` skill), because the harness downloads the latest one.
Before releasing, Part 3 can use a locally built `nubo.exe` put in `emu/nubo/Nubo/` by hand.

### What Part 1 found

Built on 2026-10-14. `frontends/nubo/src/command_line.rs` reads the options and explains them. Where
the build differs from the steps above, and what was checked:

- **`--scale`.** Iced 0.14's `scale_factor` multiplies the display's own scale factor, which isn't
  known before the window opens. So the window opens at 160·n × 144·n logical pixels, and when it
  opens (and whenever the display's scaling changes) the app asks for the scale factor and resizes it
  to 160·n ÷ factor logical pixels, which winit rounds to exactly 160·n physical pixels
  (`app::screen_only_size`, with a unit test for every scale at the usual factors). The window can't
  be resized by hand, and keeps its title bar, since the harness finds it by its title.
- **Checked on a virtual X display (Xvfb)** in a cloud session, with `WINIT_X11_SCALE_FACTOR` for
  the display's scaling: `--scale 2` gives an inside of exactly 320×288 pixels at 100%, 125%, 150%
  and 200%, filled by the picture with nearest-neighbour scaling. With `--scale 1 --fresh --speed 4`,
  the window's pixels match the Shootout's `dmg-acid2` reference exactly, and `cgb-acid2`'s within 38
  of the 50 the harness allows. Nubo wrote no files.
- **`--fresh` keeps more out than the plan listed.** Besides settings, battery saves, the resume slot,
  the library and the folder watcher, a `--fresh` game offers no save states, cheats or screenshots
  (`Game::keeps_data`), and settings changed in the window aren't saved. Battery saves are discarded
  through `nubo_frontend::session::Saves::Discarded`, which also puts the cartridge clock on emulated
  time.
- **`--speed`** takes the fast forward speeds the settings offer (2, 3, 4 and 8) and starts the
  emulator fast forwarding at that speed from its first frame, for the whole run, without changing
  the saved speed (`StartOptions::fast_forward`).
- **Mistakes on the command line now stop the app** with a message and exit code 2, `--model`'s
  included, which used to be ignored. With `--scale`, a ROM that can't be read or started stops the
  app with exit code 1 before the window opens, since the window has nowhere to show why.

Checked on Windows with 0.6.0 in Part 3: at 100% display scaling the inside of the window is
exactly 160×144 (the harness's `--get-startuptime` compares it pixel for pixel).

**Done when:** the three options are merged and documented, and a screenshot taken by hand shows a
160×144 window with only the game in it.

### Optional: a dry run without Windows

Before Part 3, a cloud session can estimate the results with the headless frontend:

1. Run each Shootout test with `cargo screenshot <rom> --seconds <s> --model dmg|cgb`, for the boot
   time plus the test's `runtime`.
2. Compare the PNG with the test's references using the same rule as `compareImage` (grayscale, no
   pixel more than 50 off). Pillow is installed in the cloud container; run Python with `-I` and keep
   the script in the scratchpad, not in the repository.

This checks only the last frame, while the harness accepts the first frame that matches, so it is an
approximation. It still catches wrong palettes, models or timing, and improves the table above. Once
Part 5 is built, `cargo test-roms` does this properly and the dry run is no longer needed.

## Part 2: the harness class

The Shootout's `emulators/nubo.py` is `.github/shootout/nubo.py`, ready to copy into a clone of the
Shootout for Part 3; the nightly run uses the same file, so it stays the one copy. It was checked on 2026-10-14 against the options as merged (`frontends/nubo/src/command_line.rs`) and
against the Shootout's `main` at `38b926b`, still its newest commit then: `downloadGithubRelease` takes
`filter` and `require_asset`, and the window title still ends with "Nubo". Part 3 ran it on Windows
with 0.6.0 and fixed two mistakes in the first draft (see "What Part 3 found"), and set `startup_time`
from `--get-startuptime`. The test ROM runner's `HARNESS_SPEED` and `HARNESS_STARTUP_MILLISECONDS`
(`tools/nubo-test-rom-runner/src/shootout.rs`) must stay this class's `speed` and `startup_time`.

It downloads the latest release's `Nubo-windows-x64.zip` and starts
`nubo.exe <rom> --model dmg|cgb|sgb --scale 1 --fresh --speed 4 --raw-screen`. `--raw-screen` shows the
emulator's own picture: plain grays and no color correction or frame blending, whatever the defaults
are, as the harness has GSE (`--apply-color-correction false`), Emulicious, bgb and SameBoy turn off
theirs. Color correction is on for players since 0.9.0 and would cost the three CGB `rtc3test`s; the
option is in releases from 0.9.0 on, so the pull request to the Shootout waits for 0.9.0.

On a Super Game Boy, `--scale 1` makes the window 256×224, the TV picture with the border around the
Game Boy screen at (48, 40), as mGBA's and NO$GMB's classes expect too; `getScreenshot` crops the
screen out of it.

It also needs an entry in `EMULATOR_SPECS` in `main.py`, which the "Add Nubo to the Shootout" step of
`.github/workflows/shootout.yml` adds, ready to copy.

It also needs a line in the README's "Tested emulators" list, in alphabetical order (between NO$GMB and
PyBoy).

**Done when:** the files are ready to copy into a clone of the Shootout; they don't belong in the Nubo
crates. **Done**: `.github/shootout/nubo.py` and the workflow's `EMULATOR_SPECS` entry.

## Part 3: a local run on Windows (maintainer)

1. Install Python 3 (and 7-Zip, which only other emulators' downloads need). Clone `gbdev/GBEmulatorShootout`, then run
   `python -m pip install -r requirements-core.txt -r requirements.txt`.
2. Add the files from Part 2. Releases from 0.9.0 on have `--scale`, `--fresh`, `--speed`,
   `--raw-screen` and the Super Game Boy, so the harness downloads the latest one. To test an unreleased change, copy a
   build of `main` (`cargo xtask windows` makes the zip) to `downloads/nubo.zip`, as CI does, so
   `downloadGithubRelease` leaves it alone and `extract` unpacks it.
3. Run a quick subset first: `python main.py --emulator nubo --test daid`. Then measure startup time
   with `python main.py --emulator nubo --get-startuptime` and set `startup_time` from it.
4. Run everything for Nubo: `python main.py --emulator nubo`. Use 100% display scaling, and don't touch
   the PC while it runs.
5. Optionally run all emulators (`python main.py`) and build the table with `build.py`. This is the
   comparison Phase 1 of ACCURACY_PLAN.md asks for. Record each emulator's version and the date.
6. Compare `nubo.json` with the expected results above. For each surprise, run the same ROM in
   `cargo test-roms` or `cargo screenshot` to tell a Nubo bug from a harness problem (palette, scale,
   timing, window title). Fix Nubo bugs here as usual, and keep FAILING_TESTS.md up to date.

**Done when:** a full run whose failures are all explained.

### What Part 3 found

Run on 2026-10-14 on Windows 11 at 100% display scaling, with Python 3.14, the Shootout's `main` at
`38b926b` and Nubo 0.6.0 as the harness downloads it (the latest release, with Part 1's options and the
Super Game Boy). Only Nubo was run, not the other emulators (step 5).

- **All 261 tests the harness judges pass**, the number Part 5 counts, which would rank Nubo first ahead
  of DocBoy's 255. The other three: `which.gb` on DMG and CGB only reports (no reference image), and
  `daid/rom_and_ram.gb` doesn't run, since Nubo refuses its cartridge type (0x08, ROM with RAM and no
  MBC) and exits; it has no reference image either, so it counts for no emulator. Nubo runs these
  cartridges now (`CartridgeMapper::RomRam`, after 0.7.0), so the test starts and shows what it finds
  (`FAILING_TESTS.md`).
- **The first full run had four failures that weren't Nubo's**: SameSuite's `channel_2_duty`,
  `channel_3_freq_change_delay`, `channel_3_restart_delay` and `channel_4_lfsr` ran to their deadline,
  and their screenshots showed another window on top of Nubo's, which the harness captured instead.
  Rerun with nothing on top, all four passed in about 1.3 seconds. Check the screenshots in
  `nubo.json` before believing a failure.
- **Two mistakes in the first draft of the harness class**, fixed in Part 2:
  - Its download filter, `name.endswith("-windows-x64.zip")`, matched the RetroArch core's
    `Nubo-libretro-windows-x64.zip` first, so the harness unpacked the core instead of the app. It now
    asks for `Nubo-windows-x64.zip` by name.
  - Started with the relative path `emu/nubo/Nubo/nubo.exe` and `cwd` set, the program wasn't found
    (`FileNotFoundError`), so every test failed to run. It now passes the absolute path.
- **Startup time** (`--get-startuptime`): 1.41 s on DMG, 1.32 s on CGB and 1.35 s on SGB, so the class's
  `startup_time` was set to 1.5. It is 2.5 now, the slowest start measured (in CI, below), since the
  Shootout's machine may be slower than this one. The value only lengthens each test's deadline
  (counted from when the window opens) and passing tests stop at their first match, so a larger one
  costs time only on tests with no reference image or that fail. The test showed the window's inside
  is exactly 160×144.
- **7-Zip isn't needed for Nubo**: `extract` unpacks a `.zip` with Python's `zipfile`. Only other
  emulators' downloads need it.
- With nearly every test passing early, the whole run took about 7 minutes.

### Part 3 in CI

`.github/workflows/shootout.yml` runs the same harness on a GitHub Windows machine. The nightly run,
which every release runs before it publishes anything, runs it after its Windows job, on the zip that
job built: the file a release publishes and the Shootout downloads. It copies the zip to
`downloads/nubo.zip` in a checkout of the Shootout at the commit the test ROM runner pins, adds the
class and its `EMULATOR_SPECS` entry, measures the startup time and runs every test. It fails unless
every Shootout test in `passing.txt` passes there too, counting a test that crashed or never opened a
window as failed, and keeps `nubo.json` (with each test's last screenshot), the startup measurement and
the log as its artifact. Runs started by hand from the Actions tab build their own zip with the Windows
workflow; pull requests don't run it.

The first CI run, on 2026-10-14 with a build of `main` before 0.7.0, matched the run above: all 261
tests passed in about 4.5 minutes, `which.gb` ran to its deadline on both models, and
`daid/rom_and_ram.gb` exited before its window opened, as it did above (Nubo runs that cartridge type
now). GitHub's Windows machines have no graphics card and no sound device; the app draws and keeps
time without them, and nothing covers its window. The
startup time measured 2.5 s for the first start (DMG) and 0.8 s and 0.9 s after it (CGB, SGB); the
class's 1.5 s then only lengthened each test's deadline, and every test passed long before it. The
class now uses 2.5 s, this first start.

## Part 4: the pull request to the Shootout

Wait until Part 5 shows Nubo ranking first on the published tests, and Part 3's run on Windows confirms
it: the maintainer wants Nubo to enter the table at the top. Wait as well until the maintainer has
reviewed Nubo themselves and the app is ready to go public, the UI above all: the Shootout links to Nubo,
so it is where many people will first see it. Then, only once the maintainer says to go
ahead, open a pull request to `gbdev/GBEmulatorShootout` from the
maintainer's fork with `emulators/nubo.py`, the `EMULATOR_SPECS` entry and the README line. Mention the
Nubo version it was tested with and the local results. The Shootout's maintainers rerun the suite and
publish the table at gbdev.io/GBEmulatorShootout.

**Prepared** on 2026-10-15, at the maintainer's go: the branch `add-nubo` of
[mschoenm/GBEmulatorShootout](https://github.com/mschoenm/GBEmulatorShootout/tree/add-nubo), on the
Shootout's `main` at `38b926b`. Besides the three changes above it adds `ci-nubo.yml` and a row in
`ci-all.yml`, the per-emulator CI workflows the Shootout's GSE pull request added too. Its
`emulators/nubo.py` is `.github/shootout/nubo.py` without the header comment; keep the two the same.
**Opened** on 2026-10-15 as
[gbdev/GBEmulatorShootout#102](https://github.com/gbdev/GBEmulatorShootout/pull/102), after 0.9.2 and
a local run on Windows with it: 261 passed, none failed, and `which.gb` (DMG and CGB) and
`daid/rom_and_ram.gb` only report information. `--get-startuptime` measured 1.44 s (DMG), 1.38 s
(CGB) and 1.34 s (SGB). The Shootout had four other pull requests adding emulators open since before
July, so it may wait a while; they touch the same lines of `main.py` and the README, so it may need
a rebase.

Afterwards, mark Phase 6 step 2 as done in ACCURACY_PLAN.md. CI runs the harness on every release
(Part 3), so a release that would lose a row fails before it is published; when one gains rows, ask the
Shootout's maintainers to rerun, so the published row doesn't fall behind.

**Done when:** Nubo appears in the published table.

## Part 5: the Shootout's tests in CI, the nightly run and the metrics

The test ROM runner already runs most of the Shootout's ROMs, but with its own checks (registers,
exact screenshots) and on the models each ROM names. This part also runs every Shootout test the way the
Shootout judges it, and compares the result with the published table. That way:

- **CI:** a change that would cost Nubo a row in the table fails, like any other test that stops passing.
- **The nightly run** checks the same, with the real boot ROM (`--verify-boot`).
- **The nightly metrics** on the wiki show Nubo's Shootout score and where it would rank, night by
  night, until it is ready for Part 4.

### The published table

The table at gbdev.io/GBEmulatorShootout is built from the Shootout's `gh-pages` branch: `tests.json`
lists the tests it shows, and `<emulator>.json` holds each emulator's result per test (with a
screenshot, so the files add up to 6.4 MB). At `gh-pages` commit `72fd745` (published 2026-06-04), the
table has 264 tests. The top of it reads:

| Emulator | Passed | Failed | Run |
|---|---|---|---|
| DocBoy | 255 | 3 | 261 |
| Emulicious | 251 | 7 | 261 |
| SameBoy | 250 | 11 | 264 |
| Beaten Dying Moon | 244 | 17 | 264 |
| GSE | 215 | 46 | 264 |
| bgb | 202 | 59 | 264 |

The other rows are counted as "info": tests with no reference image, such as `which.gb` and
`rom_and_ram.gb`, and the tests an emulator can't run, such as the Super Game Boy ones.

The plan assumed the published list was older than the Shootout's `main`, with no Mealybug tests and
none of the tests that target one hardware revision. At the pinned `main` commit `38b926b` the two lists
turned out to be the same 264 tests: `main` runs only the DMG Mealybug tests, and the revision tests are
commented out. "Would rank first" is still counted on the published list, so it stays right when they
part again.

### Steps

1. **The scoreboard.** Check in `shootout_published.txt` next to `shootout.sha256`:
   - It records the pinned `gh-pages` commit and its date.
   - It has one line per published test with the emulators that pass it, for example
     `mooneye/acceptance/div_timing.gb<TAB>DocBoy,Emulicious,SameBoy,...`.
   - It is written by a small command (for example `cargo xtask shootout-scoreboard <commit>`) that
     fetches `tests.json` and the `<emulator>.json` files at that commit, so CI never downloads them.
   - Update it when the Shootout publishes again, in a pull request of its own.
2. **Files.** Of the Shootout's `testroms/` on `main` (32 MB), the ROMs are 31 MB, and almost all are
   already in the c-sp collection the runner downloads (four large Mooneye MBC ROMs alone are most of
   it). The reference images are 1.3 MB.
   - Download only the reference images (`.png` and `.fail.png`) and the few ROMs the collection
     lacks, at the pinned `main` commit the runner already uses, and add them to `shootout.sha256`.
   - Use the collection's copy of a ROM only when its SHA-256 matches the Shootout's.
   - The test ROM cache in CI (`.github/actions/test-roms`) covers them through `shootout.sha256`.
3. **Tests.** Read the test list of the pinned `main` commit once and keep it as a table in the runner,
   regenerated when the pin moves, like `SCREENSHOT_TESTS`. Don't run the Shootout's Python. For each
   test:
   - **ID:** `shootout/<name as the Shootout writes it>`, for example
     `shootout/mealybug-tearoom-tests/ppu/m3_scy_change.gb (CGB RevD)`. It can't clash with the tests
     that already run, and it matches the scoreboard's names.
   - **Model:** `DMG` runs on `Model::Dmg`, `CGB` on `Model::CgbE` and `SGB` on `Model::Sgb`, the
     models the harness class starts. (`SGB` tests were skipped at first, as in the Shootout, where
     they count as not run; they run since Nubo emulates the Super Game Boy.)
   - **Check:** pass when a pass image matches under the Shootout's rule (grayscale, no pixel more than
     50 off), fail when a `.fail.png` matches, and fail at the deadline.
   - **Sampling like the harness:** the harness takes a screenshot only every 0.1 s of real time, which
     at the harness class's `--speed 4` is every 0.4 emulated seconds. A picture shown for less than
     that may never be seen. So a match only counts once it has held for 0.1 s × the harness speed of
     emulated time, whatever frame the harness happens to sample. Keep the harness speed as one
     constant next to the test list, the same value `emulators/nubo.py` passes.
   - **Deadline:** the test's `runtime` plus a margin of 2 seconds, counted from power-on, since the
     Shootout's `runtime` includes the boot ROM.
   - Tests with no reference image only report information. Leave them out.
   - **Already covered:** daid's and CasualPokePlayer's tests already run this way. Move them under
     the `shootout/` IDs instead of running them twice, with their entries in `passing.txt`.
4. **Results.** List the passing ones in `passing.txt` as usual, so CI fails when one stops passing. In
   `FAILING_TESTS.md`, explain the failing ones in groups:
   - tests of one hardware revision run on another model
   - Bully on DMG, which real DMGs fail too
   - real Nubo bugs, each with what to try
5. **The summary line.** After the per-suite lines, the runner prints one line the nightly can read,
   counted on the published tests only:

   ```
     shootout  259/264  rank 1  leader DocBoy 255  published 2026-06-04 72fd745
   ```

   Nubo's rank is one more than the number of emulators that pass more published tests than it does.
   A tie counts as sharing the rank.
6. **The nightly metrics.** Nothing new runs at night: `cargo test-roms --verify-boot` in the
   `verify-boot` job already runs the Shootout's tests.
   - **Reading the result:** next to the step that counts the passing tests, read the `shootout` line
     into job outputs. In "Write tonight's metrics", add them to `night.json`:

     ```json
     "shootout": {"passed": 259, "total": 264, "rank": 1, "leader": "DocBoy", "leader_passed": 255,
                  "published": "2026-06-04", "published_commit": "72fd745"}
     ```

     Each night keeps the publication its score was counted against, so the history stays right after
     `shootout_published.txt` moves to a newer publication.

   - **On the wiki page:** in `cargo xtask metrics report` (`xtask/src/metrics.rs`), put a line at the
     top of the "Nightly metrics" page, such as "GB Emulator Shootout: 259 of 264, would rank 1st
     (leader: DocBoy, 255; published 2026-06-04)". Add a "Shootout" column to `HISTORY_COLUMNS`.
   - **Threshold:** give `shootout.passed` none, like `test_roms_passed`. A drop already fails CI
     through `passing.txt`, and the column shows the trend.
   - **Tests:** cover the new line and column in `xtask/src/metrics/tests.rs`.
7. **Docs.**
   - `tools/nubo-test-rom-runner/AGENTS.md` and the runner's module docs: the Shootout tests, the scoreboard and the summary line
   - `docs/CI.md`: the Shootout score in the metrics

   In the README: the test suite list.

**Cost.** Estimated on 2026-10-13 in a 4-core cloud container, before it was built:
- Nubo ran all 294 Shootout ROMs for 2 emulated seconds on both DMG and CGB-E in 7.0 s with
  `--jobs 4`. That is about 1,180 emulated seconds, or 170 per second of real time.
- The Shootout's roughly 310 tests on `main` add up to about 555 emulated seconds of `runtime`; the
  longest is 20 s.
- With the 2-second margin, that is at most about 1,200 emulated seconds. This is the worst case, in
  which no test finishes early.
- So the worst case is **about 7 seconds** on 4 cores, and passing tests stop at their first matching
  frame, so it is less in practice.
- GitHub's Linux runners for public repositories also have 4 cores. On top of today's roughly 30
  seconds, that is up to about a quarter more. At night, `--verify-boot` also emulates the boot ROM of
  each of these tests instead of starting from a copy, as it does for every other test.
- The runner starts the longest tests first, so the 20-second ones overlap with the rest.

Measured once it was built, on 2026-10-13 in the same 4-core container, three runs each of the whole
`cargo test-roms` with and without the Shootout's 258 tests (other work shared the machine, so the
CPU time is the steadier number):
- With them: about 88.7 CPU seconds. Without them: about 76.0. So they add **about 13 CPU seconds,
  about 3 seconds on 4 cores**, or 17%. On an idle machine the whole run took 23.5 to 26 seconds with
  them.
- On their own (`cargo test-roms shootout`) they take 3.5 to 4.5 seconds, and 9 seconds with
  `--verify-boot`, which runs the boot ROM for each of them.
- Every test passes long before its deadline, so the longer deadline below costs nothing unless tests
  start failing; a failing test then runs for its `runtime` plus 30 emulated seconds.

**Done when:**
- `cargo test-roms` runs the Shootout's tests in CI and at night, and prints the summary line.
- `passing.txt` and `FAILING_TESTS.md` list them.
- The "Nightly metrics" wiki page shows the score and rank at the top, and a column in its history.
- The extra time is measured and noted.

### What Part 5 found

Done on 2026-10-13. `cargo test-roms` prints
`  shootout  251/264  rank 2  leader DocBoy 255  published 2026-06-04 72fd745`; with evenly spaced
grays (below), `258/264  rank 1`. Where the build
differs from the steps above, and why:

- **Files.** Only 53 of the 258 tests' ROMs are in the c-sp collection with the Shootout's SHA-256
  (the acid tests, Blargg's sound tests and `interrupt_time`, Strikethrough and the DMG Mealybug tests);
  the collection's Mooneye, SameSuite, `cpu_instrs`, `mem_timing`, `oam_bug` and other ROMs are other
  builds. So the runner downloads 202 ROMs (29 MB, most of it four large Mooneye MBC ROMs) and 259
  reference screenshots (0.25 MB) from the Shootout, once; CI caches them through `shootout.sha256`.
  The download's marker now holds the manifest's checksum, so a changed manifest downloads the new
  files on machines that already have the old ones.
- **Deadline.** "The `runtime` plus 2 seconds from power-on" failed 100 of the 258 tests that pass
  with more time: the CGB boot ROM alone takes 3.1 seconds and the DMG's 1.7, and some tests need up
  to 8.5 seconds more than their `runtime`. The runner uses the harness's own deadline instead,
  `runtime / speed + startup_time + 5` seconds of real time, which at speed 4 with the class's
  `startup_time` of 2.5 seconds (1 in the first draft, then 1.5) is the `runtime` plus 30 emulated
  seconds.
- **Seven failing tests, all for the colors.** None of the plan's expected failures happen: the
  revision tests aren't in the list, and Nubo passes the Shootout's build of Bully on DMG. But the
  harness compares a screenshot of the app, so the runner compares the frame in the colors the app
  shows, not mapped to the screenshots' grays as the other screenshot checks are. Nubo's dark gray
  (0xA9) is 84 steps of brightness from the screenshots' (0x55), more than the 50 the comparison
  allows, so the seven DMG tests with dark gray in their screenshots fail. `FAILING_TESTS.md` lists
  them with what to try, and explains the six published tests that aren't counted. The maintainer
  chose to make the emulator's own grays evenly spaced (white, 0xAA, 0x55, black), so the default
  palette shows them too: the seven pass, and Nubo passes 258 of 264, which would rank it first.
- **daid's and CasualPokePlayer's tests run twice.** Since the two comparisons now judge different
  things (the emulation, in the screenshots' grays, and what the harness would see), their old
  checks under their own IDs stay, next to the `shootout/` ones.
- **Fail screenshots.** The Shootout has no `.fail.png` at the pinned commit. The check handles them,
  and a regenerated list adds them when they appear.
- **The publication date** in `shootout_published.txt` is the day of the newest result in the
  emulators' files (2026-06-04, the day of `gh-pages` commit `72fd745`), since the GitHub API that
  would give the commit's date isn't reachable from every machine. The command takes the full commit
  hash.
