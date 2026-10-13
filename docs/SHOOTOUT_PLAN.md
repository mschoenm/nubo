# Plan: adding Nubo to the GB Emulator Shootout

The [GB Emulator Shootout](https://gbdev.io/GBEmulatorShootout/) runs about 265 test ROMs on 17 Game Boy
emulators and publishes a table of their results. Getting Nubo into it is step 2 of Phase 6 in
[ACCURACY_PLAN.md](ACCURACY_PLAN.md), and running its harness locally on all emulators also covers much
of Phase 1 (measuring other emulators on the same tests).

The work has four parts, plus an optional fifth:

- Parts 1 and 2 can be done in a cloud session.
- Part 3 needs a Windows PC and is done by the maintainer.
- Part 4 is a pull request to another project's repository, so it only happens once the maintainer says
  to go ahead.
- Part 5 adds the Shootout's checks to `cargo test-roms`.

## Where the code lives

- **In this repository, in the Iced app (`frontends/iced`).** The Shootout downloads the Windows zip of
  Nubo's latest release and takes screenshots of the real `nubo.exe`, so the options it needs
  (`--scale`, `--fresh`, `--speed`, Part 1) must be in the app Nubo ships, not in a separate program.
  - The options are parsed in `main.rs`.
  - The screen-only window is in the window settings and `views/game.rs`.
  - The steps of [RESTRUCTURE_PLAN.md](RESTRUCTURE_PLAN.md) leave the options in the Iced app. What
    `--fresh` turns off (storage, battery saves) moves to `nubo-frontend` in its step 4. Do Part 1
    before step 3 of that plan or after step 6, not in the middle.
- **In the Shootout's repository (`gbdev/GBEmulatorShootout`).** The harness class
  `emulators/nubo.py`, its entry in `main.py` and the README line belong to that project and go in
  through a pull request from the maintainer's fork (Part 4). Until then their draft lives in this
  document (Part 2), never in the Nubo crates: ACCURACY_PLAN.md keeps harnesses for other emulators
  out of them.
- **Optionally, in the test ROM runner (`tools/test_rom_runner` after the restructuring's step 1,
  `frontends/test_rom_runner` before it).** Part 5 runs the Shootout's tests headless with its
  reference images and tolerance, so `cargo test-roms` catches a Shootout regression before a release
  does. It needs no new tool: the runner already runs daid's and CasualPokePlayer's Shootout tests
  this way (`Check::GrayscaleScreen`).

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
  is `Nubo/nubo.exe` (`xtask/src/windows.rs`). The release also has `Nubo-windows-x64-setup.exe`, which
  `downloadGithubRelease`'s default filter (`"win" in name`) would match too, so the harness class
  must filter on the zip's name.
- `nubo.exe <rom> --model dmg|cgb` (`frontends/iced/src/main.rs`, `parse_args`). `dmg` is `Model::Dmg`
  and `cgb` is `Model::CgbE` (`crates/nubo-core/src/model.rs`).
- While a game runs, the window title is `<game> — Nubo` (`App::title` in `frontends/iced/src/app.rs`).
  The class's `title_check` has to be `title.endswith("Nubo")`.
- The default palette for original Game Boy games is plain gray (`DmgPalette::Gray`), the colors the
  references expect.
- Nubo has its own boot ROMs, so the harness doesn't need to download any.

What's missing:

1. **The window shows more than the game screen.** It opens at 4× with 24 px of padding and a 52 px
   top bar, and can't be made smaller than 480×420 (`frontends/iced/src/main.rs`, `DEFAULT_SCALE_FACTOR`,
   `SCREEN_PADDING`, `TOP_BAR_HEIGHT`, `MINIMUM_SIZE`). The game view (`frontends/iced/src/views/game.rs`)
   also draws the auto-hiding bar, toasts, the pause menu and the fast forward badge.
2. **Runs aren't independent.** Settings (`settings.toml`), battery saves (`<rom>.sav`, `<rom>.rtc`), the
   library and its pictures live in the per-user folders (`ProjectDirs::from("", "", "Nubo")` in
   `frontends/iced/src/storage/files.rs`, under `%APPDATA%\Nubo` on Windows). The harness only deletes a
   `.sav` next to the ROM, so the MBC and clock tests (rtc3test, CasualPokePlayer's tests) would start
   from the save of the previous run. A player's own settings, such as a green palette, would also
   change the results.
3. **No way to run faster than real time from the command line.** The suite would take about 265 ×
   (runtime + a few seconds) at normal speed.

## Expected results

This is an estimate: each Shootout test was matched by ROM file name and model against
`frontends/test_rom_runner/passing.txt`. Nubo's own runner checks registers exactly where the
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

Nubo doesn't emulate Super Game Boy commands, borders or palettes, so the class returns `None` for
`SGB` and those tests count as not run.

## Part 1: command-line options in the desktop app

One pull request in this repository, following CLAUDE.md (doc comments, clippy, both clippy runs,
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
4. Document the options. Update the doc comment at the top of `main.rs`, the Iced paragraph and Build
   Commands block in CLAUDE.md, and the README if it lists command-line options.
5. Add unit tests for `parse_args` (new options, missing or invalid values) and for the window size.

A cloud session has no display, so the window itself can only be checked on a real desktop. Ask the
maintainer to try `nubo.exe <rom> --scale 1 --fresh --speed 4` on Windows at 100% and 150% display
scaling, and to take a screenshot of the window to confirm it is exactly 160×144.

Then a release (see "Making a Release" in CLAUDE.md), because the harness downloads the latest one.
Before releasing, Part 3 can use a locally built `nubo.exe` put in `emu/nubo/Nubo/` by hand.

**Done when:** the three options are merged and documented, and a screenshot taken by hand shows a
160×144 window with only the game in it.

### Optional: a dry run without Windows

Before Part 3, a cloud session can estimate the results with the headless frontend:

1. Run each Shootout test with `cargo screenshot <rom> <seconds> --model dmg|cgb`, for the boot time
   plus the test's `runtime`.
2. Compare the PNG with the test's references using the same rule as `compareImage` (grayscale, no
   pixel more than 50 off). Pillow is installed in the cloud container; run Python with `-I` and keep
   the script in the scratchpad, not in the repository.

This checks only the last frame, while the harness accepts the first frame that matches, so it is an
approximation. It still catches wrong palettes, models or timing, and improves the table above.

## Part 2: the harness class

This is a draft for the Shootout's `emulators/nubo.py`. Adjust it to the options as merged, and
calibrate `startup_time` with `--get-startuptime` in Part 3.

```python
from util import *
from emulator import Emulator
from test import *
import os


class Nubo(Emulator):
    def __init__(self):
        super().__init__("Nubo", "https://github.com/mschoenm/nubo", startup_time=1.0, features=(PCM,))
        # While a game runs the title is "<game> — Nubo".
        self.title_check = lambda title: title.endswith("Nubo")
        self.speed = 4.0

    def setup(self):
        # The release also has a setup .exe, which the default filter would pick.
        downloadGithubRelease("mschoenm/nubo", "downloads/nubo.zip",
                              filter=lambda name: name.endswith("-windows-x64.zip"), require_asset=True)
        extract("downloads/nubo.zip", "emu/nubo")
        setDPIScaling("emu/nubo/Nubo/nubo.exe")

    def startProcess(self, rom, *, model, required_features):
        model = {DMG: "dmg", CGB: "cgb"}.get(model)
        if model is None:
            # No Super Game Boy commands, borders or palettes.
            return None
        return subprocess.Popen([
            "emu/nubo/Nubo/nubo.exe", os.path.abspath(rom),
            "--model", model, "--scale", "1", "--fresh", "--speed", "4",
        ], cwd="emu/nubo/Nubo")
```

It also needs this entry in `EMULATOR_SPECS` in `main.py`:

```python
    {
        'factory': lambda: _new_instance("emulators.nubo", "Nubo"),
        'keywords': ["Nubo", "nubo"],
        'name': "Nubo",
        'url': "https://github.com/mschoenm/nubo",
    },
```

It also needs a line in the README's "Tested emulators" list, in alphabetical order (between NO$GMB and
PyBoy).

**Done when:** the files are ready to copy into a clone of the Shootout. They can live in a comment on
the Part 1 pull request or in this document; they don't belong in the Nubo crates.

## Part 3: a local run on Windows (maintainer)

1. Install Python 3 and 7-Zip. Clone `gbdev/GBEmulatorShootout`, then run
   `python -m pip install -r requirements-core.txt -r requirements.txt`.
2. Add the files from Part 2.
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

## Part 4: the pull request to the Shootout

Only once the maintainer says to go ahead: open a pull request to `gbdev/GBEmulatorShootout` from the
maintainer's fork with `emulators/nubo.py`, the `EMULATOR_SPECS` entry and the README line. Mention the
Nubo version it was tested with and the local results. The Shootout's maintainers rerun the suite and
publish the table at gbdev.io/GBEmulatorShootout.

Afterwards, mark Phase 6 step 2 as done in ACCURACY_PLAN.md. Rerun the Shootout locally for releases
that change accuracy, so the published row doesn't fall behind.

**Done when:** Nubo appears in the published table.

## Part 5 (optional): the Shootout's checks in `cargo test-roms`

The test ROM runner already runs most of the Shootout's ROMs, but with its own checks (registers,
exact screenshots) and on the models each ROM names. This part also runs every Shootout test the way the
Shootout judges it, so a change that would cost Nubo a row in the published table fails CI.

1. **Files.** Of the Shootout's `testroms/` (32 MB), the ROMs are 31 MB and almost all are already in
   the c-sp collection the runner downloads (four large Mooneye MBC ROMs alone are most of it). The
   reference images are 1.3 MB.
   - Download only the reference images (`.png` and `.fail.png`) and the few ROMs the collection
     lacks, at the pinned Shootout commit the runner already uses, and add them to `shootout.sha256`.
   - Use the collection's copy of a ROM only when its SHA-256 matches the Shootout's.
2. **Tests.** Read the test list once and keep it as a table in the runner, regenerated when the pinned
   commit changes, like `SCREENSHOT_TESTS`. Don't run the Shootout's Python. For each test:
   - **ID:** `shootout/<name>`, so it can't clash with the tests that already run.
   - **Model:** `DMG` runs on `Model::Dmg` and `CGB` on `Model::CgbE`, the models the harness class
     starts. `SGB` tests are skipped, since Nubo has no Super Game Boy commands.
   - **Check:** pass on the first frame that matches a pass image under the Shootout's rule (grayscale,
     no pixel more than 50 off). Fail on a frame that matches a `.fail.png`, or at the deadline.
   - **Deadline:** the test's `runtime` plus a margin of a few seconds, counted from power-on, since
     the Shootout's `runtime` includes the boot ROM.
   - Tests with no reference image only report information; leave them out.
   - **Already covered:** daid's and CasualPokePlayer's tests already run this way, so don't add them
     twice.
3. **Results.** List the passing ones in `passing.txt` as usual. In `FAILING_TESTS.md`, explain the
   failing ones in groups: tests of one hardware revision run on another model, Bully on DMG (which
   real DMGs fail too), and real Nubo bugs.

**Cost.** Measured on 2026-10-13 in a 4-core cloud container:
- Nubo ran all 294 Shootout ROMs for 2 emulated seconds on both DMG and CGB-E in 7.0 s with
  `--jobs 4`. That is about 1,180 emulated seconds, or 170 per second of real time.
- The Shootout's 310 or so tests add up to about 555 emulated seconds of `runtime`; the longest is
  20 s.
- With a margin of 2 s per test, that is at most about 1,200 emulated seconds. This is the worst case,
  in which no test finishes early.
- So the worst case is **about 7 seconds** on 4 cores, and passing tests stop at their first matching
  frame, so it is less in practice.
- GitHub's Linux runners for public repositories also have 4 cores. On top of today's roughly 30
  seconds, that is up to about a quarter more.
- The runner starts the longest tests first, so the 20-second ones overlap with the rest.

Measure it again once it is built, with and without the new tests, and note the result here.

**Done when:** `cargo test-roms` runs the Shootout's tests, `passing.txt` and `FAILING_TESTS.md` list
them, and the extra time is measured and noted.

