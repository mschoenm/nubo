# Accuracy plan

Nubo's goal is to be the most accurate Game Boy and Game Boy Color emulator there is. This file says what
it would take to claim that honestly: what the claim has to mean, where Nubo stands, and the work left,
in the order it should happen. Update it as phases finish.

Status on 2026-10-12: `cargo test-roms` passes 6520 of 6557 tests on 12 hardware models, and
[FAILING_TESTS.md](../frontends/test_rom_runner/FAILING_TESTS.md) explains the other 37. That makes Nubo
one of the strongest emulators on public test ROMs, but nobody can verify a "most accurate" claim yet:
other emulators haven't been run on the same tests, nothing has been checked on a real console, and some
hardware isn't emulated at all.

## What the claim has to mean

There is no single accuracy score, so the claim rests on five conditions. All five must hold before
the README or a release says "most accurate".

1. **Test ROMs.** On one pinned set of public test ROMs, run on the hardware models each test targets,
   Nubo passes more tests than every other emulator measured, on both monochrome and color models.
   Every test it still fails is explained.
2. **Hardware.** Where test suites disagree with each other, and where Nubo's timing was fitted to
   another emulator instead of to tests verified on hardware, the behaviour has been checked on real
   consoles and Nubo matches them.
3. **Coverage.** Nubo emulates the hardware licensed games use: the Super Game Boy's functions, the
   link cable, infrared, and the cartridge types and accessories of the commercial library. An emulator
   that can't run a game can't be the most accurate at running it.
4. **Games.** Games and demos known for effects that are hard to emulate look and sound as they do on
   hardware, compared with captures from real consoles.
5. **Reproducible.** Anyone can rerun the comparison with published scripts and get the same table.

## Where Nubo stands

What is strong:

- 6520 of 6557 test ROMs from 20 suites pass, on DMG-0, DMG, MGB, SGB, SGB2, CGB-0 to CGB-E and AGB. The
  JSON CPU tests pass on every opcode, M-cycle by M-cycle.
- Each model leaves the boot ROM in its own state (registers, DIV phase, frame position, the boot
  sound's phase), calibrated against Mooneye's, GBMicrotest's and Gambatte's tests.
- The only comparison so far was a one-off run of SameBoy on Nubo's test set early in the accuracy work.
  SameBoy passed 130 of the roughly 345 tests Nubo failed at the time, and failed 533 tests Nubo passed,
  mostly Gambatte's PPU timing tests. Nubo has fixed about 300 more failures since. This is a hint,
  not a result: Phase 1 repeats it properly.

What is missing or unproven:

- **Not checked on hardware.** All verification is by test ROMs. The test suites' expected results come
  from their authors' consoles, and in a few places they disagree (see Phase 2).
- **Timing fitted to Gambatte.** The double speed line start, the window's WY checks, STAT and LYC
  changes in double speed, the speed switch's freeze lengths and TIMA's jump ahead were worked out by
  comparing Nubo with Gambatte where Gambatte's tests left gaps. Gambatte's tests were verified on
  hardware, but its behaviour between the tested points may not be.
- **Super Game Boy.** Only the hardware differences are emulated. Command packets, borders, palettes,
  multiplayer and sound functions are not.
- **Link cable.** The serial port always behaves as if no cable is plugged in. Two Nubo instances can't
  be linked, and the Game Boy Printer isn't emulated.
- **Infrared.** The CGB's RP register (0xFF56) isn't emulated. HuC1's infrared port always sees no light.
- **Cartridge types.** ROM only, MBC1, MBC2, MBC3, MBC5 (with rumble) and HuC1 are emulated. MBC30
  (the MBC3 with an 8-bit ROM bank register and 8 RAM banks, used by the Japanese Pokémon Crystal),
  MBC6, MBC7, HuC3, MMM01, TAMA5 and the Pocket Camera are not.
- **Boot ROMs.** Nubo boots with its own boot ROMs (based on Bootix and SameBoy's), which keep the
  timing and end state of the ones they replaced. The original boot ROMs can't be used, so the boot
  sequence itself isn't Nintendo's, and post-boot state comes from calibration, not from running the
  real code.
- **Test suites not run.** Some suites aren't run because nothing checks them automatically or they need
  hardware Nubo doesn't emulate (listed in the README's Test ROMs section).

## Phase 1: measure other emulators on the same tests

Without this, "most accurate" is a guess. It is also cheap and decides how far off the claim is.

1. Build a comparison harness that runs other emulators as separate programs on the same pinned test
   ROMs and models, and judges their output with the test runner's checks (register results, reference
   screenshots, Gambatte's `_out` digits). Where an emulator can't report registers, use its screen,
   as the [GB Emulator Shootout](https://gbdev.io/GBEmulatorShootout/) does. Its harness already
   drives several emulators and may be the easiest starting point.
2. Candidates: [SameBoy](https://github.com/LIJI32/SameBoy) (headless tester), [Gambatte](https://github.com/pokemon-speedrunning/gambatte-core)
   (test runner), [Mesen2](https://github.com/SourMesen/Mesen2), [mGBA](https://github.com/mgba-emu/mgba),
   [GateBoy](https://github.com/aappleby/metroboy) (DMG only, simulated gate by gate), [ares](https://github.com/ares-emulator/ares),
   [Emulicious](https://emulicious.net) and [BGB](https://bgb.bircd.org). Record each one's version.
3. Count a test as passed only on the models it targets. List separately the tests an emulator can't
   run because it doesn't emulate that model, so "fewer models" and "wrong results" don't get mixed up.
4. Measure speed in the same run (the `cargo benchmark` games, emulated seconds per real second), so the
   table also answers what Nubo's accuracy costs.
5. Licensing: run other emulators only as separate programs and never link or copy their code. Gambatte
   is GPL-2.0-only and can't be combined with Nubo's GPL-3.0-or-later. Keep the harness out of the Nubo
   crates (a script folder or its own repository).

Done when: a table per suite and per emulator, with versions, date and the command that made it. Every
suite where another emulator beats Nubo goes into Phase 2.

## Phase 2: fix the remaining failures

From [FAILING_TESTS.md](../frontends/test_rom_runner/FAILING_TESTS.md), in order of how likely a fix is
without hardware:

| Tests | Group | Next step |
|---|---|---|
| 4 | MBC3 | MBC30 (8-bit ROM bank register, 8 RAM banks), invalid banks and the single-latch behaviour. Already in progress separately |
| 9 | Channel 1 duty timing across CGB speed switches | Realign the APU's tick parity at STOP and DIV resets (lead 5.1 in the toolkit's HANDOFF.md) |
| 2 | Mealybug's tile data switch glitch on CGB-C | Find where CGB-C's glitched fetch takes its data from, using SameBoy traces |
| 1 | Tile data switch at line 0 in double speed | Line 0 only; the CPU writes already match Gambatte, so look at the line 0 fetcher start |
| 1 | Window toggled at WX 166 | The window line counter is one line ahead; trace the window's state across the line |
| 3 | AGE spsw-interrupts | No reference emulator passes it; needs a CGB (Phase 3) |
| 2 | Mooneye's MGB madness | MGB only and unexplained by its author; needs an MGB (Phase 3) |
| 9 | DMG palette writes during mode 3 | Mealybug's DMG-blob references and AGE disagree; settle on hardware (Phase 3) |
| 3 | CGB object palette RAM at power-on | Depends on one console's power-on noise; dump several consoles (Phase 3) |
| 1 | AGE oam-write on DMG-C | Its author flags the state it depends on as unexplained; needs a DMG-C (Phase 3) |
| 2 | GBMicrotest | Expected values are wrong; confirm on a DMG and report upstream |

Done when: every failure is fixed, or shown on hardware to be a faulty test and reported to its authors.
A test with a wrong expectation can then be listed as such instead of as a failure.

## Phase 3: check against real hardware

What to buy (rough total €250 to €350 second-hand, prices not verified):

- A **Game Boy Color with a CPU CGB C**, first, because most color tests and Gambatte's results target it.
- A **DMG-01 with a CPU B**. A DMG with a CPU C for AGE's DMG-C test, if one turns up.
- Optional: a later Game Boy Color (CPU CGB D or E), a Game Boy Pocket (MGB), a Game Boy Advance or SP
  (AGB), and a Super Game Boy with a SNES for Phase 4's SGB work.
- A **flash cartridge** to run test ROMs (for example an EverDrive GB X7), or an insideGadgets flash
  cartridge with a GBxCart RW to write it and read saves back.
- A **real MBC3 cartridge with a clock** (Pokémon Gold, Silver or Crystal) and, for MBC30, the Japanese
  Pokémon Crystal. Flash cartridges don't reproduce mapper quirks exactly.
- A **link cable**, and a second console of the same kind for link and infrared tests.
- A **3.5 mm audio cable** to a line-in for sound captures, and phone photos or a capture mod for
  screens. Optional: a logic analyzer on the cartridge bus for timing that no test ROM shows.

Avoid FPGA clones (Analogue Pocket and the like) as references: they are emulators too. Run the
shootout's `which.gb` on each console to confirm its revision.

Work:

1. Run every test Nubo fails, and every test where references disagree, on each console. Keep a photo of
   each result.
2. Run the report-only ROMs (`which.gb`, `rom_and_ram.gb`, [AntonioND's GBC hardware tests](https://github.com/AntonioND/gbc-hw-tests),
   the 144p Test Suite) and record what each console shows, so they can become automatic checks in Phase 5.
3. Re-check the behaviours fitted to Gambatte (listed under "Where Nubo stands"). Write small test ROMs
   that sweep each one cycle by cycle, like Gambatte's `_1`/`_2` pairs, record the hardware results and
   add the ROMs to the test runner as Nubo's own suite.
4. Dump work RAM, VRAM, OAM, HRAM and palette RAM at power-on on each console, to decide how much of
   the power-on pattern Nubo should model.
5. Record sound from tests such as SameSuite's and compare waveforms, including the output's high-pass
   filter.

Done when: a results file in the repository lists each check with console, revision, date and result,
and Nubo matches all of them or the differences are fixed or explained.

## Phase 4: emulate the missing hardware

Largest phase, but each part stands alone:

1. **MBC30** and the remaining MBC3 behaviour (also in Phase 2).
2. **Super Game Boy**: command packets (palettes, attribute maps, borders, multiplayer, masking, sound),
   so SameSuite's `sgb` tests and CasualPokePlayer's `sgb-ext-test` can run.
3. **Link cable**: two linked Nubo instances (in one process first, over a network later), the external
   clock, and the Game Boy Printer.
4. **Infrared**: the CGB's RP register, including what it reads with nothing in front of it, and linking
   two instances.
5. **Cartridge types**: MBC6, MBC7 (tilt sensor and EEPROM, Kirby Tilt 'n' Tumble), HuC3 (clock,
   infrared, speaker), MMM01, TAMA5 and the Pocket Camera (fed with a picture or a webcam). Check the
   cartridge types in the No-Intro game lists to find what else licensed games use.
6. **Original boot ROMs** as an option for players who supply them, so the boot and the post-boot
   state can come from the real code. Nubo's own boot ROMs stay the default.

Done when: every cartridge type and accessory in the licensed library is supported, and the tests that
need them run.

## Phase 5: run every test there is

1. Make the unrun tests checkable: Mealybug's `mbc3_rtc` and Scribbltests' manual ROMs (with hardware
   photos as references), `which.gb` (expecting each model's name), `rom_and_ram.gb`, `open-bus-ss-test`
   (a reference screenshot from hardware), AntonioND's tests (comparing with his recorded results),
   `gb-camera-test` after the camera, and the SGB tests after Phase 4.
2. Look for suites not run yet, such as newer releases of the c-sp test ROM collection and the suites
   listed by gbdev.
3. Add a game list: games and demos known for hard effects (mid-line raster effects, timing-sensitive
   copy protection, unusual mappers), each with a fingerprint and a hardware capture to compare with.

## Phase 6: publish and keep it true

1. Publish the Phase 1 table in the README with its method, versions and date, and rerun it for each
   release.
2. Add Nubo to the GB Emulator Shootout.
3. Report wrong expectations to test suite authors, and publish Nubo's own hardware-verified test ROMs
   (Phase 3) for other emulators to use.
4. Keep `cargo test-roms --verify-boot` and `cargo cpu-tests` running in CI on every push to main
   (once GitHub Actions is enabled), and keep `passing.txt` and FAILING_TESTS.md current.
5. Ask the gbdev community, who have the hardware and the knowledge, to review the results.

## The claim itself

Make it only after Phases 1, 2, 3 and 6. Phases 4 and 5 make it stronger, and without them it needs a
qualifier ("for games that need no Super Game Boy, link cable or special cartridge"). Say exactly what
was measured and link the table, for example:

> Nubo passes N of M tests from K public Game Boy test suites, more than any other emulator we measured
> (versions, date). Every test it fails is explained, and the behaviours tests don't pin down were
> checked on a DMG-01 and a Game Boy Color (CPU CGB C).

GateBoy deserves a note: it simulates the DMG's gates, so on monochrome timing it is a reference in its
own right. If it beats Nubo there, say so.

## Order

1. Phase 1 first: it is cheap and shows how far off the claim is.
2. Phase 2's software fixes at the same time.
3. Phase 3 once the hardware arrives, then the Phase 2 rows that need it.
4. Phases 4 and 5 after that, largest gaps first (MBC30, then link cable and SGB).
5. Phase 6 when the table and the hardware results back the claim.
