# Speed plan

Nubo runs the benchmark games with 13 to 31% fewer instructions than at the start of this work, with
results that are bit for bit the same. Gambatte, the fastest emulator in
[the comparison](EMULATOR_COMPARISON.md), is still ahead: 54 to 92 M instructions per emulated second
against Nubo's 85 to 200 M. This file says what was done, what was learned from other emulators, and the
work left, in the order it should happen. Update it as parts get done.

## The research

Five clean-room reports, in [speed-research/](speed-research), explain how other emulators get their
speed. Each was written by an agent that read the emulator's source; the code in Nubo was written from
the reports alone, as `AGENTS.md` requires for Gambatte. They hold ideas only: no code, tables, constants
or identifiers from the emulators, whose file names appear only in each report's Provenance section.

| Report | Emulators | License | Main lesson for Nubo |
|---|---|---|---|
| [GAMBATTE.md](speed-research/GAMBATTE.md) | Gambatte (Game Boy) | GPL-2.0-only: ideas only | Keep the PPU's timing apart from its pixels; draw whole tiles through lookup tables |
| [MGBA.md](speed-research/MGBA.md) | mGBA (Game Boy, Game Boy Advance) | MPL-2.0 | Nubo already has its scheduling ideas; its edge is a whole-line pixel generator |
| [BSNES_ARES.md](speed-research/BSNES_ARES.md) | bsnes, ares (Super Nintendo) | GPL-3.0, ISC | Split timing from pixels; answer status reads from the predicted timeline |
| [MESEN_NESTOPIA.md](speed-research/MESEN_NESTOPIA.md) | Mesen2, Nestopia UE (NES, Game Boy) | GPL-3.0, GPL-2.0-or-later | Don't catch the PPU up for IF reads; decode each tile row once |
| [SEGA.md](speed-research/SEGA.md) | Genesis Plus GX, BlastEm (Mega Drive) | Non-commercial (ideas only), GPL-3.0 | Tight per-tile loops; keep the next deadline instead of checking again |

All five agree on where the time goes: the PPU took about 60% of Nubo's instructions, mostly in mode 3,
where it stepped the pixel fetcher one dot at a time even for plain tiles. The reports measured Nubo at
commit `3f6e3c7`, before the work below, so their Nubo numbers are the starting point.

## What was done

Each change is a shortcut that must give exactly what running every dot one at a time gives.

| Commit | Change | Instructions saved |
|---|---|---|
| `3c50dcd` | A plain background tile takes the fetcher's eight steps in one go and decodes its pixels through a bit-spreading table. A read of IF no longer makes the PPU catch up while its quiet dots hold no interrupt request at all (`AddressBus::ppu_read_can_wait`), which Nubo's CGB boot ROM does about 500,000 times while it waits for frames | 8 to 18% |
| `cb08bc3` | Runs of plain tiles in one loop, with the tile map row, tile data area and row in the tile kept at hand, and the fetcher's registers written back once at the end | 6 to 14% more |
| `4070596` | The seven dots that shift out a line's dummy pixels at once, and a run of plain tiles that may begin just before the screen (fine scrolling) | 1 to 2% more |

How it was checked, for each commit:

- The fingerprints of every frame, sample and end state, recorded before the work, still match on the
  benchmark games and on all of the roughly 9,000 test ROM runs (`cargo benchmark --compare`).
- 2,000 random scenes of `gpu/tests/random_scenes.rs` matched running dot by dot (a local run; the test
  in CI runs 18). That run caught one bug before it was committed.
- `cargo test-roms` still passes 6,793 of 6,824 runs, 261 of 264 on the Shootout's table.
- The `eager` build takes none of the shortcuts, so matching the old build also means matching it.

## Results

Callgrind, millions of instructions per emulated second, 20 emulated seconds per game and model:

| Game | Model | Before | After | Change |
|---|---|---|---|---|
| µCity | DMG | 123 | 85 | −31% |
| µCity | CGB | 181 | 134 | −26% |
| Tobu Tobu Girl | CGB | 185 | 139 | −25% |
| Tobu Tobu Girl | DMG | 148 | 114 | −23% |
| Geometrix | CGB | 232 | 186 | −20% |
| Geometrix | DMG | 231 | 200 | −13% |

These count instructions, which stay the same from run to run. Real speed hasn't been timed yet.

## The work left

In order: the safe, smaller steps first, the large one last.

1. **Measure real speed.** Run `cargo xtask compare-emulators` and update the tables in
   [EMULATOR_COMPARISON.md](EMULATOR_COMPARISON.md).
2. **Cache each line's objects.** OAM scan runs for every line (3 to 5%). Keep the list of objects per
   line until OAM or the object size changes, and fall back to the scan for anything unusual (OAM DMA,
   the OAM corruption bug, a scan that a write lands in). Low to medium risk.
3. **Fewer slow dots around HBlank and the line start.** About 10 dots per visible line, plus about 260
   per frame in VBlank around OAM DMA and VRAM writes, still go through `Gpu::tick_dot` (2 to 5%).
4. **A leaner CPU read path.** A page table for plain memory (ROM, work RAM, HRAM), with rare checks
   (processor test mode, boot ROM exit, HDMA) moved out of the instruction loop (5 to 15%). Medium risk:
   bank switches, cheats and OAM DMA must turn a page slow whenever they need to.
5. **Answer LY and STAT reads from the predicted timeline** instead of catching the PPU up, and let the
   quiet dots reach the next real STAT edge (5 to 15% on games that poll; more on games with raster
   effects). Medium to high risk; needs the timing model of step 6 for mode 3.
6. **A whole-line renderer** with a model of mode 3's length derived from Nubo's own pipeline, which
   stays the reference and the fallback. The estimates range from 1.3 to 2 times the speed. High risk:
   before it starts, the random scene test should run more cases, for example in the nightly run.

## Not taken, on purpose

- **No frame copy.** It looks like 3 to 5% under Valgrind, which counts each byte of a block copy, but
  costs far less in real time, and swapping the buffers would change what save states hold.
- **Cheaper sound output**, as Gambatte has: it would change Nubo's sound.
- **Shortcuts that give up accuracy**, which every report lists for its emulators: Nubo must stay exact.
- **A JIT** (BlastEm's) or coroutines (bsnes's): they need `unsafe` and don't run in the browser.
