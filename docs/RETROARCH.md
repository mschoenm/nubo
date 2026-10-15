# Nubo in RetroArch

Nubo also runs in [RetroArch](https://www.retroarch.com/) and other frontends of the libretro interface. RetroArch then brings what it has for every core: save state slots, rewind, run-ahead, netplay, shaders, RetroAchievements, its cheat list, recording and button remapping.

## Installing

The core is experimental and not in RetroArch's own core downloader yet, so it is installed by hand:

1. Download the zip for your system (`Nubo-libretro-<system>.zip`) from a [release](https://github.com/mschoenm/nubo/releases/latest) or the [nightly build](https://github.com/mschoenm/nubo/releases/tag/nightly).
2. Copy `nubo_libretro.so` (Linux), `nubo_libretro.dll` (Windows) or `nubo_libretro.dylib` (macOS) into RetroArch's `cores` folder, and `nubo_libretro.info` into its `info` folder. Settings > Directory in RetroArch shows where they are. Without the info file RetroArch doesn't know which games the core runs, and turns off rewind and run-ahead for it.
3. Load a `.gb` or `.gbc` game with Load Core > Nubo, or pick "Nintendo - Game Boy / Color (Nubo)" for a playlist.

The Linux cores need glibc 2.34 or newer (Ubuntu 22.04, Debian 12 or later); the macOS core runs on Apple silicon and Intel Macs. There is no Android or iOS core yet.

## What It Does

- **Games:** every cartridge the app runs, with the boot ROMs built in, so no BIOS files are needed. The core options pick the model (automatic, or any model from DMG-0 to AGB), the colors of Game Boy games (gray, green or Pocket), the colors and border of Super Game Boy games (the game's own, or a palette and Nubo's border or none, as the Super Game Boy's menu picks them; Nubo's border shows a night or a day sky by the computer's clock) and the cartridge clock; the model and the clock take effect when the game is restarted.
- **Saves:** RetroArch keeps in-game saves as `.srm` and `.rtc` files. The `.srm` is the same raw cartridge RAM as the app's `.sav` file, and the `.rtc` the same clock file, so saves move between the app and RetroArch by renaming them.
- **Save states, rewind, run-ahead and netplay**, with states of one fixed size. A state saved with the cartridge clock on real time goes on with the time it is loaded at; for netplay with games that have a clock, such as Pokémon Gold, set the Cartridge clock option to Emulated time on every player's side. The Super Game Boy colors and border options are the player's own: loading a state keeps them, and each netplay player sees their own.
- **Cheats:** RetroArch's cheat list, with Game Genie and GameShark codes, several in one cheat joined with `+`, and cheats that RetroArch applies itself by writing to memory (its cheat files' RetroArch handler and its cheat search).
- **RetroAchievements:** the memory maps achievements read, laid out as RetroAchievements expects a Game Boy and a Game Boy Color. RetroArch's own memory tools see the same memory, and what they write reaches the game: work RAM, high RAM, video RAM, sprite memory and cartridge RAM (a change to battery-backed RAM is saved too). The hardware registers are read-only.
- **Rumble** on rumble cartridges such as Pokémon Pinball.
