<p align="center">
  <img src="images/icon.png" width="128px" height="128px" alt="Nubo's logo, a cloud">
</p>

<h1 align="center">Nubo</h1>

<p align="center">
  <strong>Game Boy and Game Boy Color games, played exactly as the real hardware plays them.</strong><br>
  On Windows and Linux, in RetroArch, or right in your browser.
</p>

<p align="center">
  <a href="https://github.com/mschoenm/nubo/releases/latest"><img src="https://img.shields.io/github/v/release/mschoenm/nubo?label=download" alt="Latest release"></a>
  <a href="https://mschoenm.github.io/nubo/"><img src="https://img.shields.io/badge/play-in_the_browser-5b8def" alt="Play in the browser"></a>
  <a href="https://github.com/mschoenm/nubo/actions/workflows/ci.yml"><img src="https://github.com/mschoenm/nubo/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/mschoenm/nubo" alt="License: GPL-3.0"></a>
</p>

<p align="center">
  <img src="images/flatpak/tobu-tobu-girl-deluxe.png" width="600" alt="Tobu Tobu Girl Deluxe running in Nubo">
</p>

<p align="center">
  <a href="#download"><strong>Download</strong></a> &nbsp;·&nbsp;
  <a href="https://mschoenm.github.io/nubo/"><strong>Play in your browser</strong></a> &nbsp;·&nbsp;
  <a href="#features"><strong>Features</strong></a> &nbsp;·&nbsp;
  <a href="#getting-started"><strong>Getting started</strong></a>
</p>

Nubo rebuilds the Game Boy one clock cycle at a time: the processor, the picture and the sound all run in step, just as they do inside the console. Games and demos that push the hardware to its limits, changing the picture halfway down the screen or timing their sound to the cycle, look and sound just as they did on the real console. Nubo passes 261 of the 264 tests of the [GB Emulator Shootout](https://gbdev.io/GBEmulatorShootout/), more than any emulator in its published table.

Accuracy is only half of it. Nubo keeps your games in a library with their title screens, saves your progress with a picture of the moment, shows Super Game Boy games in their own colors and borders, and plays just as well with a controller as with a keyboard.

## Features

- **True to the hardware.** The processor, the picture and the sound, emulated cycle by cycle and checked against thousands of test ROMs on every change. [How accurate Nubo is](#accuracy).
- **Every Game Boy there is.** The original Game Boy, the Game Boy Pocket, the Super Game Boy 1 and 2, every revision of the Game Boy Color, and the Game Boy Advance playing Game Boy games. Nubo picks the right one for each game, and you can always choose another.
- **Super Game Boy, as it looked on your TV.** Games made for it start in their own colors and borders, with up to four players on four controllers. Games without a border of their own get Nubo's: leave the controller alone for seven minutes and its clouds start to drift.
- **Your collection at a glance.** Point Nubo at your ROM folders and it builds a library with each game's title screen or box art, a search box, and a Continue playing row with the games you played last.
- **Never lose your progress.** Save states come with a picture, quick save and quick load take one key, and Nubo can open each game right where you stopped. In-game saves are kept just as the cartridge's battery kept them.
- **Play your way.** Keyboard or controller, with every button yours to change and menus you can use from the controller. Fast forward through the slow parts, go fullscreen, take screenshots.
- **Colors you like.** Gray, green or Pocket screens for Game Boy games, Super Game Boy palettes with a color editor, and a light or dark look that can follow your system.
- **Cheats.** GameShark and Game Genie codes, kept for each game.
- **The cartridges nearly every game uses.** MBC1, MBC2, MBC3 with its real-time clock (and the MBC30 of 4 MB games), MBC5, HuC1, and plain ROM with or without RAM. Games load from `.gb`, `.gbc` and `.zip` files.

## Download

| Platform | Download |
| -------- | -------- |
| Windows | [Installer](https://github.com/mschoenm/nubo/releases/latest/download/Nubo-windows-x64.msi) · [Zip](https://github.com/mschoenm/nubo/releases/latest/download/Nubo-windows-x64.zip) (runs without installing) |
| Linux | [Flatpak for x86_64](https://github.com/mschoenm/nubo/releases/latest/download/Nubo-linux-x86_64.flatpak) (most PCs) · [Flatpak for ARM](https://github.com/mschoenm/nubo/releases/latest/download/Nubo-linux-aarch64.flatpak) |
| Browser | [Play in your browser](https://mschoenm.github.io/nubo/), nothing to install |
| RetroArch | The core for Windows, Linux and macOS is in each [release](https://github.com/mschoenm/nubo/releases/latest) ([how to install it](#retroarch-core)) |
| macOS | No download yet: [build it yourself](BUILDING.md#macos) |

- **Windows:** the files aren't signed yet, so SmartScreen may warn you the first time. Choose More info, then Run anyway.
- **Linux:** open the `.flatpak` file with your software center, or run `flatpak install --user Nubo-linux-x86_64.flatpak`. Flatpak fetches the runtime Nubo needs from Flathub the first time.
- **Browser:** plays one game at a time, keeping its in-game saves in the browser. The library, save states, cheats and screenshots are in the desktop app.
- **Want the newest changes?** The [nightly build](https://github.com/mschoenm/nubo/releases/tag/nightly) has everything that passed every check, ahead of the next release.

## Getting Started

Nubo doesn't come with any games. Play the cartridges you own, or start with free homebrew games such as [Tobu Tobu Girl Deluxe](https://tangramgames.itch.io/tobu-tobu-girl-deluxe), [µCity](https://github.com/AntonioND/ucity) or the hundreds on [Homebrew Hub](https://hh.gbdev.io).

Open a single game, or add your ROM folders under Settings > Library and let Nubo fill the library:

<p align="center">
  <img src="images/screenshots/library.png" width="700" alt="Nubo's game library with Geometrix, Tobu Tobu Girl Deluxe and µCity, each shown with its title screen">
</p>

These are the default controls. All of them but Esc can be changed under Settings > Controls.

| Keyboard | Game Boy |
| -------- | -------- |
| Arrow keys | D-pad |
| Z | A |
| X | B |
| Enter | Start |
| Space | Select |

| Keyboard | Does |
| -------- | ---- |
| Esc | Pause menu |
| Tab (hold) | Fast forward |
| F5 / F8 | Quick save / quick load |
| F11 | Fullscreen |
| F12 | Screenshot |

On a controller, the D-pad or left stick moves, the right face button (B on Xbox, ○ on PlayStation) is A, the bottom face button (A on Xbox, ✕ on PlayStation) is B, L1 opens the pause menu and R1 fast forwards.

<details>
<summary><strong>Where your saves are</strong></summary>

Everything Nubo keeps (settings, the library, in-game saves, save states, cheats and their pictures) is in your user data folder:

| System | Folder |
| ------ | ------ |
| Windows | `%APPDATA%\Nubo` |
| macOS | `~/Library/Application Support/Nubo` |
| Linux (Flatpak) | `~/.var/app/io.github.mschoenm.nubo/` |
| Linux (built yourself) | `~/.local/share/nubo` (or `$XDG_DATA_HOME/nubo`), with the settings in `~/.config/nubo` |

Screenshots go to your Pictures folder, in `Pictures/Nubo`.

</details>

<details>
<summary><strong>Installing, updating and removing</strong></summary>

- **Windows:** the installer installs Nubo for your user, without administrator rights, with a Start menu entry; it can also offer Nubo for opening `.gb` and `.gbc` files and add a desktop shortcut. Running a newer installer updates Nubo and keeps those choices. Uninstall Nubo from Settings > Apps, like any app. To install without questions from a script, run `msiexec /i Nubo-windows-x64.msi /qn`, adding `ASSOCIATE_FILES=0` (don't offer Nubo for ROMs) or `DESKTOP_SHORTCUT=1` (add a desktop shortcut); `/passive` shows only a progress bar.
- **Linux:** the Flatpak adds Nubo to your app grid; it can read ROM folders in your home folder and on external drives. Start it from a terminal with `flatpak run io.github.mschoenm.nubo`, and remove it with `flatpak uninstall --user io.github.mschoenm.nubo` or your software center. To update, install the newer file with `--reinstall` added; your saves and settings stay.

</details>

## Not Supported Yet

- A download for macOS (you can [build it](BUILDING.md#macos)), and Android or iOS.
- Linking two games with the link cable.
- Rumble in the desktop app and the browser (the [RetroArch core](#retroarch-core) has it).
- The Pocket Camera, and the cartridges with an MBC6, MBC7 (Kirby Tilt 'n' Tumble), MMM01, HuC3 or TAMA5.

## Accuracy

Every change to Nubo is checked against thousands of test ROMs: small programs that each test one detail of the hardware against what the real consoles do. A change that breaks one doesn't get in.

- **[GB Emulator Shootout](https://gbdev.io/GBEmulatorShootout/):** 261 of its 264 tests pass, counted on the table published on 2026-06-04. That would put Nubo first; the leader there, DocBoy, passes 255.
- **The test ROM suites:** 6,793 of 6,824 test runs pass on the 12 hardware models Nubo emulates, from Blargg's tests, Mooneye, the acid tests, SameSuite and Mealybug Tearoom to Gambatte's thousands.
- **The processor:** every instruction is checked cycle by cycle against the [JSON CPU tests](https://github.com/adtennant/GameboyCPUTests), and all of them pass.
- **Still fast:** being exact costs speed, yet Nubo runs games 50 to 125 times as fast as a real Game Boy on a single core of a modest cloud server, and 3 to 6 times as fast as SameBoy, which also draws the screen dot by dot.

[The results suite by suite](docs/ACCURACY.md), [the tests that still fail and why](tools/nubo-test-rom-runner/FAILING_TESTS.md), and [the speed comparison](docs/EMULATOR_COMPARISON.md).

## RetroArch Core

Already playing in [RetroArch](https://www.retroarch.com/)? Nubo runs there too, with everything RetroArch adds: rewind, run-ahead, netplay, shaders, RetroAchievements and recording. The boot ROMs are built in, so no BIOS files are needed. The core isn't in RetroArch's core downloader yet, so you install it by hand:

1. Download `Nubo-libretro-<system>.zip` for your system from the [latest release](https://github.com/mschoenm/nubo/releases/latest).
2. Copy the core (`nubo_libretro.so`, `.dll` or `.dylib`) into RetroArch's `cores` folder and `nubo_libretro.info` into its `info` folder. Settings > Directory in RetroArch shows where they are.
3. Load a `.gb` or `.gbc` game with Load Core > Nubo.

[More about the core](docs/RETROARCH.md): its options, saves that move between it and the app, netplay with clock games, and the systems it runs on.

## Help and Feedback

Found a game that doesn't play right, or something that could be better? [Open an issue](https://github.com/mschoenm/nubo/issues), with the game's name and, if you chose one, the Game Boy model you played it on.

## Building and Contributing

Nubo is written in Rust. With a [Rust toolchain](https://rustup.rs) installed (plus the ALSA and udev development files on Linux), `cargo play` builds and runs the desktop app, and `cargo xtask web` the browser version. [BUILDING.md](BUILDING.md) has the other commands, the packages, the tests and a map of the code. Pull requests are welcome; [AGENTS.md](AGENTS.md) has the project's rules and the checks to run before you push.

## Credits

Nubo started as a fork of [RetroBoy](https://github.com/smparsons/retroboy) by Samuel Parsons, who wrote the original emulator, its web frontend and its logo ([play the original](https://smparsons.github.io/retroboy)). Nubo builds on that work.

Much of its accuracy comes from other people's research and code:

- [SameBoy](https://github.com/LIJI32/SameBoy) by Lior Halphon: the APU channel timing, the mode 3 pixel pipeline, the OAM corruption bug, the OAM DMA bus conflicts, the CGB speed switch timing, STOP mode, the PPU register write timing, the serial clock, the MBC3's banks and clock latch and the Super Game Boy's commands, colors and borders are ported from SameBoy, and the Game Boy Color boot ROM is a modified version of SameBoy's (Expat License, see [NOTICE](NOTICE)). SameBoy was also the reference for halt timing, the order of the hardware models, GameShark codes and many smaller hardware quirks.
- [Bootix](https://github.com/Hacktix/Bootix) by Hacktix: the boot ROM of the monochrome models is a modified version of Bootix (CC0).
- [Gambatte](https://github.com/pokemon-speedrunning/gambatte-core) by sinamas: a reference for the timing of double speed and the window, worked out by running the same test ROMs in Gambatte and Nubo and studying where they differ. Gambatte's GPL-2.0-only license can't be combined with Nubo's, so no code comes from it.
- [Pan Docs](https://gbdev.io/pandocs/) and the [gbdev](https://gbdev.io) community: the reference for nearly every hardware register.
- [SonoSooS's Game Boy CPU notes](https://gist.github.com/SonoSooS/c0055300670d678b5ae8433e20bea595): the fetch, decode and execute overlap of the CPU.
- The authors of the test ROMs that keep the emulator honest, linked in [the results table](docs/ACCURACY.md#test-roms): Blargg, Joonas Javanainen (Mooneye), Wilbert Pol, Matt Currie (the acid tests and Mealybug Tearoom), Lior Halphon (SameSuite), sinamas (Gambatte), Austin Appleby (GBMicrotest), c-sp (AGE, and the [Game Boy test ROM collection](https://github.com/c-sp/game-boy-test-roms) that packages them), ax6 (rtc3test), Hacktix (Bully, Strikethrough and Scribbltests), Brian Jia (TurtleTests), Damian Yerrick (little-things-gb), Eric Kirschenmann (MBC3 Tester), daid, CasualPokePlayer and the gbdev community (the GB Emulator Shootout), and adtennant (the [JSON CPU tests](https://github.com/adtennant/GameboyCPUTests)).
- [Fira Sans](https://github.com/mozilla/Fira): the web version's font, bundled by Iced (SIL Open Font License).
- The [libretro thumbnail collection](https://github.com/libretro-thumbnails/libretro-thumbnails) and the No-Intro game lists of the [libretro database](https://github.com/libretro/libretro-database): the box art the desktop app can download.
- [Lucide](https://lucide.dev): the icons of the desktop app (ISC License, see [NOTICE](NOTICE)).
- [libretro.h](https://github.com/libretro/libretro-common) by the RetroArch team: the libretro API the RetroArch core's bindings are written from (MIT License, see [NOTICE](NOTICE)). [SameBoy's libretro core](https://github.com/LIJI32/SameBoy) and [rcheevos](https://github.com/RetroAchievements/rcheevos) showed how a Game Boy core lays out its memory for RetroAchievements.
- [Tobu Tobu Girl Deluxe](https://tangramgames.itch.io/tobu-tobu-girl-deluxe) by Tangram Games: the game in the screenshots (`images/`) and in some of the save states every version is checked against (`save-states/`), whose graphics are licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
- [µCity](https://github.com/AntonioND/ucity) and [Geometrix](https://github.com/AntonioND/geometrix) by Antonio Niño Díaz: games in the library screenshot (`images/screenshots/`) and in the save states every version is checked against (`save-states/`), licensed under the GPL, version 3 or later.

The original project was built with the help of these resources:

1. [Gameboy CPU Manual](http://marc.rawer.de/Gameboy/Docs/GBCPUman.pdf)
2. [Pan Docs](https://gbdev.io/pandocs/)
3. [Blargg's Test ROM Collection](https://github.com/retrio/gb-test-roms)
4. [Gameboy Doctor](https://github.com/robert/gameboy-doctor)
5. [Imran Nazar's Gameboy Emulator Tutorial](https://imrannazar.com/series/gameboy-emulation-in-javascript)

## License

Nubo is free software, licensed under the [GNU General Public License, version 3](LICENSE) or (at your option) any later version. It started as a fork of RetroBoy, whose code remains available under the [Apache License 2.0](LICENSE-APACHE). The [NOTICE](NOTICE) file lists the third-party code Nubo includes and their licenses. The packages also come with `THIRD_PARTY_LICENSES.txt`, the licenses of the Rust crates Nubo is built from; keep these files with any copy you distribute.

Game Boy and Game Boy Color are trademarks of Nintendo. This project is not affiliated with or endorsed by Nintendo, and it doesn't include any Nintendo software or games.
