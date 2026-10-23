# How bsnes and ares get cycle accuracy and speed, and what Nubo can take from them

Clean-room "reader" report: ideas and techniques only. It holds no source code, no pseudo-code that follows their functions, no constants or timing numbers from them, and no identifiers from their source. File names appear only under Provenance. Nubo identifiers (`run_dots`, `has_steady_shape` and so on) are Nubo's own.

---

## 1. Summary

### What made bsnes fast without losing accuracy

From v107 on, bsnes got faster mainly in four ways. Only the first two keep full accuracy.

1. **Components sync only when another component could see their state.** Every chip is a cooperative thread with its own clock. A chip runs ahead of the others, sometimes a long way, until it touches state that another chip shares. Only then does it hand control over. The audio CPU, for example, runs until it reads or writes the four mailbox registers it shares with the main CPU, and the main CPU does the same from its side. The audio DSP is not a thread at all. It is a resumable state machine that the audio CPU "pulls" forward to the current time before it touches the RAM the two share.
2. **The CPU keeps its own copy of the PPU's beam position.** The horizontal and vertical counters move in a fixed, predictable way, so the main CPU advances its own copy as it runs. It answers IRQ timing, the H/V-blank status register and DMA start points without asking the PPU. The PPU can then run behind the CPU by up to a scanline, and only reads or writes of PPU registers force a sync.
3. **A fast, scanline-based PPU, separate from the accurate one.** At a fixed point in each line it takes a snapshot of the PPU registers and palette RAM into a per-line slot. Lines are drawn later, in a batch, and several threads can draw them at once. The pending lines are drawn ("flushed") before any write to VRAM or sprite memory, because those memories are shared and not part of the snapshot. Drawing a whole line at once, with its own tight loops for each layer, is much cheaper than working through the hardware's fetch schedule dot by dot. **Not exact:** an effect in the middle of a line is seen only at the snapshot point. Its own header says so.
4. **Speed options that give up accuracy.** These include a fast DSP that runs a whole sample at a time, coprocessors synced only once per scanline, no VRAM blocking, per-game render points, and overclocking. Nubo must not adopt any of these.

The accurate PPU stayed dot-based but got cheaper. The dot schedule of a line is unrolled at compile time, so each dot position compiles to only the work that happens there. The real work sits in out-of-line functions so the unrolled body stays small enough for the instruction cache. A comment in the source credits this with a large share of the speed of the whole emulator.

ares, the successor, uses an absolute-timestamp scheduler for any number of threads. Each thread's clock counts in a common unit of time scaled by its frequency, a fixed unique offset breaks ties, and the smallest value is subtracted now and then so the clocks don't overflow. To save a state, ares runs every thread to a "safe point" first. Its Super Nintendo cores are the bsnes ones: an accurate dot-based PPU and a "performance" PPU that draws each line in one go at a fixed point, with per-game overrides of that point (not exact). **The ares Game Boy core offers Nubo nothing for speed.** It runs in lockstep, switching threads every clock. Mode 3 has a fixed length and there is no pixel FIFO, so it is less accurate and slower by design.

### What matters most for Nubo

A callgrind profile of Nubo's release benchmark (Geometrix and Tobu Tobu Girl, CGB, 10 emulated seconds, about 2.1–2.5 billion instructions each) shows where the time goes:

- **The PPU takes about 60% of all instructions.** Most of that is mode 3: about 14% in the plain-pixel path (`plain_pixels.rs`), about 12% in the inlined pipeline code, and about 10% more in the fetcher, tile and pixel helpers. That is roughly 35–40% of the whole run, or about 60 instructions per mode-3 dot. A further 5% goes to re-checking whether a stretch is steady (`has_steady_shape`, `stat_settled`, `stat_line_level`).
- **The PPU catches up in short pieces.** There are about a million `run_dots` calls per 10 emulated seconds, averaging about 40 dots each. The largest caller is CPU reads of PPU registers (`cpu_read_byte_elsewhere`). In Geometrix, one polling instruction (`BIT n,(HL)` on a PPU register) carries 28% of the run inclusively, because every iteration forces the PPU to catch up and ends the bulk stretch.
- About 5% is the bookkeeping of the lazy scheme itself (`run_deferred_cycles`, `bulk_cycles_available`, `sync_all_but_timers`). About 2.3% is a `memcpy` that copies the finished frame. CPU decode and bus reads take roughly 15%.

The techniques, ranked by estimated impact for Nubo:

| Rank | Technique (where it comes from) | Exact? | Estimated gain on Nubo |
|---|---|---|---|
| 1 | Separate the PPU's timing from its pixels. Draw "clean" lines with an exact line renderer, and when a mid-line change comes, replay the reference FIFO from a checkpoint (bsnes's line cache with flush-on-change, made exact) | Yes, by construction plus differential tests | 1.3–1.6x on the benchmark games; more on games with busy screens |
| 2 | Know mode 3's length at the start of mode 3, and answer reads of LY, STAT and IF from the predicted timeline without catching up (bsnes's CPU-side copy of the beam counters) | Yes | 5–15%; much more on games that poll in a loop |
| 3 | In-PPU event horizon: work out the next dot at which anything can happen once, then run straight to it. Specialize the line schedule at compile time and keep rare work out of line (bsnes's accurate PPU) | Yes | 3–8% |
| 4 | A page table for the CPU's fast path, so ROM, work RAM and HRAM reads need no range matching (bsnes/ares bus lookup tables) | Yes | 2–4% |
| 5 | Hand over finished frames by swapping buffers instead of copying them (bsnes hands the frontend its output buffer) | Yes | about 2% |
| 6 | Monomorphize hot paths over settings fixed for a whole run: processor-test mode, model family, speed (bsnes's instruction tables per mode, templated step counts) | Yes | 1–3% |
| 7 | Skip pixel production when nobody can see it (run-ahead and frame-skip frames in bsnes). Needs #1, and never on the Super Game Boy | Yes for the emulation (frame contents need care) | Large, but only for run-ahead and fast-forward |
| 8 | Batch rendering of the frame at VBlank, and multithreaded line rendering (bsnes fast PPU) | Batching yes; threads are a host detail | Small for Game Boy; not recommended now |

Not to adopt as they are (accuracy traded for speed): a fixed render point per line, or one chosen per game; ignoring VRAM and OAM blocking; syncing a chip only once per scanline; running the audio chip a whole sample at a time when the CPU can read its registers in between; per-game redirects of writes.

Already covered in Nubo: catch-up instead of cooperative threads, "sync only when observable" (the lazy PPU and APU, bulk M-cycles), one clock domain, and save states with no stack to save.

---

## 2. Techniques

### 2.0 Background: cooperative threads, relative clocks and "sync only when needed" (bsnes, ares)

**The concept.** Each chip (main CPU, audio CPU, audio DSP, PPU, coprocessors) is written as a plain loop that runs forever and counts the time it uses. Underneath, each loop is a coroutine with its own stack. The scheduler switches only when a chip is about to do something another chip could see, and only if that other chip is *behind* in time. bsnes keeps one signed number per pair of chips: one side adds its cycles multiplied by the other side's frequency, the other side subtracts in the same way, and the sign tells who is ahead, with no division. ares instead gives every thread an absolute timestamp in a shared unit, with a per-thread tie-breaking offset so equal times resolve the same way every run. It subtracts the minimum now and then to keep the numbers small.

**Invariants that keep it exact.**
- A chip may run ahead only while it touches nothing another chip can see. Every access to shared state (mailbox ports, shared RAM, registers) first lets the other side reach the same time.
- Both directions are guarded. A write from A to a register B reads, and a read by A of a register B writes, each sync B first.
- A cap on how far a chip may run ahead, enforced by a forced sync every scanline, keeps memory and latency bounded and gives natural places to stop for frame output.
- Save states need every coroutine at the top of its loop, where its stack holds nothing. Both projects either run all threads to such a safe point before saving, or copy the raw stacks (not portable).

**How Nubo differs.** Nubo has no coroutines: the CPU drives everything. The PPU and APU are state machines that owe dots or cycles and catch up before anything can see them. That is exactly the "sync only when needed" idea, written as catch-up, which is the right translation for safe Rust and wasm32. Nubo already has the pieces: `reaches_ppu` is the guard on shared state, `run_dots` reports the quiet horizon, and `dot_limit` bounds drift. Nubo has one clock domain (dots, with M-cycles as multiples), so the frequency-scaled clocks bsnes and ares need are unnecessary. Save states are trivial, since every component is a plain struct.

**What is left to take.** Two things, below:
- The *direction* of sync. bsnes makes a chip run ahead and the CPU pull it forward only on access; Nubo does this already.
- *Predicting* the other chip's observable state without running it (2.2).

The pull-driven DSP is worth noting as a pattern. It is a resumable state machine that keeps its phase within the sample: entering it continues from the saved phase, falls through the remaining phases until the clocks given run out, and stores the new phase. Nubo's APU catch-up already does this in its own way.

### 2.1 Separate PPU timing from PPU pixels; exact line rendering with checkpoint-and-replay fallback (from bsnes's fast PPU, made exact)

**The problem.** In Nubo, mode 3 costs about 60 instructions per dot. It is about 35–40% of all instructions, the biggest single cost. The reference pipeline (the SameBoy port) works through the fetcher's steps, two FIFOs and object stalls one dot at a time. Nubo's plain-pixel shortcut already skips the phase machinery on simple dots, but it still works per dot and still re-checks its conditions per dot.

**What bsnes does.** The fast PPU accepts that a line is a function of (registers and palette at one moment) plus (VRAM and sprite memory). It snapshots the first part per line and draws the line with layer-by-layer loops. To defer drawing safely, it flushes the pending lines before anything changes the parts it did not snapshot. It is inexact only because the snapshot moment is fixed and mid-line writes to registers or palettes are ignored.

**The exact version for Nubo.** Keep the idea of "snapshot plus flush-on-change", but replace the fixed snapshot moment with *proof that nothing changed*, and replace "ignore mid-line changes" with *replay the reference*.

1. **Split timing from pixels.** On the Game Boy, how long mode 3 lasts, and so the STAT mode changes, the mode 0 interrupt and the HBlank DMA start, depends only on non-data inputs: SCX's fine bits, whether and where the window starts (LCDC's window bit, WX, the WY trigger), the objects OAM scan found and their X positions, LCDC's object enable and size bits, the model and the speed. It never depends on tile data or palette contents. So timing can be worked out at the start of mode 3 without producing a single pixel.
2. **Get the timing exactly.** There are two safe ways. Ideally use both, the second checking the first in debug builds and tests.
   - *Timing-only replay:* run the reference fetcher/FIFO state machine in a mode that skips VRAM data reads, palette lookups and frame buffer writes, and only counts dots until drawing ends. This is exact by construction and much cheaper per dot than the full pipeline.
   - *Memoization:* cache the mode-3 length under a key made of every timing input listed above: fine scroll, window parameters, the object X list with each object's place in the OAM-scan order, the LCDC bits, model and speed. Within a frame most lines share a key, so the replay runs once per distinct configuration. The key must cover every input, because a missing one silently changes the timing. Have debug builds and the random-scene tests compare the cached value with a fresh replay.
3. **Define a "clean line".** From the start of mode 3 to its end:
   - no CPU write lands on anything the pipeline reads during mode 3: LCDC, SCX, SCY, WX, WY, the DMG palettes, the CGB palette data and index registers, OPRI, VRAM and OAM (Nubo already routes all of these through `reaches_ppu` and the dot-by-dot write path);
   - no OAM DMA or VRAM DMA activity touches OAM or VRAM;
   - no one-dot glitch flag set by `write_timing.rs` is pending;
   - no countdown is running (`Countdowns` idle);
   - the line is not the first after the LCD is switched on;
   - no STOP-mode blocking applies.

   Most lines of most games are clean.
4. **Draw clean lines with a line renderer.** It goes tile by tile (decode a tile row's two bitplanes into eight color indices at once, then map them through a small palette table) and object by object (merge each object's eight pixels by the model's priority rules, the BG-to-OBJ priority bits, CGB master priority and OPRI). It has a separate inner loop for each color mode, as `draw_plain_pixels_in` has now. It must reproduce what the reference FIFO gives on clean lines, including the quirks that come from clean inputs: fine-scroll discard, objects partly off the left edge, the window at the screen edges, the window line counter, the ten-object limit (already applied by OAM scan) and the DMG and CGB object ordering. For any configuration it does not cover yet (rare window positions, unusual object overlaps), it **declines**, and the line runs through the reference pipeline as today. This is the same "decline to the fast path" pattern as `can_draw_plain_pixel`, but at line granularity.
5. **Checkpoint and replay on a mid-line change.** When a line starts as clean and a relevant write arrives in the middle of mode 3, the line is no longer clean. Nubo already catches the PPU up before such a write and runs that M-cycle dot by dot. At that moment, *before the write is applied*:
   - restore the pipeline state saved at the start of mode 3;
   - replay the reference pipeline dot by dot from mode 3's start to the current dot, writing the frame buffer row as usual;
   - apply the write;
   - continue dot by dot to the end of the line.

   This is exact without snapshotting any registers or memory. Being clean so far *means* nothing the pipeline reads has changed since mode 3 started, so the current registers, VRAM and OAM are exactly what the replay needs. The checkpoint is only the pipeline struct at mode-3 start (empty FIFOs, the fetcher's starting state, the OAM-scan result), a few hundred bytes and a cheap clone.
6. **When to draw.** The simplest exact choice: draw at the end of mode 3, or at the replay point if a write came first. Deferring further (whole-frame batches, 2.8) is possible but needs flushes before every VRAM and OAM write, which come often in VBlank, so it adds little on the Game Boy.

**Fitting the bit-exact rule.** The frame buffer row, the mode-3 length, every STAT and interrupt event and every save-state field must equal the `eager` build. The line renderer and the timing cache sit behind the same kind of guard the steady stretches use. The replay uses the reference code itself. Verify it with:
- the existing randomized `gpu/tests/random_scenes.rs`, extended to random clean lines with random objects, window and scroll, comparing the line renderer and timing cache against the FIFO;
- random mid-line writes, to exercise replay;
- `cargo benchmark --compare` between an eager build and a normal one, on the benchmark games and the test ROMs.

**Save states.** If the checkpoint is only taken and used inside one catch-up, it never needs to be stored. A save state can come in the middle of mode 3 (between `Emulator` calls). To keep this simple, either:
- finish the line through the reference path before returning from an `Emulator` call while a line is in fast mode, as Nubo already runs deferred M-cycles at the end of each call; or
- draw the clean line up to the current dot and hand the pipeline state at the current dot to the reference engine.

The first way changes nothing in the stored layout. Store no new fields unless needed. If any are added, give them new stored names.

**Estimated gain.** If clean lines move from about 60 to about 5–10 instructions per mode-3 dot (line rendering plus amortized timing), mode 3 falls from about 35–40% of instructions to about 5–10%: roughly **1.3–1.6x** overall on the benchmark games. Scenes with many objects and scrolling gain more, title screens less.

**Risks.**
- **Accuracy:** a missing timing-key input or quirk. Mitigate by declining anything unusual, and by differential tests against the FIFO.
- **Complexity:** a second renderer to maintain next to the SameBoy port. Keep it small and let it decline freely.
- **Save states:** none if lines are finished before returning.

### 2.2 Predict observable PPU state; answer register reads from the timeline (from bsnes's CPU-side beam counters)

**The problem.** Games poll LY, STAT (mode bits, LY=LYC) and IF in tight loops while they wait for VBlank or HBlank. In Nubo every such read forces `run_deferred_cycles` plus `catch_up_ppu`, ends the deferral budget and re-runs `bulk_cycles_available`. In Geometrix this path carries about 28% of the run inclusively. Most of that is PPU work that would happen anyway, but the catch-ups average only about 40 dots each, so the fixed cost per call (`has_steady_shape`, `stat_settled`, `quiet_dots`, deferral bookkeeping) and lost bulk stretches add up.

**What bsnes does.** The beam position advances in a fully predictable way, so the CPU keeps its own copy and never asks the PPU for it. The PPU only syncs on its own register accesses. To model the small hardware delay between the counters and the CPU's interrupt logic, it computes "the counter value some clocks ago" from the current value instead of storing a history.

**For Nubo.** While the PPU is in a steady shape and mode 3's end is known in advance (2.1, or `Pipeline::dots_before_last_pixels` as now), LY, the STAT mode bits and the LY=LYC flag at "now plus owed dots" are a pure function of (line, line dot, owed dots, the known mode-3 end, LYC, and the existing visible-mode lag rules). IF's PPU bits during a quiet stretch are by definition unchanged (`quiet_dots` guarantees that no request can happen). So:

- give `Gpu` a read-only "what would this register read return after `n` more dots" query. It is valid only when the stretch is steady and no countdown, delayed LYC or delayed LCDC effect is pending. Otherwise it returns "unknown" and the bus catches up as today;
- treat a read of LY, STAT or IF answered this way like a plain read: no catch-up, no flush of deferred cycles, deferral continues;
- writes still catch up as today.

**Exactness.** The answer must equal what the PPU would compute after catching up. Prove it by having debug builds catch up a clone and compare, and by having the random-scene tests compare the prediction with the dot-by-dot value at random offsets. The visible-mode lag and the LY=LYC rules for line 153 are the risky parts, so let the query decline near line starts and ends (as `STEADY_START_DOT` does).

**Further step (related, not from bsnes).** A polling loop that only reads predictable registers and plain memory, and branches back, could be fast-forwarded to the dot where the polled value changes. This is often called idle-loop or spin-loop skipping. It is exact only if the loop provably has no other effects (no writes, no flags left different), so detect it conservatively.

**Gain.** 5–15% on the benchmark games. Much more on games that poll in a loop instead of halting.

**Risks.** Subtle STAT timing. The guard must be conservative. Save states are unaffected, since nothing is stored.

### 2.3 Event horizon inside the PPU; compile-time specialized line schedule (from bsnes's accurate PPU)

**The problem.** `run_dots` re-checks `has_steady_shape`, `stat_settled` and the line state for each chunk, and the reference pipeline dispatches on fetcher and phase state every dot. Together about 5% of instructions are spent asking whether nothing changes.

**What bsnes does.** Each dot position of the line is a compile-time instance, so "what happens at dot X" costs nothing to decide at run time. Hot but rare work (fetches, sprite evaluation) is moved into out-of-line functions so the unrolled schedule stays small. The DSP's resumable phase machine applies the same idea to audio.

**For Nubo (safe Rust).**
- Keep one "next event dot" per line: the earliest of the next mode change, the WY check, a countdown expiring, mode 3's end (known from 2.1), the line end and the frame end. Update it only when a write or event changes the inputs. Then `run_dots` advances `line_dot` straight to it, with no per-chunk shape checks. `has_steady_shape` becomes a debug assertion instead of a hot check.
- Make the timing variant (DMG, CGB, CGB double speed) and the color mode generic parameters of the line-schedule and renderer functions, as `draw_plain_pixels_in` already does for colors, so the inner loops carry no model branches.
- Mark the rare paths (`tick_drawing`'s glitch branches, object fetch, window start) `#[cold]` or `#[inline(never)]` so the hot loop stays small for the instruction cache, as bsnes found out the hard way.

**Exactness.** This is pure restructuring and must give the same results. Check with the random-scene tests and with fingerprint comparison against an eager build.

**Gain.** 3–8%. It overlaps with 2.1, which takes most of mode 3 off this path.

### 2.4 A page table for the CPU's memory fast path (from the bsnes/ares bus)

**What they do.** At startup, and whenever a mapping changes, the bus builds a lookup table that maps each address to a handler number and an offset. Every access then costs one indexed load and an indirect call. bsnes uses a table entry per address in the full 24-bit space. That costs a lot of memory but needs no range logic at all.

**For Nubo.** Everything mapped on the Game Boy is 4 KiB-aligned, except the I/O, HRAM and OAM area at the top. A page table of 16 entries (4 KiB) or 64 entries (1 KiB) is enough. Each entry is either a direct slice of plain readable memory (the boot ROM overlay, the current ROM banks, a work RAM bank) or "slow". Update the affected entries when:
- an MBC write changes `rom_banks`;
- the boot ROM is unmapped;
- SVBK changes;
- cheats are registered (then mark pages that have cheats slow).

The fast path then becomes one table load plus a slice index, with no `match` on address ranges, no boot-ROM check and no per-read cheat check. `processor_test_mode` simply maps everything slow.

**Exactness.** Pure restructuring. The risk is a stale entry after a mapping change. Mitigate it with a debug assertion that compares the table with `read_plain_memory`, as Nubo's `rom_banks` check does now.

**Gain.** 2–4% (`cpu_read_byte`, `read_byte_in_synced_cycle` and `peek` together are about 8% self).

### 2.5 Swap frame buffers instead of copying (bsnes hands over its output buffer)

`publish_frame` copies the finished frame into `completed_frame` every frame, about 2.3% of instructions. Keep the working buffer in the frontend's format and swap the two buffers at VBlank. The next frame redraws every visible pixel anyway. The cases where it does not (LCD off with white, the first frame after the LCD is switched on, STOP) already write their own content. `frame_buffer` is stored in save states and `completed_frame` is not, so after a swap make sure the state still stores the working buffer's contents in the same format and under the same name. If the stored format must change, use a new stored name.

**Gain:** about 2%. Low risk.

### 2.6 Specialize hot paths over settings fixed for a run (from bsnes's mode-specific instruction tables and step templates)

bsnes compiles four complete instruction tables, one per combination of its CPU's register-width flags, so no instruction re-checks a mode. It also makes the cycle count of each step a compile-time parameter, so the per-step loop unrolls. Nubo has no CPU modes, but every bus access checks `processor_test_mode()` at run time. `cgb_mode`, the model and the double-speed state are re-checked in many hot paths, and the deferral path re-asks the same questions on every `sync_now`.

Options:
- a const-generic "flavor" on the run loop (normal or processor-test; DMG family or CGB), chosen once per `step_until_next_audio_buffer` or `run_frame` call;
- keeping derived flags (such as "may defer", "a slow page is mapped") so the fast path tests a single boolean.

**Gain:** 1–3%. Pure restructuring.

### 2.7 Skip pixel production when it cannot be seen (bsnes run-ahead and frame-skip frames)

bsnes's PPU skips drawing on frames the frontend will throw away. That is exact for it because drawing has no effect on timing. Once Nubo has 2.1, its timing no longer depends on drawing either, so a frontend could ask for "timing only" frames for RetroArch's run-ahead hidden frames (libretro tells the core when video is not wanted) and for fast-forward frames that are never shown.

Constraints:
- **Never on the Super Game Boy models.** The SNES side reads the Game Boy's drawn pixels for its VRAM transfers (border, palette and attribute data), so there the pixels are visible to the emulated system.
- `frame_buffer` is stored in save states, and a frame boundary may fall in the middle of an LCD frame. Lines skipped before a state is saved would come back stale, both in the state and in the next frame shown. Either skip only whole LCD frames that start and end inside the skipped period, or keep the clean-line snapshots and draw them when a state is saved. The first is simpler.
- It must not change any fingerprint other than the frames that were explicitly not wanted.

**Gain:** for run-ahead with one hidden frame, almost the whole pixel cost of that frame. Nothing in normal play.

### 2.8 Batch rendering at VBlank and multithreaded line rendering (bsnes fast PPU): not recommended now

Deferring all lines of a frame and drawing them at VBlank improves cache locality and allows threads. On the Game Boy, though:
- the screen is small (a frame is a fraction of a millisecond of rendering at Nubo's speeds), so thread handoff costs about as much as it saves;
- the web build has no threads without extra setup;
- games write VRAM and OAM throughout VBlank, and the HBlank DMA writes VRAM between lines, so flushes would come often.

Revisit only after 2.1, and only on desktop if profiles show line rendering still dominant.

### 2.9 Techniques that trade accuracy for speed: do not adopt as they are

- **A fixed render point per line**, plus per-game overrides of that point (bsnes fast PPU, ares performance PPU). Mid-line raster effects are lost. Nubo's exact replacement is 2.1.
- **Ignoring VRAM blocking during active display**, and redirecting OAM writes per game (bsnes options and hotfixes). This changes what games see.
- **Syncing a chip only at scanline boundaries** (bsnes's delayed coprocessor sync). Cross-chip communication within a line goes wrong.
- **Running the audio chip a whole output sample at a time** (bsnes's fast DSP option). The audio CPU can read the DSP's envelope, output and end-of-sample registers within a sample. Nubo's APU catch-up already cuts batches where output can change and catches up before reads of channel state, which is the exact version.
- **Overclocking, frame-skip of game logic, and title-based hotfixes.**

### 2.10 ares's Game Boy core

It runs each component as a thread and syncs on every clock. It uses a fixed mode-3 length with no object or scroll penalties, no pixel FIFO, and samples sound at the system clock rate. It is less accurate than Nubo and structurally slower. The one idea it shows is a tiny per-dot history of the PPU mode (a shift register) for reads that see the mode a few dots late. Nubo already models these lags with its own fields (`visible_mode`, `previous_lcdc`, `previous_lyc_flag`). It has nothing to offer for speed.

---

## 3. Suggested order of work

1. **2.5 Swap frame buffers.** Small, low risk, about 2%, and good practice for the save-state field rules.
2. **2.3 (first half): event horizon in `run_dots`.** It removes the repeated shape checks and sets up the structure 2.1 needs (a known mode-3 end and a next-event dot).
3. **2.1 Timing split.** First the timing-only replay plus the memo cache, with exhaustive differential tests against the FIFO. Use it at first only to set the next-event dot and to drop per-dot STAT checks during mode 3.
4. **2.2 Predicted register reads**, which builds on the known timeline. Measure on the poll-heavy benchmark games.
5. **2.1 Line renderer with checkpoint-and-replay**, starting with the most common case (no window, up to ten objects, DMG and CGB color modes), and widening coverage as tests allow. Compare eager and normal builds on the benchmark games and the test ROMs (`cargo benchmark --compare`) at every step.
6. **2.4 Page table** and **2.6 specialization**, as cleanup once the PPU no longer dominates the profile.
7. **2.7 Timing-only frames** for libretro run-ahead and fast-forward, if wanted.
8. **2.8**: only if profiles show a need.

At each step:
- run `cargo test-roms`;
- compare fingerprints against a build without the change (on the benchmark games and the test ROMs) and between eager and normal builds;
- run `cargo save-states` whenever stored fields could change.

---

## 4. Provenance

**Repositories (shallow clones, read only; no build scripts run):**
- bsnes: https://github.com/bsnes-emu/bsnes, commit `78474e5d39f58872dc21aca18cf9eb583b147fe7` (2026-10-07). License GPL-3.0-or-later (`LICENSE.txt`, `GPLv3.txt`). The DSP files from the "snes_spc" library inside it are LGPL-2.1-or-later (Shay Green), per their file header. libco is ISC.
- ares: https://github.com/ares-emulator/ares, commit `9408cb43d4948fc3ea6e152a307a34348df3fe04` (2026-10-06). License ISC (`LICENSE`).

**bsnes files read:**
- `bsnes/emulator/emulator.hpp`
- `bsnes/sfc/sfc.hpp` (scheduler and thread)
- `bsnes/sfc/system/system.cpp`, `bsnes/sfc/system/serialization.cpp`
- `bsnes/sfc/cpu/cpu.hpp`, `cpu.cpp`, `timing.cpp`, `memory.cpp`, `io.cpp`, `irq.cpp`
- `bsnes/sfc/memory/memory.hpp`, `memory-inline.hpp`, `memory.cpp`
- `bsnes/sfc/ppu/counter/counter.hpp`, `counter-inline.hpp`
- `bsnes/sfc/ppu/ppu.cpp`, `main.cpp`, `io.cpp` (start)
- `bsnes/sfc/ppu-fast/ppu.hpp`, `ppu.cpp`, `line.cpp`, `io.cpp` (start)
- `bsnes/sfc/smp/smp.hpp`, `smp.cpp`, `timing.cpp`, `memory.cpp`, `io.cpp` (port parts)
- `bsnes/sfc/dsp/dsp.hpp`, `dsp.cpp`, `SPC_DSP.cpp` (structure of its run loop)
- `bsnes/processor/wdc65816/instruction.cpp`, `instructions-read.cpp` (dispatch structure)

**ares files read:**
- `ares/ares/scheduler/scheduler.hpp`, `scheduler.cpp`, `thread.hpp`, `thread.cpp`
- `ares/sfc/ppu/main.cpp`, `ares/sfc/ppu/ppu.cpp` (step)
- `ares/sfc/ppu-performance/ppu.hpp`, `ppu.cpp`
- `ares/sfc/cpu/timing.cpp`
- `ares/sfc/smp/timing.cpp`
- `ares/sfc/dsp/dsp.cpp` (sync calls)
- `ares/sfc/memory/memory.hpp`, `inline.hpp`
- `ares/gb/cpu/timing.cpp`, `ares/gb/cpu/memory.cpp`, `ares/gb/ppu/ppu.cpp`, `ares/gb/apu/apu.cpp`

**Nubo files read:**
- `AGENTS.md`, `crates/nubo-core/AGENTS.md`, `docs/EMULATOR_COMPARISON.md`
- `crates/nubo-core/src/address_bus.rs`, `address_bus/bulk_cycles.rs`, `address_bus/dma.rs`, `address_bus/write_timing.rs` (header), `address_bus/cheats.rs` (`apply_cheats`)
- `cpu.rs` (header), `cpu/opcodes.rs` (step and dispatch), `cpu/microops.rs`
- `gpu.rs` (header, `run_dots`, `publish_frame`), `gpu/steady.rs`, `gpu/pipeline.rs` (header), `gpu/pipeline/plain_pixels.rs` (header), `gpu/save_state.rs` (field list)

**Measurements:**
- callgrind on Nubo's own `nubo-benchmark` (release; once more with line tables, built into the scratchpad), geometrix.gbc and tobudx.gb on CGB for 10 emulated seconds;
- the profiles were kept in a temporary folder only.

Nothing was copied into or changed in `/home/user/nubo`.
