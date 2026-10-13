# Speed next to other emulators

`cargo xtask compare-emulators` measures how fast Nubo runs games next to other Game Boy emulators: [SameBoy](https://github.com/LIJI32/SameBoy), [Gambatte](https://github.com/pokemon-speedrunning/gambatte-core) (gambatte-core), [mGBA](https://github.com/mgba-emu/mgba), [binjgb](https://github.com/binji/binjgb) and [Gearboy](https://github.com/drhelius/Gearboy). It fetches each of them at a pinned commit into `target/emulator-comparison`, builds it with the compiler flags of its own release builds, and runs it through a small harness (`xtask/emulator_harnesses/`) that does exactly what nubo-benchmark does for Nubo.

```bash
cargo xtask compare-emulators                       # The three benchmark games, DMG and CGB, best of 3 rounds of 30 s
cargo xtask compare-emulators ~/Roms/GameBoy        # Other games (files or folders of .gb and .gbc files)
cargo xtask compare-emulators --emulators nubo,sameboy,gambatte --models cgb --rounds 5 --seconds 60
cargo xtask compare-emulators --instructions        # Count instructions with Valgrind instead of timing (Linux)
cargo xtask compare-emulators --screenshots         # Also keep each run's last frame, to check every emulator got as far
```

The first run takes a few minutes to fetch and build everything; later runs reuse the builds until a pinned commit, a harness or the build steps change.

## What it needs

Everything else in the workspace needs only Rust. This task builds other emulators from their C and C++ sources, so it also needs those tools; an emulator whose tools are missing is left out with a note saying what to install, and Nubo always runs. It is written for Linux and macOS but has only been tried on Linux so far; on Windows, use WSL.

| Emulator | Needs |
|---|---|
| Nubo | Rust |
| SameBoy | git, make, a C compiler (clang if there is one), [RGBDS](https://rgbds.gbdev.io) for its boot ROMs |
| Gambatte | git, a C++ compiler, and what SameBoy's boot ROMs need |
| mGBA | git, CMake, make, a C compiler |
| binjgb | git, a C compiler |
| Gearboy | git, a C and C++ compiler, and what SameBoy's boot ROMs need |
| `--instructions` | [Valgrind](https://valgrind.org) (Linux) |

## What is measured

Every emulator runs each ROM the way nubo-benchmark does (`harness.h`): from power-on for the given emulated time, taking 44,100 Hz stereo sound in buffers of 512 samples, holding nubo-benchmark's scripted buttons (Start a few times, then running right while jumping) decided once per buffer, and drawing every frame. Only the time spent producing the buffers counts, in Nubo's `step_until_next_audio_buffer` and in each harness's `run_buffer`. Speed is emulated time divided by that time.

The runs go one at a time, round after round, and the table shows each run's best round, since a busy machine only slows a run down. Times still vary between machines and from run to run on shared ones (up to 50% on a cloud machine), so compare the emulators with each other rather than with numbers from another machine.

`--instructions` counts, with Valgrind's callgrind, the instructions each emulator runs inside those same functions, per emulated second. The count is the same however busy the machine is and takes about 50 times longer to measure, so the runs are 10 s by default and run in parallel. It is a second opinion, not a fairer one: an emulator can run more instructions and still be faster when they are cheaper (fewer cache misses and mispredicted branches), so time is what players feel.

## How each emulator is set up

| | Built with | Boot ROM | Models | Sound |
|---|---|---|---|---|
| Nubo | `cargo build --release` | Nubo's own | DMG, CGB-E | 44,100 Hz, band-limited |
| Nubo eager | the same with `--features nubo-core/eager`: every M-cycle on its own, without the lazy shortcuts | Nubo's own | DMG, CGB-E | the same |
| SameBoy | its Makefile with `CONF=release` (`-O3 -ffast-math -flto`) | SameBoy's | DMG-B, CGB-E | 44,100 Hz with its accurate high-pass filter |
| Gambatte | the flags of its SConstruct (`-O2 -fomit-frame-pointer`) | SameBoy's | DMG, CGB | 2,097,152 Hz turned into 44,100 by its cheapest resampler ("Fast") |
| mGBA | CMake Release (`-O3 -flto`), GB core only | none: it only runs boot ROMs with a known checksum | DMG, CGB | 131,072 Hz turned into 44,100 by its resampler with the cosine interpolator |
| binjgb | its CMake Release flags (`-O3`) | none: it can't run one | DMG, CGB | 44,100 Hz, 8-bit |
| Gearboy | the flags of its libretro core (`-O3 -flto`) | SameBoy's | DMG, CGB | 44,100 Hz, band-limited (Blargg's Gb_Snd_Emu) |

What to keep in mind when reading the results:

- **Resampling.** Gambatte and mGBA make sound at a high rate and leave turning it into 44,100 Hz to the frontend, which the harnesses do inside the timed function with the cheapest resampler each has. mGBA's frontends use a windowed sinc by default, which takes more time than the emulation itself (it roughly doubles mGBA's instruction count).
- **No boot ROM.** mGBA and binjgb start in the game's post-boot state, so they skip the boot animation (about 1.5 s on DMG and 3 s on CGB) the others emulate, and their games are a little further along when the scripted buttons come.
- **Different designs.** Gearboy and binjgb don't draw dot by dot with a pixel FIFO like Nubo and SameBoy, which is less work but less exact. Gambatte works out when things will happen instead of stepping through every cycle, which Nubo's lazy shortcuts do in part. The comparison measures speed only; how accurately each emulator runs games is a different question (see `docs/ACCURACY_PLAN.md`).
- **Light games.** The benchmark games spend much of the run on title screens and menus (the button script never gets Tobu Tobu Girl past its title screen, and µCity on DMG only shows that it needs a Game Boy Color). Quiet screens favour emulators that skip ahead, like Nubo and Gambatte. Run it on other games for busier scenes.

Gearboy and binjgb can't put a cartridge without Game Boy Color support in CGB mode, so they show a dash for those on CGB.

The other emulators are only built and run on the machine that runs the comparison, never distributed with Nubo, and no code from them is used in Nubo (Gambatte is GPL-2.0-only, so it may only be a reference).

## Results

On a shared cloud machine (4 cores of an Intel Xeon at 2.1 GHz) on 2026-10-13, with the commits pinned in `compare_emulators.rs`:

| Best of 3 rounds of 30 s | Nubo | Nubo eager | SameBoy | Gambatte | mGBA | binjgb | Gearboy |
|---|---|---|---|---|---|---|---|
| Geometrix DMG | 47.7x | 10.8x | 17.3x | 206.7x | 81.1x | 93.3x | 67.8x |
| Geometrix CGB | 56.4x | 12.9x | 20.6x | 190.3x | 72.2x | 84.0x | 61.4x |
| Tobu Tobu Girl DMG | 80.3x | 10.8x | 18.1x | 309.2x | 108.9x | 82.3x | 58.2x |
| Tobu Tobu Girl CGB | 69.5x | 13.3x | 22.7x | 242.9x | 90.9x | 76.7x | 61.1x |
| µCity DMG | 126.0x | 12.2x | 20.4x | 408.1x | 123.9x | 95.3x | 78.8x |
| µCity CGB | 75.7x | 8.7x | 13.9x | 228.1x | 108.3x | 75.3x | 43.6x |

Nubo ran 2.7 to 6.2 times as fast as SameBoy, all of it from its lazy shortcuts: running every M-cycle on its own (Nubo eager), it is about 0.6 times as fast. Gambatte was 3 to 4.3 times as fast as Nubo, mGBA 1 to 1.7 times, binjgb 0.8 to 2 times and Gearboy 0.6 to 1.4 times.
