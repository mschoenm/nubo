<p align="center">
  <img src="images/icon.png" width="128px" height="128px">
  <br />
  Nubo is a cycle-accurate Game Boy and Game Boy Color emulator written in Rust that runs on the desktop and in the browser.
</p>

## Introduction

Nubo is a cycle-accurate Game Boy emulator written in Rust. It runs as a native desktop app built with [Iced](https://iced.rs), and the same app compiles to WebAssembly to run in the browser.

## Download

| Platform | Download |
| -------- | -------- |
| Windows | [Installer](https://github.com/mschoenm/nubo/releases/latest/download/Nubo-windows-x64-setup.exe) · [Zip](https://github.com/mschoenm/nubo/releases/latest/download/Nubo-windows-x64.zip) (runs without installing) |
| Linux | [Flatpak for x86_64](https://github.com/mschoenm/nubo/releases/latest/download/Nubo-linux-x86_64.flatpak) (most PCs) · [Flatpak for ARM](https://github.com/mschoenm/nubo/releases/latest/download/Nubo-linux-aarch64.flatpak) |
| macOS | Not available yet; build `Nubo.app` with `cargo xtask bundle` (see [Desktop App](#desktop-app)) |
| Browser | [Play in the browser](https://mschoenm.github.io/nubo/) |

These are the files of the [latest release](https://github.com/mschoenm/nubo/releases/latest). On Windows, the files aren't signed yet, so SmartScreen may warn the first time: choose More info and then Run anyway. On Linux, open the `.flatpak` file with your software center to install it, or run `flatpak install --user Nubo-linux-x86_64.flatpak`. The [nightly build](https://github.com/mschoenm/nubo/releases/tag/nightly) has the newest `main` that passed every check.

## Features

- Accurate CPU that passes all [JSON CPU tests](https://github.com/adtennant/GameboyCPUTests)
- Accurate audio emulation, with channel timing ported from [SameBoy](https://github.com/LIJI32/SameBoy)
- Dot-based graphics emulation with a pixel FIFO, so mid-scanline effects and mode 3 timing match the hardware
- Every hardware model from the first Game Boy to the Game Boy Advance, picked automatically per game or by hand
- MBC1, MBC2, MBC3 (including the MBC30 for 4 MB games), MBC5, and HuC1 support, with RTC support for MBC3 cartridges
- Loading ROMs directly from `.zip` files
- A desktop app with:
  - A game library built from your ROM folders (optionally including subfolders), with search, recently played games, and each game's title screen or downloaded box art
  - Save states with pictures, quick save and load, and continuing where you left off
  - Fast forward, fullscreen, screenshots, and volume control
  - Keyboard and controller controls you can change, and menus you can use with a controller
  - GameShark and Game Genie cheats per game
  - Light and dark themes that can follow the system, and gray, green or Pocket colors for Game Boy games
- In-game saves for battery-backed cartridges, kept in the user's data folder (or the browser's local storage)

## Getting Started

The only requirement is a Rust toolchain installed with [rustup](https://rustup.rs). Everything is driven by cargo, on Windows, macOS, and Linux:

```
cargo play                          # Run the desktop app
cargo play path/to/rom.gb           # Run a specific ROM (.gb, .gbc, or a .zip containing one)
cargo play path/to/rom.gbc --cgb    # Run in Game Boy Color mode
cargo play path/to/rom.gb --model mgb  # Run on another model: dmg-0, dmg, mgb, sgb, sgb2, cgb-0 to cgb-e, agb
cargo xtask web                     # Build the app for the browser and serve it at http://127.0.0.1:8080
cargo xtask bundle                  # Build Nubo.app with its icon into target/bundle/ (macOS)
cargo xtask flatpak                 # Build the app as a Flatpak, install it for your user and write a .flatpak file into target/bundle/
cargo xtask windows                 # Build the Windows installer and a zip into target/bundle/ (Windows, needs Inno Setup 6)
cargo screenshot path/to/rom.gb 10  # Run headless for 10 emulated seconds and save a screenshot to target/screenshots/
cargo test                          # Run the test suite
cargo test-roms                     # Download and run the test ROM suites
```

On Linux, audio and controller support need the ALSA and udev development files, for example `sudo apt install libasound2-dev libudev-dev pkg-config` on Debian or Ubuntu.

## Desktop App

The app lives in the frontends/nubo folder. Add your ROM folders under Settings > Library to fill the library, or open a single ROM. Everything it keeps (settings, the library, in-game saves, save states, cheats and pictures) goes in the user's data folder: `~/Library/Application Support/Nubo` on macOS, `%APPDATA%\Nubo` on Windows and `~/.local/share/nubo` (or `$XDG_DATA_HOME/nubo`) on Linux, with settings in `~/.config/nubo` on Linux.

On Windows, `cargo xtask windows` builds `Nubo-<version>-windows-x64-setup.exe` in `target/bundle/`, which installs Nubo for your user (no administrator rights) with a Start menu entry and an uninstaller, and can offer Nubo for opening `.gb` and `.gbc` files, plus a zip of the same files to run Nubo without installing. It needs [Inno Setup 6](https://jrsoftware.org/isinfo.php). Each release has both files (as `Nubo-windows-x64-setup.exe` and `Nubo-windows-x64.zip`), and so does the `nightly` pre-release. They aren't signed yet, so Windows SmartScreen may warn the first time: choose More info and then Run anyway.

On macOS, `cargo xtask bundle` builds `Nubo.app` in `target/bundle/`, with its own icon in the Dock. Drag it to the Applications folder to install it.

On Linux, `cargo xtask flatpak` builds the app as a Flatpak and installs it for your user, to try it in the sandbox that Flathub apps run in. Start it from the app grid or with `flatpak run io.github.mschoenm.nubo`, and remove it with `flatpak uninstall --user io.github.mschoenm.nubo`. It needs flatpak-builder; the first build downloads the freedesktop runtime from Flathub, adding Flathub for your user if it isn't there yet. In the sandbox the app keeps its data in `~/.var/app/io.github.mschoenm.nubo/`, and it can read ROM folders in your home folder and on external drives.

It also writes `target/bundle/Nubo-<version>-linux-<architecture>.flatpak` (each [release](https://github.com/mschoenm/nubo/releases) has these files for x86_64 and ARM as `Nubo-linux-<architecture>.flatpak`, and the `nightly` pre-release has them for the newest `main` that passed every check), a single file to give to others: opening it in a software center such as GNOME Software installs Nubo with its icon and menu entry (or run `flatpak install --user Nubo-<version>-linux-<architecture>.flatpak`), and uninstalling it removes both. The computer needs Flatpak; the file points to Flathub for the runtime, which is downloaded the first time.

The default controls are below; all of them can be changed under Settings > Controls.

| Keyboard Key | Game Boy Button |
| ------------ | --------------- |
| Arrow Keys   | D-Pad           |
| Z            | A               |
| X            | B               |
| Enter        | Start           |
| Space        | Select          |

Esc opens the pause menu, Tab fast forwards while held, F5 and F8 quick save and load, F11 switches to fullscreen and F12 saves a screenshot. On a controller, the D-pad or left stick moves, the right face button (B on Xbox, ○ on PlayStation) is A, the bottom face button (A on Xbox, ✕ on PlayStation) is B, L1 opens the pause menu and R1 fast forwards.

## Web Version

`cargo xtask web` builds the same Iced app for the browser. It can open a ROM and play it; the library, save states, cheats and screenshots need the desktop app. Every push to main publishes it to GitHub Pages.

## Test ROMs

`cargo test-roms` downloads a pinned release of the [Game Boy test ROM collection](https://github.com/c-sp/game-boy-test-roms), plus the test ROMs and reference screenshots of the [GB Emulator Shootout](https://gbdev.io/GBEmulatorShootout/) that the collection doesn't have in the same version, and runs each test ROM on the hardware models it targets: DMG-0, DMG, MGB, SGB and SGB2 (monochrome) and CGB-0 to CGB-E and AGB (color). The Shootout's own tests also run the way the Shootout judges them, on a DMG or a CGB-E with its screenshot comparison, and the runner compares the result with the Shootout's published table: counted on the 264 tests published on 2026-06-04, Nubo passes 258, which would put it first (the leader, DocBoy, passes 255; the other six are three Super Game Boy tests and three that only report information). The tests that currently pass are listed in [tools/nubo-test-rom-runner/passing.txt](tools/nubo-test-rom-runner/passing.txt), and CI fails if any of them stop passing. The tests that still fail, and why, are explained in [tools/nubo-test-rom-runner/FAILING_TESTS.md](tools/nubo-test-rom-runner/FAILING_TESTS.md). What still has to happen before Nubo can claim to be the most accurate Game Boy emulator is planned in [docs/ACCURACY_PLAN.md](docs/ACCURACY_PLAN.md).

| Suite | Monochrome | Color |
| ----- | ---------- | ----- |
| [Blargg's tests](https://github.com/retrio/gb-test-roms) | 7/7 | 7/7 |
| [Mooneye Test Suite](https://github.com/Gekkio/mooneye-test-suite) | 137/138 | 97/97 |
| [Wilbert Pol's Mooneye tests](https://github.com/wilbertpol/mooneye-gb) | 153/154 | 107/107 |
| [dmg-acid2](https://github.com/mattcurrie/dmg-acid2) | 1/1 | 1/1 |
| [cgb-acid2](https://github.com/mattcurrie/cgb-acid2) | | 1/1 |
| [cgb-acid-hell](https://github.com/mattcurrie/cgb-acid-hell) | | 1/1 |
| [Mealybug Tearoom Tests](https://github.com/mattcurrie/mealybug-tearoom-tests) | 24/24 | 69/71 |
| [SameSuite](https://github.com/LIJI32/SameSuite) | | 82/82 |
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
| [GB Emulator Shootout](https://github.com/gbdev/GBEmulatorShootout), with daid's and CasualPokePlayer's tests | 165/165 | 93/93 |

These aren't run, because nothing can check them automatically or they need hardware Nubo doesn't emulate:

- Mealybug Tearoom's `mbc3_rtc` and Scribbltests' `fairylake`, `winpos` and manual `statcount`: they have no reference screenshot.
- The shootout's `which.gb` and `rom_and_ram.gb`: they report what they find instead of passing or failing.
- Wilbert Pol's `logic-analysis` ROMs and Mooneye's `utils`: they are for logic analyzers and hardware dumps.
- The collection's Bully on monochrome models: real DMGs fail it too. (The Shootout runs its own build of Bully on DMG, which Nubo passes.)
- SameSuite's `sgb` tests and CasualPokePlayer's `sgb-ext-test`: Super Game Boy packets aren't emulated.
- CasualPokePlayer's `gb-camera-test` and `open-bus-ss-test`: the first needs the Pocket Camera, the second only has a photo of real hardware to compare with.
- The [144p Test Suite](https://github.com/pinobatch/240p-test-mini) (judged by eye) and [AntonioND's GBC hardware tests](https://github.com/AntonioND/gbc-hw-tests) (results recorded from real consoles, which would need a checker of their own).

Additionally, this emulator passes all [JSON CPU tests](https://github.com/adtennant/GameboyCPUTests). `cargo cpu-tests` downloads them and runs them in under a second, and CI runs them on every pull request and push to main.

## Test Suite

This project holds a fairly extensive test suite, as the bulk of the logic was designed using a TDD approach. There are a lot of tests that exercise CPU opcodes, and basic tests that exercise the GPU. Run `cargo test` to run the test suite.

## Credits

Nubo started as a fork of [Retro Boy](https://github.com/smparsons/retroboy) by Samuel Parsons, who wrote the original emulator, its web frontend and its logo ([play the original](https://smparsons.github.io/retroboy)). Nubo builds on that work.

Much of its accuracy comes from other people's research and code:

- [SameBoy](https://github.com/LIJI32/SameBoy) by Lior Halphon: the APU channel timing, the mode 3 pixel pipeline, the OAM corruption bug, the OAM DMA bus conflicts, the CGB speed switch timing, STOP mode, the PPU register write timing, the serial clock and the MBC3's banks and clock latch are ported from SameBoy, and the Game Boy Color boot ROM is a modified version of SameBoy's (Expat License, see [NOTICE](NOTICE)). SameBoy was also the reference for halt timing, the order of the hardware models, GameShark codes and many smaller hardware quirks.
- [Bootix](https://github.com/Hacktix/Bootix) by Hacktix: the boot ROM of the monochrome models is a modified version of Bootix (CC0).
- [Gambatte](https://github.com/pokemon-speedrunning/gambatte-core) by sinamas: a reference for the timing of double speed and the window, worked out by running the same test ROMs in Gambatte and Nubo and studying where they differ. Gambatte's GPL-2.0-only license can't be combined with Nubo's, so no code comes from it.
- [Pan Docs](https://gbdev.io/pandocs/) and the [gbdev](https://gbdev.io) community: the reference for nearly every hardware register.
- [SonoSooS's Game Boy CPU notes](https://gist.github.com/SonoSooS/c0055300670d678b5ae8433e20bea595): the fetch, decode and execute overlap of the CPU.
- The test ROMs that keep the emulator honest: [Blargg's tests](https://github.com/retrio/gb-test-roms), the [Mooneye Test Suite](https://github.com/Gekkio/mooneye-test-suite) by Joonas Javanainen, [dmg-acid2, cgb-acid2, cgb-acid-hell and the Mealybug Tearoom Tests](https://github.com/mattcurrie) by Matt Currie, [SameSuite](https://github.com/LIJI32/SameSuite) by Lior Halphon, the [Gambatte test suite](https://github.com/pokemon-speedrunning/gambatte-core) by sinamas, [GBMicrotest](https://github.com/aappleby/GBMicrotest) by Austin Appleby, the [AGE test ROMs](https://github.com/c-sp/age-test-roms) by c-sp, [rtc3test](https://github.com/aaaaaa123456789/rtc3test) by ax6, [Bully, Strikethrough and Scribbltests](https://github.com/Hacktix) by Hacktix, Wilbert Pol's [Mooneye tests](https://github.com/wilbertpol/mooneye-gb), [TurtleTests](https://github.com/Powerlated/TurtleTests) by Brian Jia, [little-things-gb](https://github.com/pinobatch/little-things-gb) by Damian Yerrick, the [MBC3 Tester](https://github.com/EricKirschenmann/MBC3-Tester-gb) by Eric Kirschenmann, the [GB Emulator Shootout](https://github.com/gbdev/GBEmulatorShootout) by daid and the gbdev community, with daid's and CasualPokePlayer's tests, the [JSON CPU tests](https://github.com/adtennant/GameboyCPUTests) by adtennant, and the [Game Boy test ROM collection](https://github.com/c-sp/game-boy-test-roms) by c-sp that packages them.
- [Fira Sans](https://github.com/mozilla/Fira): the web version's font, bundled by Iced (SIL Open Font License).
- The [libretro thumbnail collection](https://github.com/libretro-thumbnails/libretro-thumbnails) and the No-Intro game lists of the [libretro database](https://github.com/libretro/libretro-database): the box art the desktop app can download.
- [Lucide](https://lucide.dev): the icons of the desktop app (ISC License, see [NOTICE](NOTICE)).
- [Tobu Tobu Girl Deluxe](https://tangramgames.itch.io/tobu-tobu-girl-deluxe) by Tangram Games: the game in the Flatpak's screenshot (`images/flatpak/`) and in some of the save states every version is checked against (`save-states/`), whose graphics are licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
- [µCity](https://github.com/AntonioND/ucity) and [Geometrix](https://github.com/AntonioND/geometrix) by Antonio Niño Díaz: games in the save states every version is checked against (`save-states/`), licensed under the GPL, version 3 or later.

The original project was built with the help of these resources:

1. [Gameboy CPU Manual](http://marc.rawer.de/Gameboy/Docs/GBCPUman.pdf)
2. [Pan Docs](https://gbdev.io/pandocs/)
3. [Blargg's Test ROM Collection](https://github.com/retrio/gb-test-roms)
4. [Gameboy Doctor](https://github.com/robert/gameboy-doctor)
5. [Imran Nazar's Gameboy Emulator Tutorial](https://imrannazar.com/series/gameboy-emulation-in-javascript)

## License

Nubo is free software, licensed under the [GNU General Public License, version 3](LICENSE) or (at your option) any later version. It started as a fork of RetroBoy, whose code remains available under the [Apache License 2.0](LICENSE-APACHE). The [NOTICE](NOTICE) file lists the third-party code Nubo includes and their licenses. The macOS app and the web version also come with `THIRD_PARTY_LICENSES.txt`, the licenses of the Rust crates Nubo is built from (`cargo xtask licenses <folder>` writes it with the other license files); keep these files with any copy you distribute.

Game Boy and Game Boy Color are trademarks of Nintendo. This project is not affiliated with or endorsed by Nintendo, and it doesn't include any Nintendo software or games.
