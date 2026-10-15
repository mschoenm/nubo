# How mGBA gets its speed, and what Nubo can take from it

Clean-room report: ideas and techniques only, no code. mGBA is MPL-2.0; Nubo re-implements whatever it adopts in its own way. Commit and file list are under Provenance at the end.

## 0. Where Nubo spends its time today

I profiled Nubo's release build with debug line tables (callgrind, 10 emulated seconds, the three benchmark games, the same harness as `cargo benchmark`). Instruction counts per emulated second matched the October 13 measurements in `docs/EMULATOR_COMPARISON.md` (126 to 242 million). The share of each part:

| Run | M instr / emulated s | PPU (`Gpu::run_dots`, inclusive) | of which mode 3 plain path | of which dot-by-dot pipeline | APU catch-up | finished-frame copy |
|---|---|---|---|---|---|---|
| Tobu Tobu Girl DMG | 139 | 78% | 35% | 16% | 5.5% | 4% |
| Tobu Tobu Girl CGB | 209 | 62% | 26% | 13% | 3% | 2.7% |
| Geometrix DMG | 197 | 65% | 28% | 14% | about 3% | 2.9% |
| Geometrix CGB | 243 | 59% | 24% | 12% | 2.5% | 2.3% |
| µCity DMG | 126 | 89% | 40% | 18% | 4.6% | 4.4% |
| µCity CGB | 216 | 63% | 26% | 13% | 2.7% | 2.6% |

What these numbers say:

- **The gap with mGBA is the PPU, not the scheduler or the CPU.** Nubo's PPU alone runs about 100 to 145 million instructions per emulated second. That is roughly what mGBA's whole GB core runs (92 to 126 million). Nubo's CPU, bus and lazy bookkeeping together come to about 10 to 25%, and the APU to 3 to 6%.
- Nubo's PPU costs on the order of 70 to 100 instructions per visible pixel. A scanline-style tile loop like mGBA's costs a few instructions per pixel plus a few dozen per tile.
- In µCity CGB the CPU reads PPU state about 55,000 times per emulated second (about six times per line, probably polling STAT or LY), and each read catches the PPU up. Most of what those catch-ups cost is rendering that has to happen anyway. Still, each one also has a fixed cost, and it cuts mode 3 into small pieces.
- At every VBlank, Nubo copies the whole 92 KB finished frame (`completed_frame.copy_from_slice`). That is 2 to 4% of all instructions.
- The BLEP table is built once (a one-time 20 million instructions, counted inside the timed function on the first call). It doesn't matter for speed.

Nubo already has mGBA's main structural ideas: lazy components that catch up on demand, bulk plain M-cycles, skipping ahead while halted, timers run in bulk, palettes converted to RGBA when they are written, and tile-at-a-time background drawing. What mGBA has that Nubo lacks is a **pixel generator separate from the PPU's timing**, which draws a whole line (or range) in one tight loop. mGBA pays for this with inexact GB timing. The point of this report is how Nubo can get that cheap pixel path while keeping its dot-exact timing as the reference.

## 1. Summary: techniques ranked by estimated impact for Nubo

All estimates are rough, worked out from the profile above. "Exact" means the technique can meet Nubo's bit-for-bit rule against the `eager` build if done as described.

1. **A. A whole-line fast path for mode 3, behind an exact timing model** (exact). Decide once per line whether it is "plain". If it is, work out mode 3's length and the end state of the pipeline in closed form, and draw the 160 pixels with a tight tile loop. The dot pipeline stays as the fallback and the reference. Estimated 1.3 to 1.8 times faster overall on these games, and more once lines with objects are covered (A2).
2. **B. Validity windows for PPU-visible values** (exact). The PPU publishes "LY/STAT/VRAM access/IF bits stay like this until dot D". CPU reads and VRAM/OAM/palette writes before D are then served without a catch-up. This cuts the fixed cost of catch-ups (about 3 to 10% on polling games) and keeps mode 3 whole so A can apply. It is mGBA's "registers are kept current by events" idea, made exact.
3. **C. Dirty-line skipping** (exact). Don't recompose a line whose inputs (VRAM, OAM and palette generations, plus the registers latched for that line) equal those of the pixels already in the buffer. The timing still runs. Large on static screens (title screens, menus, pause), nothing while scrolling. Needs A's separation of pixels from timing.
4. **D. No full-frame copy at VBlank** (exact with care). Swap buffers, or copy only the rows that changed (combine with C). Saves 2 to 4%. Save states store the frame in progress, so it needs care (see section D).
5. **E. Exact skipping of polling loops** (exact variant of mGBA's GBA idle-loop detection). Detect a loop that only reads values with known validity windows. Then run whole iterations in bulk up to the earliest moment a read value can change. Estimated 5 to 20% on games that wait for VBlank by polling instead of HALT, nothing on games that halt.
6. **J. A "frame not shown" mode** (exact for emulation). Skip pixel composition for frames the frontend will throw away, such as RetroArch run-ahead's hidden frames and fast-forward, while the timing runs. Needs A or C's separation. The exception is a Super Game Boy transfer in progress, which reads the screen.
7. **F. A cached earliest-deadline scheduler** (exact). Keep each component's next deadline as an absolute time, recomputed only when that component changes, instead of asking every component on each non-plain M-cycle. 1 to 3%.
8. **G. An instruction-fetch fast path ("active region")** (exact). Cache where the program counter's memory lives (the current ROM bank's offset and its end) and fetch through it, refreshing only on jumps out of the window, bank writes, boot ROM exit and cheat changes. 1 to 3%.

Not recommended, because they trade accuracy for speed (details in section 5): mGBA's approximate mode 3 length, its four-events-per-line GB PPU, its GBA idle-loop "halt until the next event", fixed-rate point sampling of the APU with averaged noise, and frame skipping as dropped frames.

No technique here needs a JIT. mGBA has none either (both of its CPU cores are interpreters), so everything fits `#![forbid(unsafe_code)]` and wasm32.

---

## 2. Techniques in detail

### A. A whole-line mode 3 fast path behind an exact timing model

**What mGBA does (concept).** mGBA's GB PPU has two halves:

- **Timing.** A few scheduled events per line: OAM scan ends, drawing ends, HBlank ends, and frame events. They update LY, STAT, the interrupt flags and the mode, and start HBlank DMA.
- **Pixels.** A renderer that, given a line and a horizontal range, draws that range straight from VRAM, OAM, palettes and registers, in tight loops: one tile-map lookup and two bitplane reads per tile, then a whole tile's pixels at once. It draws lazily. When the CPU writes a register that affects pixels during drawing, the PPU first works out the current x from the time elapsed since drawing started and draws up to it; then the write takes effect. At the end of drawing it draws the rest of the line.

The pixels go through two passes:

1. Background and objects are composed into a per-line buffer of small color indices, with the priority bits packed above the index so the winning layer can be chosen with plain integer comparisons.
2. The index buffer is mapped to output colors through a palette that was converted to the output format when it was written.

On the GBA, the same split goes further: the whole line is rendered at the start of HBlank, layer by layer in priority order, with windows split into horizontal spans.

**What mGBA gives up.** On the GB it works out the length of mode 3 with a simple formula: a base length, plus a fixed cost per object on the line, minus the fine scroll. It ignores the objects' X positions relative to the background fetcher, the window and other quirks. Its timing is therefore not dot-exact. Nubo must not adopt that formula.

**The idea for Nubo.** Keep the SameBoy-derived dot pipeline as the source of truth. Add a line-level path that is taken only when its result is provably identical:

1. **When.** The PPU is about to run from the start of mode 3 (or from the current point in it) to the end of drawing in one call, with nothing pending. That means:
   - no object selected by OAM scan for this line, or objects disabled for the whole line;
   - the window can't trigger on this line (WY/WX conditions and their DMG/CGB quirks decide this, using Nubo's existing checks);
   - no countdown running, no one-dot glitch flag set, no OAM DMA or speed switch;
   - the LCD isn't in its first line after being switched on;
   - the CPU isn't in STOP mode.

   This holds whenever the CPU doesn't touch PPU state during mode 3, which the lazy design already makes the common case (and B makes more common).
2. **Timing in closed form.** For such a line, the dot at which drawing ends depends only on the line's start state: the fine scroll latched at the start of mode 3 plus Nubo's own model and speed variants, which Nubo already encodes in its calibrated constants. The same goes for the end state of the fetcher and FIFOs (which tile and step, what the FIFO holds, `position_in_line`, the mode 0 interrupt countdown). Compute that end state directly instead of stepping the fetcher eight times per tile, as `take_tile_steps` does now. The easiest way to get it right is to derive it from the existing dot code: run the dot pipeline over a plain line in a test, and check the closed form against it for every fine scroll value, model and speed.
3. **Pixels in a tight loop.** For each tile on the line, read the tile-map byte (and on CGB the attribute byte), then the two bitplane bytes. Turn the pair into eight 2-bit color indices with a 256-entry "bit-spread" table: it interleaves the bits of one byte with zeros, so combining two lookups gives all eight indices at once. Then map each index through the pre-converted RGBA palette for that tile's palette. Handle the partial first tile from the fine scroll, CGB horizontal flip and the VRAM bank. This is about the cost of mGBA's loop: a few instructions per pixel.
4. **Leave the state exactly as `tick_dot` would.** Jump the line state to HBlank at the computed dot. Set the visible mode, the STAT interrupt line, the HBlank DMA signal and everything else the dot path sets at that dot, by calling the same helper the dot path uses at the transition.

**A2, objects on the line (later).** Object fetches stall the FIFO for a number of dots that depends on the object's X relative to the background fetcher and the fine scroll. There are public descriptions of this (Pan Docs' mode 3 length section). Nubo would use a formula checked against its own pipeline, never mGBA's. Draw objects in a separate per-line pass into the index buffer:

- DMG priority: lower X wins, then lower OAM index. CGB priority: OAM index.
- Apply each object's BG-over-OBJ priority bit together with LCDC bit 0.
- Pack priority into the high bits, so composition is one comparison per pixel.

This is where most of the dot-pipeline cost goes (12 to 18% of all instructions), but it is also where the quirks are. Do it only after A is solid.

**Where Nubo differs and what it costs.** Nubo's plain path still pays per pixel for:

- shifting the FIFO;
- the eight fetcher step functions per tile;
- checks such as `can_draw_plain_pixel` and `has_steady_shape`;
- extracting the color index bit by bit (the single hottest source line in the profile).

The dot path runs `tick_dot` on every line with objects or a window. Together they are 40 to 58% of all instructions in these games.

**How it stays exact.** The fast path is an optimization of `run_dots`, so it is covered by Nubo's existing checks:

- a debug-build shadow check: run the dot path on a clone and compare state and pixels, as Nubo already does for `take_tile_steps` against `advance_fetcher`;
- new cases in `gpu/tests/random_scenes.rs`, which compare bulk and dot runs;
- `cargo benchmark --compare` between eager and lazy builds, over the games and all test ROMs;
- `cargo test-roms`.

The class of lines it accepts can start very narrow and grow.

**Estimated gain.** The plain path is 24 to 40% of all instructions. Cutting it by 70 to 85% gives roughly 1.25 to 1.5 times as fast. With A2 covering most lines that have objects, roughly 1.5 to 1.8 times.

**Risks.**
- Accuracy: a quirk the closed form misses. The shadow check, the random scenes and the eager fingerprints catch this.
- Save states: none. The fast path produces the same state, and any caches it adds are left out of save states and recomputed.
- Complexity: moderate for A, high for A2.
- The emulator's `Debug` output is compared between emulators in tests (`--verify-boot`, `clone_with_rom_matches_a_real_boot`). Any cache added must either be deterministic or excluded from that comparison.

### B. Validity windows: serve PPU-visible values without a catch-up

**What mGBA does (concept).** Every PPU state change the CPU can see happens inside a scheduled event, which writes the new value straight into the I/O register array: LY, the mode bits and the LYC flag in STAT, the interrupt flags. A CPU read of those registers is therefore a plain array load with no synchronization at all. VRAM and OAM accessibility is decided by the mode, which the events keep current. Only pixel-affecting writes during drawing make the renderer draw up to the current position.

**The idea for Nubo.** Nubo's PPU is dot-stepped and lazily behind, so it can't keep registers current all the time. It can, however, tell how long they stay unchanged. Whenever the PPU stops (after a catch-up, or when it works out its quiet dots), it records for each value the CPU can observe a value and an absolute dot up to which that value is certain:

- LY until the dot where it changes (including the early change on line 153);
- the STAT value (mode bits, LYC flag) until the next mode change or LYC-flag change;
- whether VRAM, OAM and palette RAM are readable and writable, until the next point where access gets blocked;
- the PPU's interrupt-flag bits, until the earliest dot it could request an interrupt (Nubo already computes this as quiet dots).

A CPU read at time t below the window's end is answered from the recorded value without running the PPU. Writes work the same way:

- A VRAM write can be applied right away, without a catch-up, if none of the dots the PPU still owes up to t read VRAM (the PPU is in HBlank, VBlank or OAM scan for that whole interval). When it later catches up, it sees exactly the memory it would have seen.
- An OAM write likewise, if the owed interval has no OAM scan and no object attribute fetch.
- A palette write likewise, if the owed interval has no mode 3 dot.
- Any write that affects timing (LCDC, STAT, LYC, SCX, WX and so on) still catches up as now, with `write_timing.rs` deciding the exact dot.

**Invariants.**
- A window may end too early, never too late. Ending too early only costs speed.
- The quirks Nubo calibrated (STAT showing the mode a dot late, the LYC compare timing, the CGB/DMG differences, OAM DMA's hold on OAM, HBlank DMA) end a window early instead of being modeled in it.
- Any change to PPU state ends all windows: register writes, LCD on/off, OAM DMA, speed switch, STOP, loading a state.

**Where Nubo differs and what it costs.**
- Every CPU access in 0x8000-0x9FFF, 0xFE00-0xFEFF, 0xFF0F, 0xFF40-0xFF55, 0xFF68-0xFF6C and 0xFFFF runs the waiting M-cycles and catches the PPU up (`cpu_read_byte_elsewhere`, `cpu_write_cycle_elsewhere`).
- In µCity CGB that is about 55,000 catch-ups per emulated second. Each has a fixed cost (entering `run_dots`, `has_steady_shape`, `stat_settled`, `quiet_dots`, recomputing the budget), estimated at 100 to 200 instructions.
- It also cuts mode 3 into pieces of a few dozen dots, which keeps it off any whole-line path.

**How it fits the lazy rule.** This is a stronger form of "nothing can see the PPU is behind". The value served is exactly what the caught-up PPU would show, because the window says that value can't change before t. Debug builds can check every served read against a cloned, caught-up PPU (expensive, so only in tests).

**Estimated gain.** 3 to 10% on games that poll PPU registers. More through A, which then sees whole lines.

**Risks.** Accuracy, if a window is too long: keep them conservative and test with the eager fingerprints over all test ROMs, which exercise those edges heavily. Save states: windows are caches, left out and marked expired after a load. Complexity: moderate.

### C. Dirty-line skipping

**What mGBA does (concept).** The GBA renderer keeps one dirty bit per line, and a snapshot of the registers each line was last rendered with. Any write to VRAM, OAM or palette RAM marks every line dirty. A register write marks the line about to be drawn dirty if the value differs from that line's snapshot. At render time, a line that isn't dirty, has the same register snapshot and the same window state as last frame is skipped, and the previous frame's pixels for it stay in the output buffer. It only advances the per-line accumulators (affine reference points) that later lines need.

**The idea for Nubo.**
- Keep a generation counter that goes up on every change to what pixels are made from: CPU, OAM DMA and HDMA writes to VRAM, OAM and palette RAM, and the DMG palette registers.
- For each screen line, store a key: the generation, the line's latched inputs (SCX, SCY, LCDC, WX, WY, the window's internal line counter, the CGB/DMG/compatibility rendering mode, the "blocked" state, and on SGB nothing, because the border composition is separate) and the line number.
- When the fast path of A renders a line whose key equals the stored key for the pixels already in the buffer, skip writing pixels.

The timing work for the line always runs.

**Where Nubo differs and what it costs.** Nubo renders every pixel of every frame inside the timing pipeline, so a static title screen costs as much as a busy level. The benchmark games spend much of their runs on title screens and menus, which `docs/EMULATOR_COMPARISON.md` notes.

**How it stays exact.** The frame buffer bytes are identical by construction, as long as the key covers every input. The risk is a forgotten input. The eager-vs-lazy and before-vs-after frame fingerprints catch it, since every frame is hashed. The key and the generation are caches: leave them out of save states and invalidate them after a load, a model change or a reset.

**Estimated gain.** Up to the pixel share of the PPU (after A, perhaps a third of what remains) on static scenes, and nothing on scrolling scenes. Biggest in menus and title screens.

**Risks.** Low for timing (it never touches timing). The main risk is a stale line from a forgotten input. Moderate complexity. It interacts with D (whichever buffer the stored key refers to).

### D. Hand over the finished frame without a full copy

**What mGBA does.** The renderer writes directly into the buffer the frontend reads (or into a buffer handed out by the threaded renderer), so there is no copy per frame.

**Nubo.** It copies 92 KB at every VBlank (2 to 4% of instructions).

**The catch.** The PPU's working frame buffer is part of save states. At any moment, its rows not yet drawn this frame hold the previous frame. A plain swap of the two buffers would leave the frame from two frames ago in those rows, which changes stored states and the end-state fingerprints. The same goes for the blank (white) and Super Game Boy cases, where the completed frame isn't simply the working one.

**Exact options.**
- (a) Copy only the rows C reports as recomposed.
- (b) Swap, and also keep the previous working buffer, so that saving writes the identical "current rows, plus the previous frame's rows below" content.
- (c) Swap, store the frame buffer under a new stored name, and accept a new kept set of save states (`cargo save-states --record`), if the maintainer agrees the in-progress frame's old rows don't matter to players. Since they would differ, it's also simpler to just keep (a).

**Estimated gain.** 2 to 4%.

**Risks.** Save-state compatibility (options b and c). Option (a) has none.

### E. Exact skipping of polling loops (idle loops)

**What mGBA does (concept, GBA only).**
- Every time the CPU jumps, it notes the target. If it jumps to the same address twice in a row with the same register values, it decodes the loop body once.
- If the body has no stores, and its loads come only from ROM or from I/O registers known to stay constant, the loop is marked idle. Each later arrival at that address puts the CPU in "halt until the next event".
- A per-game list can name the loop instead, and detection gives up after many failed attempts.

This is not exact: the CPU leaves the loop at the event's time rather than at its next read after it, which shifts timing by up to one iteration.

**The idea for Nubo, made exact.** Detect a backward jump to the same program counter where, at the loop head, all registers, flags and IME are the same in two consecutive iterations, and check that one iteration:

- takes a fixed number of M-cycles P;
- does no writes, no HALT, STOP, EI, DI or RETI, and handles no interrupt (none is pending and enabled, and none can become so before the bound);
- reads only plain memory (ROM, WRAM, HRAM, with cheats applied as now) or values with a validity window from B (LY, STAT, IF, DIV, and JOYP, whose value only changes between `Emulator` calls).

Then work out the bound: the earliest dot at which any value it reads can change. That is the minimum of:

- the windows;
- the next interrupt request;
- the timer's DIV visible changes, if DIV is read;
- the M-cycle that fills the audio buffer;
- `run_frame`'s dot limit.

Run k whole iterations in one go as plain M-cycles, where k ends strictly before the bound, and leave the CPU at the loop head, as k real iterations would. Each iteration reads the same values and ends in the same register state, so the result is identical.

Side effects to reproduce: the last CPU bus address (Nubo tracks it for the wave channel; it is the same after any whole number of iterations), and the cartridge bus value if a loop reads open bus.

**Where Nubo differs.** Nubo already skips HALT exactly (`bulk_cycles.rs`). A game that waits for VBlank with a polling loop instead pays for interpreting every iteration plus, without B, a PPU catch-up per iteration.

**Estimated gain.** 5 to 20% for polling games, nothing for HALT-based ones. It mostly matters after A and B, once the PPU no longer dominates.

**Risks.** Moderate complexity and subtle exits: an interrupt becoming enabled through the timer, serial or joypad, STOP, the OAM bug on 16-bit increments (`INC rr` inside the loop on DMG), or HDMA stealing cycles from the CPU. Use detection that refuses anything unusual, plus the eager fingerprints. Save states: only caches. No skip spans an `Emulator` call boundary.

### F. A cached earliest-deadline scheduler

**What mGBA does (concept).**
- One time-ordered queue of events (a sorted singly linked list, which is cheap for the dozen or so live events). Each event has an absolute due time and a priority for ties.
- The CPU loop compares one running cycle counter against one cached "next event" value per step (per M-cycle on the GB, per instruction on the GBA). Nothing else runs until that compare fails.
- When events are processed, the counter is folded into a master clock and reset, so the hot counter stays a small 32-bit number.
- Each callback is told how late it ran, so periodic events reschedule from their ideal time without drift.
- Scheduling a new event during processing is deferred to a side list, and the cached deadline is updated at the end.
- The GB DIV/TIMA event fires only at the next visible DIV change or the next TIMA increment, whichever is first, not every tick.
- A halted CPU jumps straight to the next event.

**Nubo.** It already has the equivalent fast path: the deferral budget is one compare-and-decrement per plain M-cycle. But every non-plain M-cycle (`sync_now`) works the budget out again by asking every component (`bulk_cycles_available`: PPU quiet dots, APU edge, timer overflow, serial, DMA, speed switch, dot limit). Keeping each component's deadline as an absolute time, updated only when that component's state changes, would make that a cached minimum.

**Estimated gain.** 1 to 3%.

**Risks.** Low. Deadlines are caches; recompute them after loading a state.

### G. An instruction-fetch fast path ("active region")

**What mGBA does (concept).** The CPU keeps a pointer to the memory block containing the program counter, an address mask and the end of the block. Opcode and operand fetches are a bounds check plus an indexed load. The full memory-map decode runs only when the program counter leaves the block or a jump lands elsewhere, and bank switches refresh it. On the GBA the same record also caches that region's wait states, so a fetch costs one table-free addition. The GBA's two-stage instruction prefetch is a two-entry queue filled from this block.

**Nubo.** Each fetch goes through the plain-memory check, a boot ROM check, the banked-read computation and the cheat lookup (`read_plain_memory`). This is a few percent of instructions.

**The idea.** Keep the start offset of the current ROM bank in the shared ROM, plus the window's end address. Fetch with one checked index. Refresh on leaving the window, any write to the cartridge's ROM-area registers, the boot ROM exit, cheat changes and loading a state. In safe Rust this is an index plus a bounds check.

**Estimated gain.** 1 to 3%.

**Risks.** Low. Missing a refresh would read the wrong bank, which `cargo cpu-tests`, the test ROMs and the fingerprints catch.

### H. Audio: what mGBA does, and why Nubo should keep its own

**mGBA (concept).**
- The GB APU's channels each remember when they last changed. When a sample is due, a channel works out how many of its periods have passed with one division and jumps its phase.
- The noise channel steps its shift register several bits at a time with lookup tables, and averages the noise level over a sample.
- Samples are taken at a fixed high rate by plain point sampling, a block at a time from one scheduled event, through a simple one-pole high-pass filter.
- The frontend then resamples to the device rate, with a cosine interpolator or a windowed sinc.
- The GBA's FIFO channels are sampled at the rate the sound bias register selects, fed by DMA from timer overflow events.

**Nubo.** It cuts its catch-up at every channel output change and mixes each change as a band-limited step (BLEP). This is exact and higher quality, and costs 3 to 6% of instructions, so it is not where the gap is. Point sampling with averaged noise aliases, and would change every audio fingerprint. Don't adopt it. The one idea worth a look is the closed-form phase advance for a channel whose output can't change before the next sample, which Nubo largely has already (`cycles_until_output_change`).

### I. Rendering on another thread

**mGBA (concept).** Optionally, the emulation thread records a stream of renderer commands: register writes, palette and OAM writes, "draw line y", "end of frame". VRAM changes are tracked as dirty 4 KiB blocks and sent as copies right before the next draw command. A second thread replays the stream into a full renderer. This works because the pixel generator is a pure function of VRAM, OAM, palettes and per-line registers.

**For Nubo.** It only becomes possible after A has separated pixels from timing. It saves wall-clock time, not instructions. On wasm32 it needs threads, with SharedArrayBuffer and cross-origin isolation. Low priority. A and C give more for less.

### J. A "frame not shown" mode

**mGBA.** Frame skip turns the renderer off for N frames while the timing events keep running. As used there, it is a display trade-off (dropped frames).

**For Nubo.** Offer an API flag meaning "the next frame won't be shown", for RetroArch's run-ahead secondary runs, rewind replay and fast-forward. With the flag set, the PPU runs all timing and skips pixel composition. It is exact for everything the game can observe, because no CPU-visible state depends on pixel colors. The exception is a Super Game Boy VRAM transfer, which reads the screen, so it must render while one is in progress.

**Estimated gain.** The pixel share of the PPU, on hidden frames only. Run-ahead users would feel it most. It needs A's separation (or at least a "no output" variant of the plain path).

---

## 3. What mGBA's GBA core does, for completeness

None of these are needed for SM83 emulation. They are the same patterns as above at a larger scale:

- **ARM interpreter.**
  - Instructions dispatch through decoded tables indexed by a dozen opcode bits (ARM) or the top ten bits (Thumb).
  - Condition codes are checked with a 16-entry table of bitmasks over the four flags.
  - ARM and Thumb have separate tight loops that check only the event deadline.
  - The two-stage pipeline is modeled as a two-word prefetch queue filled from the active memory block.
  - Wait states come from per-region tables cached with the active block.
  - The cartridge prefetch buffer is approximated when the CPU stalls.
- **Timers.** A timer's counter is computed from timestamps when it is read (elapsed time divided by the prescaler). An event is scheduled only for the overflow, and cascaded timers follow from it.
- **Video.**
  - Two events per line. The whole line is rendered at the start of HBlank, layer by layer in priority order, with windows split into horizontal spans.
  - Affine backgrounds step their reference point once per line.
  - Lines are skipped when not dirty (C).
  - Rendering can optionally run on a second thread (I).
  - VRAM stalls are approximated with a per-line mask of which layers fetch.
- **Idle loops.** Detected as in E, but approximate. There is also a table of known loop addresses per game.
- **DMA.** Started from video and timer events, with its cycles added to the CPU's.

## 4. Where Nubo already matches mGBA

- **Lazy PPU and APU with catch-up on access.** mGBA keeps its GB PPU's registers current with events and draws lazily. Nubo achieves the same laziness, exactly.
- **Bulk plain M-cycles.** These play the part of mGBA's single compare per M-cycle against the next event.
- **Halt skipping.** Exact in Nubo; mGBA jumps to the next event.
- **Timers.** Both run in bulk or batches, and both clock the APU's frame sequencer from DIV edges.
- **Palettes converted to RGBA when they are written**, and tile-at-a-time background drawing.

So the scheduler is not where the gap is. The PPU's per-pixel and per-line cost is.

## 5. Techniques that trade accuracy for speed (don't adopt)

- **GB mode 3 length from a formula** based on the object count and fine scroll only, ignoring object X positions, fetcher alignment and window timing.
- **A GB PPU with four events per line, drawing ranges at register writes.** Mid-line effects come out at the right pixel only as far as the approximate timing allows. Several STAT interrupt quirks are left unhandled.
- **GBA idle-loop removal as "halt until the next event"**, which shifts when the loop exits.
- **Fixed-rate point sampling of the APU with averaged noise**, plus cosine interpolation in the frontend (aliasing).
- **Frame skipping that drops displayed frames.** (J, the exact variant for hidden frames, is fine.)
- **Starting from the post-boot state** when no known boot ROM is present. This is not a speed technique, but it makes mGBA's runs a little shorter in the comparison.

## 6. Suggested order of work

1. **A, narrow.** A plain-line mode 3 fast path: no objects, no window, nothing pending. Add a debug shadow check against the dot path, extend `random_scenes.rs`, and check eager-vs-lazy fingerprints over the games and all test ROMs. This is where the biggest single gain is.
2. **B.** Validity windows for LY, STAT, IF and VRAM/OAM/palette accessibility, conservative at first. Measure how much more often A applies on polling games (µCity CGB is a good probe).
3. **D, option (a), together with C.** Dirty-line keys, and copying only recomposed rows into the completed frame. Exact, with no change to save states.
4. **A2.** Lines with objects (and later the window) on the fast path, with an object-penalty formula verified against Nubo's own pipeline.
5. **E.** Exact polling-loop skipping, once the PPU no longer dominates and the CPU's share is worth attacking.
6. **J.** The "frame not shown" flag for RetroArch run-ahead and fast-forward.
7. **F and G.** Small, low-risk cleanups that can go in at any time.

After each step:
- run `cargo benchmark --compare` against a recording from before the change;
- compare an eager build with a lazy build of the same commit;
- run `cargo test-roms`;
- if anything stored changed (it shouldn't), run `cargo save-states`.

## 7. Provenance

- **Source:** mGBA, https://github.com/mgba-emu/mgba, shallow clone of commit `143adf5aa2a60c6036810b0cd72445e13d74b0e6` (2026-10-07, "Qt: Add options for audio-buffer-sizes of 48 and 32 samples"). License: Mozilla Public License 2.0. Read as untrusted data; nothing was built or run, and nothing was copied into Nubo.
- **mGBA files read:**
  - `src/core/timing.c`, `include/mgba/core/timing.h`
  - `src/sm83/sm83.c`, `src/sm83/isa-sm83.c` (beginning)
  - `src/gb/gb.c`, `src/gb/video.c`, `src/gb/timer.c`, `src/gb/memory.c`, `src/gb/io.c` (register write part), `src/gb/audio.c`, `src/gb/renderers/software.c`, `src/gb/core.c` (frame-skip option only)
  - `src/util/audio-resampler.c`, `src/util/interpolator.c`
  - `src/arm/arm.c`, `src/arm/isa-arm.c` and `src/arm/isa-thumb.c` (table declarations only)
  - `src/gba/gba.c`, `src/gba/memory.c`, `src/gba/io.c` (read path), `src/gba/video.c`, `src/gba/timer.c`, `src/gba/audio.c`, `src/gba/renderers/video-software.c`, `src/gba/renderers/software-mode0.c`
  - `src/feature/video-logger.c`, `src/feature/thread-proxy.c`
  - `src/platform/libretro/libretro.c` (searched for frame-skip handling)
- **Nubo files read:**
  - `AGENTS.md`, `crates/nubo-core/AGENTS.md`, `docs/EMULATOR_COMPARISON.md`
  - `crates/nubo-core/src/address_bus.rs`, `address_bus/bulk_cycles.rs`, `address_bus/dma.rs`, `address_bus/write_timing.rs`
  - `cpu.rs`, `cpu/microops.rs`, `cpu/opcodes.rs`
  - `gpu.rs`, `gpu/steady.rs`, `gpu/pipeline/plain_pixels.rs`, `gpu/palettes.rs`, `gpu/save_state.rs`
  - `apu/catch_up.rs`, `apu/blep.rs`
  - `emulator.rs` (frame API), `tools/nubo-benchmark/src/main.rs` (options)
- **Profiling:** Nubo at the working tree's HEAD, built with `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only` into a separate, temporary target directory (the repository was not modified). Run under `valgrind --tool=callgrind` with collection limited to `step_until_next_audio_buffer`, 10 emulated seconds per game and model. The output files were kept in a temporary folder only.
