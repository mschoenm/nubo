# Test ROM runner

How `cargo test-roms` works. The workflow every accuracy change follows (run it, `--update`, keep `FAILING_TESTS.md` current) is in the root `CLAUDE.md`.

```bash
cargo test-roms               # Run the test ROM suites and compare with passing.txt
cargo test-roms mooneye       # Only run tests whose name contains "mooneye" (e.g. "cgb blargg", "dmg mealybug", "gambatte/oamdma")
cargo test-roms --update      # Record the results of this run in passing.txt
cargo test-roms --verify-boot # Run the boot ROM before every test and check the boot cache against it (the nightly CI run does this)
cargo test-roms --verbose     # Show why each test fails
```

## Comparing with passing.txt

`cargo test-roms` exits with an error both when a listed test stops passing and when an unlisted test starts passing, so after fixing an accuracy issue run `cargo test-roms --update` and commit the updated list. An unfiltered run also fails when a listed test wasn't run at all (a renamed ROM, a changed model rule, a suite that is no longer collected), listing it under "Listed but not run", and an unfiltered `--update` removes such entries; filtered runs leave the entries of tests outside the filter alone. Failing screenshot tests write the emulator's screen and a `.diff.png` (the reference with differing pixels in red) to `target/test-roms/failures/<model>/`.

## Suites and models

- Downloads a pinned, checksummed release of [c-sp/game-boy-test-roms](https://github.com/c-sp/game-boy-test-roms) into `target/test-roms/`, plus daid's and CasualPokePlayer's tests from the [GB Emulator Shootout](https://github.com/gbdev/GBEmulatorShootout) at a pinned commit (the files listed with their checksums in `shootout.sha256`), and runs Blargg, Mooneye (Gekkio's and Wilbert Pol's), SameSuite, dmg-acid2, cgb-acid2, cgb-acid-hell, Mealybug Tearoom, Gambatte, GBMicrotest, AGE, rtc3test, Bully, Strikethrough, Scribbltests, TurtleTests, little-things-gb, the MBC3 Tester and the shootout tests in parallel (the README lists what isn't run and why).
- Tests run on the hardware models named by each ROM's suffix (Mooneye's G/S/C/A groups and revision lists such as `cgb0BC`, several in a row for AGE such as `-dmgC-cgbBCE`, where `ncm` is a CGB running a DMG game; DMG and CGB-E when there is none) and each Mealybug test on every revision with a reference image (DMG-blob on DMG, CGB-C, CGB-D on CGB-D and CGB-E).

## How tests pass

- Register-based tests pass when B, C, D, E, H, L hold 3, 5, 8, 13, 21, 34 (and fail on 0x42); screenshot tests pass when the normalized frame matches the reference PNG exactly.
- Gambatte's tests run on DMG and CGB-C and are checked 15 frames after the boot ROM like Gambatte's own runner (`gambatte.rs`): the hexadecimal number after `_out` in the file name must be drawn in the top-left corner, `_outaudio0`/`_outaudio1` mean silence or sound in the last frame, and the rest match a PNG in Gambatte's colors. Gambatte's CGB color formula is GPL-2.0-only, so the few non-gray colors Nubo draws in those tests map to RGB through a small table learned from the reference PNGs (an unknown color never matches).
- GBMicrotest writes 0x01 (pass) or 0xFF (fail) to 0xFF82; the ROMs that never write there (test benches and experiments) are skipped.
- rtc3test's three subtests are picked from its menu by pressing buttons after the boot ROM (each `TestCase` can list `input` keys, and the `subtest` name is added to the ID), and the runner's cartridge hooks make the MBC3 clock count emulated time.
- The smaller screenshot tests are listed in `SCREENSHOT_TESTS`; Bully runs on CGB only, since real DMGs fail it too, and firstwhite must show its reference in every frame after the game first turns the screen off (`Check::EveryFrame`), since failing emulators flash text for one frame.
- Register tests also end when the CPU locks up on an undefined opcode, which is how Wilbert Pol's older Mooneye tests finish (`Emulator::is_locked_up`).
- The shootout tests and Mooneye's MGB madness test run on the model they target and are checked one second after the boot ROM against one of their screenshots, in grayscale with the shootout's tolerance (`Check::GrayscaleScreen`), because those screenshots come from several emulators or use Mooneye's own grays.

## Boot cache

Tests start where the boot ROM hands over to the cartridge (`boot.rs`): each worker thread runs the boot ROM once per model and set of header bytes it can see (`Emulator::boot_rom_input`) and starts the other ROMs from a copy (`Emulator::clone_with_rom`), with the timeout still counted from power-on. `--verify-boot` runs the real boot ROM for every test instead and fails any test whose copy would have differed, comparing the whole emulator's `Debug` output; the core test `clone_with_rom_matches_a_real_boot` checks the same for every model with ROMs that differ everywhere outside those header bytes.

## Timeouts and hangs

- Timeouts count emulated time (each test's `Deadline`: seconds from power-on, seconds after the boot ROM, or frames after it), so results don't depend on how fast or busy the machine is (the whole suite normally finishes in about 30 seconds, or about two and a half minutes with `--verify-boot`).
- Hangs are caught three ways, so one never holds up the run for more than a few minutes: a test whose emulated time stops advancing (one audio buffer taking over 60 seconds of real time, `STALL_LIMIT`) fails, the run stops once 3 tests have stalled, and a watchdog ends the process if a test makes no progress for 90 seconds because it is stuck inside the emulator.
- Each test's outcome is stored as it finishes, so a worker that panics only fails the test it was on.
- Tests are started longest timeout first (`TestCase::longest_run_seconds`), so slow ones such as rtc3test and Blargg overlap with the thousands of short ones.
