# How Accurate Nubo Is

Nubo is checked against thousands of test ROMs: small programs that each test one detail of the hardware against what real consoles do. Every pull request runs them all, and fails if a test that passed stops passing. What still has to happen before Nubo can claim to be the most accurate Game Boy emulator is planned in [ACCURACY_PLAN.md](ACCURACY_PLAN.md).

- **The processor:** Nubo passes all [JSON CPU tests](https://github.com/adtennant/GameboyCPUTests), which check every instruction cycle by cycle.
- **Speed:** being exact costs speed, but on one core of a shared 2.1 GHz cloud server Nubo still runs games about 50 to 155 times as fast as a real Game Boy, and 3 to 8 times as fast as SameBoy, which also draws the screen dot by dot ([the comparison](EMULATOR_COMPARISON.md)).

## Test ROMs

`cargo test-roms` downloads a pinned release of the [Game Boy test ROM collection](https://github.com/c-sp/game-boy-test-roms), plus the test ROMs and reference screenshots of the [GB Emulator Shootout](https://gbdev.io/GBEmulatorShootout/) that the collection doesn't have in the same version, and runs each test ROM on the hardware models it targets: DMG-0, DMG, MGB, SGB and SGB2 (monochrome) and CGB-0 to CGB-E and AGB (color). The Shootout's own tests also run the way the Shootout judges them, on a DMG, an SGB or a CGB-E with its screenshot comparison, and the runner compares the result with the Shootout's published table. Like the other emulators in the Shootout, Nubo is judged on its own picture: its harness starts the app with `--raw-screen`, which turns off the color correction players get by default, as it has GSE, Emulicious and others turn off theirs. The tests that pass are listed in [passing.txt](../tools/nubo-test-rom-runner/passing.txt), and the ones that still fail, and why, in [FAILING_TESTS.md](../tools/nubo-test-rom-runner/FAILING_TESTS.md).

| Suite | Monochrome | Color |
| ----- | ---------- | ----- |
| [Blargg's tests](https://github.com/retrio/gb-test-roms) | 7/7 | 7/7 |
| [Mooneye Test Suite](https://github.com/Gekkio/mooneye-test-suite) | 137/138 | 97/97 |
| [Wilbert Pol's Mooneye tests](https://github.com/wilbertpol/mooneye-gb) | 153/154 | 107/107 |
| [dmg-acid2](https://github.com/mattcurrie/dmg-acid2) | 1/1 | 1/1 |
| [cgb-acid2](https://github.com/mattcurrie/cgb-acid2) | | 1/1 |
| [cgb-acid-hell](https://github.com/mattcurrie/cgb-acid-hell) | | 1/1 |
| [Mealybug Tearoom Tests](https://github.com/mattcurrie/mealybug-tearoom-tests) | 24/24 | 69/71 |
| [SameSuite](https://github.com/LIJI32/SameSuite) | 4/4 | 82/82 |
| [Gambatte](https://github.com/pokemon-speedrunning/gambatte-core) | 1865/1873 | 3340/3352 |
| [GBMicrotest](https://github.com/aappleby/GBMicrotest) | 480/482 | |
| [AGE](https://github.com/c-sp/age-test-roms) | 12/14 | 102/105 |
| [rtc3test](https://github.com/aaaaaa123456789/rtc3test) | 3/3 | 3/3 |
| [Bully](https://github.com/Hacktix/BullyGB) | | 1/1 |
| [Strikethrough](https://github.com/Hacktix/strikethrough.gb) | 1/1 | 1/1 |
| [Scribbltests](https://github.com/Hacktix/scribbltests) | 5/5 | 5/5 |
| [TurtleTests](https://github.com/Powerlated/TurtleTests) | 2/2 | 2/2 |
| [little-things-gb](https://github.com/pinobatch/little-things-gb) | 2/2 | 2/2 |
| [MBC3 Tester](https://github.com/EricKirschenmann/MBC3-Tester-gb) | 1/1 | 1/1 |
| [daid's tests](https://github.com/gbdev/GBEmulatorShootout/tree/main/testroms/daid) | 2/2 | 6/6 |
| [CasualPokePlayer's MBC3 and Super Game Boy tests](https://github.com/gbdev/GBEmulatorShootout/tree/main/testroms/cpp) | 5/5 | |
| [GB Emulator Shootout](https://github.com/gbdev/GBEmulatorShootout), judged as the Shootout judges them | 168/168 | 93/93 |

## Tests That Aren't Run

These aren't run, because nothing can check them automatically or they need hardware Nubo doesn't emulate:

- Mealybug Tearoom's `mbc3_rtc` and Scribbltests' `fairylake`, `winpos` and manual `statcount`: they have no reference screenshot.
- The shootout's `which.gb` and `rom_and_ram.gb`: they report what they find instead of passing or failing.
- Wilbert Pol's `logic-analysis` ROMs and Mooneye's `utils`: they are for logic analyzers and hardware dumps.
- The collection's Bully on monochrome models: real DMGs fail it too. (The Shootout runs its own build of Bully on DMG, which Nubo passes.)
- CasualPokePlayer's `gb-camera-test` and `open-bus-ss-test`: the first needs the Pocket Camera, the second only has a photo of real hardware to compare with.
- The [144p Test Suite](https://github.com/pinobatch/240p-test-mini) (judged by eye) and [AntonioND's GBC hardware tests](https://github.com/AntonioND/gbc-hw-tests) (results recorded from real consoles, which would need a checker of their own).
