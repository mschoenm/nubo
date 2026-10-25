# Speed techniques from Genesis Plus GX and BlastEm, and what they mean for Nubo

Clean-room reader report. It describes ideas and techniques only, in my own words. It contains no source code, no identifiers, tables, constants or timing values from either project. File names appear only under Provenance.

Genesis Plus GX (called GPGX below) and BlastEm emulate the Sega Master System, Game Gear and Mega Drive. Their Z80 is a relative of the Game Boy's SM83, and their video chip (the VDP) can be changed in the middle of a line, like the Game Boy's PPU. The two take opposite routes:

- **GPGX** is fast and accurate enough for nearly every game. It schedules by scanline: the CPU runs to the end of a line, the video chip draws whole lines, and a short list of special cases patches mid-line effects. Several of its shortcuts are approximations.
- **BlastEm** puts accuracy first. The video chip runs slot by slot (one memory-access slot at a time) as a resumable state machine, and every component catches up when it is accessed. Most of its speed comes from a JIT recompiler (out of scope for Nubo), from predicting interrupt times so the CPU need not poll the video chip, and from an optional render thread.

To ground the recommendations in Nubo's own costs, I profiled Nubo's release `nubo-benchmark` (the build at commit 3f6e3c7) with callgrind. I ran 30 emulated seconds of Tobu Tobu Girl (DMG), µCity (CGB) and Geometrix (CGB), plus 5-second runs that include the boot. I also tallied the I/O registers the games read, using a separate scratch crate outside the repository. Every number below is Nubo's, measured by me.

- **PPU (the run_dots loop and the pipeline functions it calls): 60 to 75% of all instructions.** Within that:
  - Mode 3 drawing is the bulk.
  - Even Nubo's fast "plain tile" path is heavy. About 5% of all Tobu Tobu Girl instructions go to working out each pixel's 2-bit color number bit by bit. Another 8 to 10% go to the fetcher's eight per-tile steps (finding the tile number, reading tile data, moving the FIFO state on).
  - Re-checking whether the PPU is "steady", the STAT line level and the LY=LYC flag on each call costs another 4 to 6%.
- **Copying the finished frame at vblank: 2.5 to 3.7%** (a 92 KB copy per frame).
- **CPU step, memory reads and writes, and the bus: 15 to 25%.**
- **APU: about 3%.** It is already cheap.
- **Nubo's own CGB boot ROM reads IF (0xFF0F) about half a million times** while it waits for vblank, and each read catches the PPU up. In gameplay, the games read STAT and LY only 15 to 50 times per frame.

## 1. Summary, ranked by estimated impact for Nubo

1. **Draw mode 3 in tight per-tile and per-line loops when nothing can watch the fetcher** (both emulators' line and column renderers). This includes a planar-to-chunky lookup table: a table that turns a tile row's two bit planes straight into pixel color numbers. Estimated 15 to 30% faster overall, because mode 3 is the largest single cost.
2. **Keep the PPU's next deadline instead of re-deriving it on every call** (both emulators keep "next event" times). Estimated 3 to 6%.
3. **Hand over finished frames by swapping two buffers, not copying** (BlastEm). Estimated 2.5 to 3.7%.
4. **A page-table memory map with every special mode folded into it** (both emulators' fast read and write pointer tables). Estimated 3 to 6%.
5. **Answer IF and LY reads from the predicted timeline without catching the PPU up** (BlastEm's predicted interrupt times; GPGX works out its status flags and counters from the clock). Estimated 1 to 3% in gameplay; large during the CGB boot and in games that poll in tight loops.
6. **A leaner instruction loop: one "anything special?" check per instruction, and flags computed without branches** (BlastEm's generated interpreter). Estimated 2 to 5%.
7. **Sound: Nubo already does what both do best** (edge-to-edge channel stepping with band-limited deltas). Only an optional, quality-changing kernel trim is left: about 1%, and not recommended.
8. **Optional on the desktop: draw pixels on a second thread by replaying a timestamped log of writes** (BlastEm's threaded renderer). Large potential, large complexity, and the browser build cannot use it easily.

Not for Nubo, because they trade accuracy for speed: GPGX's line-at-a-time scheduling with re-render heuristics, its counting of CPU time in whole instructions, its fixed approximations for mid-line display changes, and its fast linear-interpolated sound mode. BlastEm's JIT is excluded by Nubo's no-`unsafe` and wasm rules. Section 10 has the details.

Rough total if items 1 to 6 land: about 1.3x to 1.7x on the benchmark games. That would put Nubo near or past mGBA's numbers in `docs/EMULATOR_COMPARISON.md`, with exactness unchanged.

## 2. Drawing mode 3 in tight loops when nothing can watch the fetcher

### The concept

Both emulators split drawing a line into two parts: fetching what the line needs, and producing pixels. They make the second part as cheap as possible.

- **GPGX** draws a whole line at once into a line buffer of palette-index bytes:
  - It reads tile rows from a cache of tiles already decoded into one byte per pixel (and stored flipped as well), and writes four pixels at a time with word-wide operations (SWAR, "SIMD within a register").
  - It merges background and sprite layers through a precomputed priority lookup table, indexed by the two layers' pixel bytes.
  - It converts the line to output colors in a last pass, through a table of ready-made output colors that is updated only when palette memory is written.
  - The decoded tile cache is kept current by dirty tracking. A VRAM write marks the touched tile row dirty and adds the tile to a short dirty list. Before a line is drawn, only the dirty rows are decoded again.
- **BlastEm** keeps per-slot timing but does its pixel work a whole tile column at a time:
  - When a column's tile data has been fetched, it turns the bit planes into pixel color numbers with a planar-to-chunky lookup table (one table lookup per bit-plane byte instead of a loop over bits).
  - It then merges the eight background pixels with the sprite line buffer in a short loop.

The shared lesson: **per-pixel and per-step bookkeeping is where time goes, and it can be done once per tile or once per line whenever no one can observe the steps in between.**

### Where Nubo differs, and what it costs

Nubo's mode 3 is a SameBoy-port pixel FIFO, which is the right reference. Nubo already has three shortcuts: plain dots run without the phase machinery, a whole object-free tile row is drawn at once, and colors are cached per palette (Nubo already keeps converted colors, like GPGX's output color table). The profile shows two leftover costs in the plain-tile path:

- **Turning each tile row's two bytes into eight 2-bit color numbers bit by bit:** about 50 instructions per tile, around 5% of all Tobu Tobu Girl instructions.
- **Taking the fetcher's eight steps one by one:** each step updates the FIFO size, the position and the fetcher state as if each dot were visible. This is about 250 to 300 instructions per tile in total.

Lines with objects, the window, or a fine scroll at the line start go through the full per-dot pipeline (the other 11 to 15%).

### How it fits Nubo's bit-exact lazy rule

The key fact: **inside one PPU catch-up, the CPU does nothing.** Every register, VRAM byte and OAM byte the fetcher reads is constant for the whole stretch, except for what the PPU changes itself (HBlank DMA, which already stops the quiet window). The fetcher's intermediate states are visible only to:

- a CPU access, which forces a catch-up first;
- the dot-timed write glitches, which `write_timing.rs` flags and which already keep the PPU off the plain paths;
- the end state, if the stretch stops part-way through the line.

So, in three steps of increasing ambition:

1. **Planar-to-chunky lookup.** A 256-entry table that spreads one byte's eight bits into the eight pixel slots, combined for the low and high bit planes (or a 64K-entry table for both bytes at once), gives a tile row's eight color numbers in two lookups. This is a pure function, so it is exact by construction. It touches nothing stored, so save states are unaffected.
2. **Collapse the eight fetcher steps of a plain tile into one step.** Compute the tile-map address, read the tile number, read both data bytes, and set the FIFO, position and fetcher state directly to where the eighth step leaves them. Keep the debug assertion that the end state matches the step-by-step path. The checks Nubo already has decide when this is allowed. In particular, the step-by-step path must stay for any dot that carries a write glitch flag, and for CGB VRAM blocking during the fetch.
3. **Draw a whole line at once when the catch-up covers the rest of mode 3.** This is GPGX's model made exact. Use it when the line has no visible objects, the window cannot trigger on it, no glitch flag is set, and no HBlank DMA is running. Then the pixels are a pure function of the constant registers and VRAM, and mode 3's length is a pure function of the fine scroll. Work that length out from the existing pipeline's behavior, and verify it with the randomized scene tests; don't take it from any outside formula. Draw the line with item 1's lookup straight into the frame buffer, then set every pipeline field to the value the dot-by-dot path leaves at mode 3's end. Fall back to the current path when:
   - the catch-up ends before mode 3 ends;
   - the line has objects. A later stage could time the line with a timing-only pass of the existing pipeline that skips color work (BlastEm splits timing from pixel output this way), then merge an object line buffer through a priority rule.

### Estimated gain

Step 1 alone: about 3 to 5% overall. Step 2: another 5 to 8%. Step 3: a further 5 to 15% on object-free screens (menus, text, backgrounds), which is most of what the benchmark games show. Mode 3 is 35 to 45% of Nubo's instructions, so a 2x to 3x cheaper mode 3 gives 15 to 30% overall.

### Risks

- **Accuracy:** none if every fast path has to match the dot-by-dot path. Extend `gpu/tests/random_scenes.rs` to cover each new path (including fine scroll values, both LCDC tile areas, CGB attributes and flips, and catch-ups that end inside mode 3), and run `cargo benchmark --compare` between eager and normal builds.
- **Save states:** the lookup tables and any decoded tile cache are caches. Skip them in save states and rebuild them (a tile cache is simplest to rebuild in full on load). The pipeline fields the line path sets must be exactly what the stepwise path stores.
- **Complexity:** medium. Step 3 needs a precise list of pipeline end-state fields.

A full GPGX-style decoded-tile cache with dirty tracking is optional for Nubo. With the 2-byte lookup, decoding one Game Boy tile row is already about as cheap as reading it from a cache. The cache pays off mainly if flipped variants are kept, so CGB flip attributes cost nothing. Add it only if profiling after step 3 still shows decoding.

## 3. Keeping the PPU's next deadline instead of re-deriving it

### The concept

BlastEm's CPU loop compares one number per instruction: the cycle at which something must happen next. That cycle is the earliest of the run's end, the next predicted interrupt and the next non-maskable interrupt. It is recomputed only when something that affects it changes, such as a register write or an acknowledged interrupt. The video chip likewise works out how many cycles remain to the next line, the next horizontal interrupt and the frame end with closed-form arithmetic, instead of stepping toward them. GPGX keeps its sound chips' "next transition" times stored per channel. This is ordinary next-event scheduling: store the deadline, invalidate it on change, and never re-derive it on the hot path.

### Where Nubo differs, and what it costs

Nubo already predicts quiet windows (`Gpu::quiet_dots`) and steady stretches. But `run_dots` re-checks `has_steady_shape`, `stat_settled`, the STAT line level and the LY=LYC flag on every call and every stretch. These checks cost 4 to 6% of all instructions in the profiles: `has_steady_shape` alone is 2 to 3%, `stat_line_level` about 1 to 1.5% and `update_lyc_flag` about 1.2%. Their inputs only change:

- on CPU writes to PPU registers;
- at the line-timeline events Nubo already lists;
- when a countdown starts or ends.

### How it fits the rule

Store, alongside the PPU state, "steady until dot N of this line" and "STAT settled". Set them where a stretch ends or a timeline event runs, and clear them in every path that changes an input: register writes through `write_timing.rs`, the countdown starts, and LCD on or off. A debug assertion that the cached answer equals a fresh one (the same pattern `apu/catch_up.rs` already uses for cached predictions) keeps the cache honest. Results don't change, because only the cost of answering the same question changes.

### Estimated gain, risks

About 3 to 6%. Risk: a missed invalidation. The debug cross-check and the eager comparison catch it. The cache must not be saved: skip it and recompute it on load. Complexity is low to medium.

## 4. Swapping frame buffers instead of copying

### The concept

BlastEm keeps two frame buffers and swaps which one is drawn into at the end of each frame, so a finished frame is handed over without copying it. GPGX draws straight into the output buffer line by line.

### Where Nubo differs, and what it costs

At vblank the PPU copies its 160x144 RGBA drawing buffer into the completed frame: 92 KB per frame, 2.5 to 3.7% of all instructions in the profiles.

### How it fits the rule

Swap the two buffers at the point where the copy happens now. One subtlety keeps it exact: a pixel the PPU doesn't redraw in a frame must show what it showed before. Examples are lines left undrawn when the LCD turns off mid-frame, and the first frame after the LCD turns on, which Nubo already blanks. Track which lines were drawn this frame. At the swap, copy only the lines not drawn (normally none) from the outgoing frame into the new drawing buffer, so the next frame starts with the same contents as today.

The drawing buffer is part of the PPU's save state. A swap keeps the saved bytes identical only if those undrawn lines are copied as described. The cheapest correct form is to copy the not-yet-drawn rows lazily, just before the PPU first writes into each row of the new frame. With that, the drawing buffer always holds the same bytes as today, and fingerprints and save states don't change. Super Game Boy frames go through `Sgb::finish_frame` and need the same care.

### Estimated gain, risks

About 2.5 to 3.7%. Risk: low. Getting the "undrawn rows" rule wrong shows up as a frame fingerprint change in `cargo benchmark --compare`.

## 5. A page-table memory map with special modes folded in

### The concept

Both emulators split the CPU's 64 KiB address space into small pages. Each page has a direct pointer to its backing memory for reads (and another for writes), or a marker meaning "go through a handler". The fast path is one table lookup, one test and one memory access. Every special situation changes the table, not the hot path: bank switching, a boot ROM overlay, read-only areas (GPGX points writes to ROM at a dummy sink so a write never needs a branch), and devices that must see the access. The tables are rebuilt only when a mapper register or mode changes.

### Where Nubo differs, and what it costs

On every CPU read, Nubo's fast path:

- checks the address range;
- checks that OAM DMA isn't running;
- checks processor test mode;
- checks whether the boot ROM covers the address;
- looks up the ROM bank and does a banked read;
- checks for cheats;
- puts the address on the APU's bus;
- checks test mode again, to record bus activity.

In the profiles, `cpu_read_byte`, `read_byte_in_synced_cycle` and `peek` together cost 6 to 10% of all instructions.

### How it fits the rule

Keep, in the address bus, a 16-entry (4 KiB pages fit the Game Boy's map) or 64-entry table. Each entry says "plain: region and offset" or "slow". It is safe Rust: an index into Nubo's existing arrays, not a pointer.

- **Mark a page slow** while OAM DMA is in progress, in processor test mode, where the boot ROM is mapped, when a cheat targets an address in the page, and for everything outside ROM, work RAM and HRAM.
- **Rebuild the affected entries** whenever one of those conditions changes, and on bank switches (Nubo's `update_rom_view` is the natural place).

The fast path then does exactly what the current one does for plain memory. Recording bus activity in test mode is handled by test mode making every page slow.

The APU bus-address store per access must stay for exactness, because the wave channel glitch reads it. One option is to write it only when the wave channel can read the bus: keep a flag the APU sets and clears. With a debug check that the flag is right, this is exact.

### Estimated gain, risks

About 3 to 6%. The risk is a stale table entry after a bank or mode change. Debug builds can check every fast read against the full `peek` path. The table is derived state: skip it in save states and rebuild it on load (and in `clone_with_rom`).

## 6. Answering IF and LY reads from the predicted timeline

### The concept

BlastEm never runs the video chip just to learn whether an interrupt is pending. It asks the video chip, in closed form, at which cycle the next vertical and horizontal interrupts will fire. The CPU compares its own cycle count against that and takes the interrupt when it gets there. It recomputes the prediction only after a register write or an acknowledgement. GPGX answers several status reads from the CPU's cycle count within the current line: the horizontal blank flag, the horizontal counter, and whether the frame interrupt flag is already set at this point of the line.

### Where Nubo differs, and what it costs

Nubo counts IF (0xFF0F), LY and STAT among the addresses that reach the PPU. Any read of them runs the waiting M-cycles and catches the PPU up. This is always correct, but:

- Nubo's own CGB boot ROM reads IF about half a million times while waiting for vblank, each read followed by a catch-up of a few dozen dots;
- games that poll LY or STAT in tight loops split the PPU's work into tiny stretches, which keeps it off its bulk paths and pays the per-call overhead each time.

In steady gameplay of the three benchmark games, these reads are only 15 to 50 per frame, so the gain there is small.

### How it fits the rule

- **IF:** Nubo's quiet window already guarantees that no PPU interrupt request can come within the dots the PPU is owed, and `bulk_cycles_available` already guarantees that the waiting M-cycles request no timer or serial interrupt. So while the owed dots plus the waiting M-cycles' dots are within the quiet window, the IF value the CPU would read equals the stored IF. The read can be answered without running anything.
- **LY:** LY changes only at fixed points of the line timeline, never because of how long mode 3 takes. While the LCD is on and nothing pending changes it, the value at "current dot plus owed dots" can be computed from Nubo's own timeline rules: line length, the line-153 quirk and the per-model line-start dots, all Nubo's own constants. It must equal what a catch-up would show. The simplest safe version answers only while the owed dots stay within the current line's steady part. Otherwise it catches up.
- **STAT's mode bits:** these depend on where mode 3 ends, which is known only after running it. Answer them without a catch-up only in HBlank, VBlank and OAM scan stretches, where the next change is at a known dot. Section 2's line drawing makes mode 3's end known earlier, which widens this.

Each shortcut needs a debug assertion that compares its answer with the caught-up value, plus the eager comparison.

### Estimated gain, risks

About 1 to 3% on the benchmark games, much more for the CGB boot and for polling-heavy games. Risk: the rules for when a read may skip the catch-up must be exactly as narrow as the guarantees. The debug cross-check makes mistakes loud. Nothing is stored, so save states are unaffected.

## 7. A leaner instruction loop and flags without branches

### The concept

BlastEm's newer Z80 interpreter is generated from a description of each instruction as a bit pattern with register fields:

- The generator makes one specialized handler per concrete opcode, with register choices and constants folded in.
- Dispatch goes through a table of handlers, or a computed jump.
- The loop does one comparison per instruction against the next deadline (section 3). Interrupts are checked only when that deadline is crossed.
- Flags are kept in separate fields where they are cheapest to produce: the last result for sign and zero, a carry field, a parity or overflow field, and so on. The flag byte is assembled only when an instruction reads it as a whole, for example when pushing AF.

GPGX's Z80 uses a big switch with precomputed flag lookup tables. Those tables are large and cache-unfriendly on modern CPUs, so arithmetic is better.

### Where Nubo differs, and what it costs

Nubo's `Cpu::step` is 4 to 9% of all instructions. Per instruction it checks:

- whether a Super Game Boy holds the CPU in reset;
- whether the CPU is locked up;
- the run state;
- whether an HBlank DMA is in progress;
- processor test mode;
- the HALT bug;
- the EI delay;
- the boot ROM exit;
- the opcode after HALT;
- whether an interrupt is pending.

Flags are set one at a time through branches. Rust's `match` on the opcode already compiles to a jump table, and the register helpers inline to constants, so BlastEm's per-opcode specialization brings little to Nubo.

### How it fits the rule

- **Fold the rare checks into one flag.** Keep a "slow instruction start" flag in the CPU. Set it whenever any rare condition becomes true: SGB reset hold, lock-up, a non-running state, HBlank DMA in progress, test mode, a pending HALT bug or EI delay, the boot ROM mapped. Clear it when all are false. The common path then tests one flag. The pending-interrupt check stays as it is (IME first, so it is already cheap).
- **Compute F in one assignment per ALU instruction,** from the wide result with shifts and masks, instead of four conditional bit updates.

Both are internal rewrites with identical results. `cargo cpu-tests` checks every opcode cycle by cycle.

### Estimated gain, risks

About 2 to 5%. Low risk with the CPU tests. Keep `Registers::f` as the stored form, so save states are unchanged. Split flag storage (BlastEm's way) would add a conversion on every save and load, and isn't worth it for the SM83's four flags.

Threaded dispatch, meaning tail calls between handlers or a computed goto, isn't available in stable Rust (guaranteed tail calls are unstable). Pre-decoding doesn't help one-byte opcodes. A JIT is excluded. An interpreter could still borrow a JIT's benefit by caching decoded blocks of straight-line code, but for the SM83 (one-byte opcodes, cheap decode) the decode step is not where Nubo's time goes, so this is not recommended.

## 8. Sound: Nubo already matches the better design

### The concept

GPGX does not step its square/noise chip tick by tick. For each channel it keeps the time of the channel's next output edge. It jumps from edge to edge up to the requested time, and at each edge adds only the change in level (a delta), at its exact timestamp, to a band-limited buffer (Blargg-style band-limited synthesis). A volume change is queued as a pending delta and applied at the next update point. Reading the buffer integrates the deltas, applies a gentle high-pass filter and produces output samples. The CPU's writes catch the chip up to the write's time first. Timestamps are counted within the frame and rebased at frame end.

BlastEm does the simpler thing: it steps the chip once per internal tick, puts each tick's mixed level through a one-pole low-pass filter, and linearly interpolates to the output rate. That costs work on every tick and gives lower quality.

### Where Nubo stands

Nubo's `apu/catch_up.rs` already uses the GPGX scheme. Batches are cut only where a channel's output can change, each change goes into a band-limited step (BLEP) buffer at its sub-sample position, and the result is exact against M-cycle stepping. The APU is about 3% of instructions.

The one remaining difference: Nubo's kernel is 64 output samples wide, in floating point, against a much narrower integer kernel in GPGX, which also has an optional cheaper "fast" mode. A narrower kernel would make each output change a few times cheaper. But it changes the sound: slightly more aliasing, and different sample bits, so fingerprints change. It is a quality choice, not an emulation-accuracy one, and saves about 1%. Not recommended unless someone wants it as a setting. Building the shared BLEP table once per process is already done with a lazily built static. It shows up in short profiles only as a one-off startup cost.

## 9. Optional, desktop only: a second thread for pixels

### The concept

BlastEm can run two copies of its video chip:

- **On the emulation thread, a timing-only copy.** It keeps everything the CPU can observe (status, counters, interrupts, DMA, sprite evaluation results) and draws nothing.
- **On a render thread, a full copy.** It replays a log of every register, VRAM and palette write, each stamped with its cycle, and draws the pixels.

The two produce identical results because the renderer's output never feeds back into what the CPU sees, and both copies see the same writes at the same times.

### Fit for Nubo

On the Game Boy, mode 3's length depends on objects, the window and the fine scroll, so the timing copy still has to run the pipeline's fetch timing. Only the pixel and color part moves to the other thread: popping FIFO pixels into colors, priority mixing and writing the frame buffer. The CPU-visible timing stays on the main thread. CGB palette writes would be replayed from the log, as BlastEm does.

The browser build would need shared-memory threads, which need cross-origin isolation, so the web version would keep the single-thread path. Save states must be taken at a point where the renderer has caught up with the log.

### Estimated gain, risks

Up to the pixel share of the PPU, perhaps 15 to 25% of the work, moved to another core, after sections 2 and 3 have shrunk it. The complexity is high: two code paths that must agree, plus synchronization. Consider it only after everything above.

## 10. What not to adopt (trades accuracy for speed)

- **Line-at-a-time scheduling with re-render patches (GPGX).**
  - The CPU runs to the end of a line, and the video chip draws the whole line at once.
  - Writes that land in the horizontal blank re-draw the line.
  - A display-disable in mid-line blanks a pixel range worked out from the write's cycle.
  - Sprite limits after such tricks are reduced by a fixed amount, which the source itself calls inexact.

  Nubo's PPU is exact to the dot. The exact form of the idea is section 2 step 3: draw a line at once only when no write can land inside it.
- **Counting CPU time in whole instructions (GPGX's Z80).** Cycles are added per instruction from cycle tables, with special handling for a few instructions, so the other chips can't see where inside an instruction an access happens. Nubo's per-M-cycle accesses must stay.
- **Status values from per-line cycle tables (GPGX).** The idea of answering reads from time is good (section 6), but only computed from Nubo's own calibrated timeline and checked against a real catch-up.
- **Fast linear-interpolated sound (GPGX option) and BlastEm's low-pass-and-interpolate resampler.** Lower quality than Nubo's BLEP output.
- **BlastEm's JIT.** It needs `unsafe` and generated machine code, and doesn't work on wasm32.
- **Large flag lookup tables (GPGX's Z80).** Not inexact, just not faster on modern CPUs. Prefer arithmetic.

## 11. Suggested order of work

1. **Planar-to-chunky lookup in the plain-tile path** (section 2, step 1). Small, isolated, exact by construction.
2. **Frame buffer swap with lazy copying of undrawn rows** (section 4). Small and measurable.
3. **Cache the PPU's steady and settled answers** (section 3), with debug cross-checks.
4. **Collapse the fetcher's eight steps for plain tiles into one** (section 2, step 2). Extend `random_scenes.rs` first.
5. **Page-table fast path for CPU reads and writes** (section 5), with a debug check against `peek`.
6. **Fold the instruction-start checks into one flag, and compute flags without branches** (section 7). Run `cargo cpu-tests`.
7. **Draw object-free, window-free lines at once** (section 2, step 3). The biggest step: write the end-state field list first, and use randomized tests with catch-ups ending at every dot of mode 3.
8. **Answer IF and LY reads without catching up while the quiet window allows** (section 6). Measure the CGB boot before and after.
9. **Later, if wanted:** a timing-only pass for lines with objects, and the desktop render thread (section 9).

After each step: `cargo test-roms`, `cargo benchmark --compare` against a recording made before it, an eager-against-normal comparison, and `cargo save-states` where stored fields could be affected. None of the steps above should change what save states store.

## 12. Provenance

- **Genesis Plus GX:** https://github.com/ekeeke/Genesis-Plus-GX, shallow clone of commit 49c584764893b0505ac7f768a754f97330fa4392 (2026-10-06).
  - Licenses:
    - The main code uses a non-commercial license, which is incompatible with the GPL.
    - The Z80 core is MAME-derived freeware for non-commercial use.
    - The band-limited buffer library is LGPL-2.1-or-later, by Shay Green.
    - The optional FM cores are LGPL.

    Only ideas were taken, and nothing was copied.
  - Files read: `LICENSE.txt`, `core/system.c` (the Master System frame loop), `core/vdp_ctrl.c` (the mid-line write handling, status and counter reads, and dirty tile tracking), `core/vdp_render.c` (the caches, lookup tables, Mode 4 background and sprite renderers, line render and color remap), `core/z80/z80.c` (header, dispatch, flag tables, run loop), `core/memz80.c` (port handlers), `core/sound/psg.c`, `core/sound/sound.c` (FM glue), the head of `core/sound/blip_buf.c`, and grep results from `core/cart_hw/sms_cart.c` (page table setup).
- **BlastEm:** the official Mercurial repository at https://www.retrodev.com/repos/blastem could not be reached: the session's proxy refused the host (HTTP 403), and Mercurial isn't installed. I used the GitHub mirror https://github.com/libretro/blastem, shallow clone of commit ab743653ed110eeb7b1fe64d00cc5f9279c2bfec (2026-10-10). License: GPL-3.0-or-later (`COPYING`), which is compatible with Nubo, but still only ideas were taken.
  - Files read: `COPYING`, `README` (head), `psg.c`, `psg.h`, `render_audio.c`, `sms.c` (I/O handlers and run loop), `vdp.c` (run entry points, the render thread, the timing-only and Mode 4 slot state machines, the inactive-period loop, the Mode 4 fetch and render, interrupt prediction), `z80.cpu` (register, flag and instruction descriptions), `z80_util.c`, `cpu_dsl.py` (flag handling and the generated run loop), and `genesis.c` (component synchronization).
- **Nubo (read only, nothing edited):** commit 3f6e3c7fc29d5837835c6650551c8964e99e6935. Read: `AGENTS.md`, `crates/nubo-core/AGENTS.md`, `docs/EMULATOR_COMPARISON.md`, and these files under `crates/nubo-core/src/`:
  - `address_bus.rs`, `address_bus/bulk_cycles.rs`, `address_bus/dma.rs`, `address_bus/write_timing.rs`;
  - `cpu.rs`, `cpu/opcodes.rs`, `cpu/microops.rs`, `cpu/interrupts.rs`;
  - `gpu.rs`, `gpu/steady.rs`, `gpu/pipeline/plain_pixels.rs`, `gpu/pipeline.rs` (parts), `gpu/palettes.rs` (parts), `gpu/save_state.rs` (parts);
  - `apu/catch_up.rs`, `apu/blep.rs`;
  - `emulator.rs` (public API).
- **Measurements:** callgrind on the existing release `nubo-benchmark` (5 and 30 emulated seconds; Tobu Tobu Girl DMG, µCity CGB, Geometrix CGB). I/O read tallies came from a throwaway crate in the scratchpad that depends on `nubo-core` by path and steps the public `Emulator` API. Profiles and the crate were kept in a temporary folder only. Some line-level numbers come from callgrind outputs with source-line information that were already in that `prof` folder (from another session's run of the same commit); I only read them.
