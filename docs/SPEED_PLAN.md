# Speed plan

Nubo runs the benchmark games with 16 to 33% fewer instructions than at the start of this work, with
results that are bit for bit the same. Gambatte, the fastest emulator in
[the comparison](EMULATOR_COMPARISON.md), is still ahead: 54 to 92 M instructions per emulated second
against Nubo's 83 to 193 M, and 2.2 to 3.9 times Nubo's speed, down from 3 to 4.3 times. The small, safe
steps are used up; what is left are two larger changes, to be tried as separate prototypes. This file
says what was done, what was learned from other emulators, what was tried without gain, and the work
left. Update it as parts get done.

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

| Pull request | Change | Instructions saved |
|---|---|---|
| #95 | A plain background tile takes the fetcher's eight steps in one go and decodes its pixels through a bit-spreading table. A read of IF no longer makes the PPU catch up while its quiet dots hold no interrupt request at all (`AddressBus::ppu_read_can_wait`), which Nubo's CGB boot ROM does about 500,000 times while it waits for frames | 8 to 18% |
| #95 | Runs of plain tiles in one loop, with the tile map row, tile data area and row in the tile kept at hand, and the fetcher's registers written back once at the end | 6 to 14% more |
| #95 | The seven dots that shift out a line's dummy pixels at once, and a run of plain tiles that may begin just before the screen (fine scrolling) | 1 to 2% more |
| next | Plain CPU reads of ROM, work RAM and HRAM take a short inlined path (`AddressBus::cpu_read_plain_byte`), everything else out of line | 2 to 3% more |
| next | The bulk OAM scan stops at ten objects, checks the y range with one comparison and sets its bus latches once | about 1.5% more |

How it was checked, for each commit:

- The fingerprints of every frame, sample and end state, recorded before the work, still match on the
  benchmark games and on all of the roughly 9,000 test ROM runs (`cargo benchmark --compare`).
- 2,000 random scenes of `gpu/tests/random_scenes.rs` matched running dot by dot (a local run; the test
  in CI runs 18). That run caught one bug before it was committed. The nightly run now tries 10,000
  scenes with a new seed each night (`NUBO_RANDOM_SCENES`, `NUBO_RANDOM_SEED`).
- `cargo test-roms` still passes 6,793 of 6,824 runs, 261 of 264 on the Shootout's table.
- The `eager` build takes none of the shortcuts, so matching the old build also means matching it.

## Results

Callgrind, millions of instructions per emulated second, 20 emulated seconds per game and model:

| Game | Model | Before | After | Change |
|---|---|---|---|---|
| µCity | DMG | 123 | 83 | −33% |
| µCity | CGB | 181 | 129 | −29% |
| Tobu Tobu Girl | CGB | 185 | 134 | −28% |
| Tobu Tobu Girl | DMG | 148 | 110 | −26% |
| Geometrix | CGB | 232 | 179 | −23% |
| Geometrix | DMG | 231 | 193 | −16% |

In time, best of 3 rounds of 30 s with `cargo xtask compare-emulators` (the tables are in
[EMULATOR_COMPARISON.md](EMULATOR_COMPARISON.md)), the gap to Gambatte shrank from 3 to 4.3 times
(2026-10-13) to 2.2 to 3.9 times (2026-10-15). Nubo now runs as fast as mGBA or faster on four of the six
runs. Times vary by up to 50% on shared machines, so the gap to the other emulators, measured in the
same run, is what to compare.

## Tried, without gain

Each of these kept every result the same but saved nothing measurable (under 1%, or cost more), so
none was kept:

- **A page table for plain CPU reads** (one entry per 4 KiB page: ROM or work RAM at an offset, or the
  long way), rebuilt on bank, SVBK and boot ROM changes. Once the read path is inlined, working out
  where an address goes costs only a few instructions, and the table's own checks cost as much.
- **One CPU run loop** with `Cpu::step` inlined into it and a stop condition from data, to pay the
  call into `step` once per call instead of once per instruction.
- **Inlining the check for M-cycles waiting to run** before accesses that aren't plain.
- **Asking whether the PPU's shape is steady less often**, running the dots that can't be steady (the
  start of a line, HBlank's delays, the countdowns) one by one without asking before each.
- **Skipping idle countdowns** in `Gpu::tick_dot`.

## The work left

Both are large and carry risk, so each is to be tried as a separate prototype first.

1. **A whole-line renderer** with a model of mode 3's length derived from Nubo's own pipeline, which
   stays the reference and the fallback. The estimates range from 1.3 to 2 times the speed. High risk:
   the model must give mode 3's exact length for every arrangement of objects, the window and fine
   scrolling, and every PPU quirk must either be handled in both renderers or send the line to the
   reference one.
2. **Answer LY and STAT reads from the predicted timeline** instead of catching the PPU up, and let the
   quiet dots reach the next real STAT edge (5 to 15% on games that poll; more on games with raster
   effects). Medium to high risk; needs the timing model of the line renderer for mode 3.

A smaller one, if those wait: **sort the objects into the lines they cover once per OAM change**
instead of scanning OAM on every line (about 3%). Most games rewrite OAM by DMA every frame, so this
only pays as one pass per frame instead of one per line, and it must stay exact when OAM changes in the
middle of a frame.

## Not taken, on purpose

- **No frame copy.** It looks like 3 to 5% under Valgrind, which counts each byte of a block copy, but
  costs far less in real time, and swapping the buffers would change what save states hold.
- **Cheaper sound output**, as Gambatte has: it would change Nubo's sound.
- **Shortcuts that give up accuracy**, which every report lists for its emulators: Nubo must stay exact.
- **A JIT** (BlastEm's) or coroutines (bsnes's): they need `unsafe` and don't run in the browser.
