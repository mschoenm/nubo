# Speed techniques in Mesen2 and Nestopia UE, and what they mean for Nubo

A clean-room "reader" report: it describes ideas and techniques only, not code. Mesen2's Game Boy core and both projects' NES cores were studied. Nubo's matching parts were read and profiled, so each technique comes with what Nubo does today and what that costs.

## How Nubo spends its time today

I measured Nubo before ranking anything, because the ranking depends on where the instructions go. Method: `nubo-benchmark` built in release from Nubo HEAD `3f6e3c7`, each benchmark game on DMG and CGB-E for 10 emulated seconds, under Valgrind's callgrind (instructions are deterministic, unlike wall time on a shared machine).

- **The PPU dominates.** The PPU catch-up (`Gpu::run_dots`, with everything it calls) accounts for 60–69% of all instructions on Geometrix, 63–80% on Tobu Tobu Girl and 63–87% on µCity. That works out to about 25–33 instructions per PPU dot on average. Mode 3 rendering is most of it: the plain-pixel and tile paths, the fetcher steps (finding the tile number, reading tile data), decoding each pixel's color number from the two bit planes, and the per-call checks (`has_steady_shape`, `stat_settled`, the countdown checks, `quiet_dots`).
- **The APU is cheap already**, at 1.7–3.5% inclusive. Its lazy catch-up works.
- **The CPU and memory map cost about 10–15%.** The hot spots are the address classification (`reads_plain_memory` alone is 2.7%), the boot ROM check on every ROM read (0.6%), a read path that isn't inlined (`read_byte_in_synced_cycle`: about 3% in call overhead), and the per-instruction checks in `Cpu::step` (about 5%).
- **The frame copy costs 2.3–4.4%.** This is the `memcpy` of 92 KB per frame in `publish_frame`.
- **Small catch-ups cost about 1.5 times as much per dot as large ones.** In µCity on CGB, about 500,000 CPU reads of IF (0xFF0F) in 10 s each made the PPU catch up a few dots. I ran a throwaway experiment in a scratchpad copy that stopped IF reads from catching the PPU up. The same PPU dots then cost 348 M instructions in large stretches instead of 530 M in small ones, and the run's total fell by 193 M (−8.8%). The saving was the same 193 M on all three games on CGB and zero on DMG. It comes from Nubo's own CGB boot ROM, whose `WaitFrame` polls IF in a tight loop during the boot animation. Every game that polls IF, LY or STAT pays the same penalty. In this experiment the fingerprints (frames, audio, end state) of all six 30-second runs were identical. The naive change is not exact in general (see technique 1).

One finding stands on its own. **Mesen2's Game Boy core is an eager design:** it steps the PPU, timer and DMA every half M-cycle, and its only shortcuts are an idle countdown in HBlank/VBlank and an APU that catches up between channel timer events. Nubo is already further along on the Game Boy side, so its useful lessons come from the two NES cores, together with Mesen2's memory map.

## 1. Summary, ranked by estimated impact for Nubo

The gains are estimated shares of instructions on the benchmark games. They don't simply add up.

1. **Interrupt-flag horizon (don't catch the PPU up for IF/IE reads that can't see a change)**: measured at −8 to −9% on CGB 10-second runs (−3% on 30-second runs), more on games that poll IF. Exact.
2. **Cheap catch-ups: cached steady horizon and a single pending-effects flag**: 4–8%, more on games that poll LY/STAT or write PPU registers often. Exact.
3. **Decode tile rows once, through a lookup table, and keep the decoded row with its colors**: 3–6%, and it removes most of the penalty for small catch-ups in mode 3. Exact.
4. **Per-line fetch context (an incremental fetch address, like the NES's internal VRAM address)**: 3–5%. Exact.
5. **Swap frame buffers instead of copying**: 2.3–4.4%. Exact, with care for frames that weren't fully drawn.
6. **Page-table memory map, with the boot ROM and test mode mapped as pages**: 3–5%. Exact.
7. **Deadline-driven CPU inner loop, with rare checks moved to deadline boundaries**: 3–5%. Exact.
8. **Reading LY (and the STAT mode) from the timeline without running the renderer**: 0–4% on these games, more on LY-polling games. Exact, but risky.
9. **APU: merge same-moment output changes, optionally with an integer BLEP**: 1–2%. Exact between lazy and eager, but it changes the audio bits once.
10. **Skip output work for frames the frontend throws away (RetroArch run-ahead)**: large in run-ahead, zero in the benchmark. It changes output, not emulation. Optional.
11. **Timestamped retroactive events for cartridge timers**: little relevance on the Game Boy, where Nubo already does this for the MBC3 clock.

Techniques in the studied code that **trade accuracy for speed, which Nubo must not adopt**, are listed at the end of section 2.

## 2. The techniques

### Technique 1: Interrupt-flag horizon

**The problem.** A lazy PPU is only cheap while nothing forces it to catch up. Nubo forces a catch-up before every CPU access in `reaches_ppu`, which includes IF and IE. A program that waits for VBlank by polling IF therefore makes the PPU run in slices of a few dots, one per loop iteration. Each slice pays the full per-call overhead, and slices are too short for the 8-pixel tile path.

**What the studied emulators do.** Neither catches up for an interrupt line when it doesn't have to. Nestopia never steps its PPU in the CPU loop. The PPU works out when it will next assert its interrupt (the NMI at the start of VBlank), hands that moment to the CPU as a timestamp, and is otherwise only caught up when the CPU touches one of its registers. Mapper interrupt counters likewise report "this interrupt line goes up at cycle T", and the CPU's interrupt poll compares the timestamp with its own poll point. Mesen2's NES APU applies the same test every CPU cycle: it only runs if a frame-sequencer step or a DMC interrupt can fall inside the elapsed window, and otherwise lets the window grow. The common idea: **know the earliest moment a component can change something a reader looks at; reads before that moment don't need the component caught up.**

**How it fits Nubo.** Nubo already computes how long the PPU can be left behind (`Gpu::quiet_dots`). When IE has the STAT interrupt disabled, that horizon deliberately ignores STAT interrupt requests, because "only reads of IF see them", and that is exactly why IF reads must catch up. The fix is to keep a second horizon next to `ppu_quiet_dots`: the dots during which the PPU can't request **any** interrupt (VBlank or STAT), whatever IE says. Nubo's existing quiet-dots logic already works this out when it is called as if the STAT interrupt were enabled. On a CPU read of IF:

- If the owed PPU dots end before that interrupt horizon, the PPU can't have set an IF bit in them. Return IF as it stands (after running the deferred M-cycles, as now) and leave the PPU behind.
- Otherwise catch up as today.

A read of IE needs no catch-up at all, since IE is plain state the PPU doesn't change. Only writes to IE change how long the PPU may be left behind, so 0xFFFF stays a catch-up point for writes. A write to IF still catches up: it can clear a bit the PPU is about to set, and the order of the two has to be right.

**Invariants.** The PPU's contribution to IF over the owed dots is exactly "no change" when no request can fall in them. All other IF sources (timer, serial, joypad) are already brought up to date by `run_deferred_cycles`. The horizon has to be recomputed (or invalidated) whenever something that shapes it changes: STAT, LYC, LCDC, an HBlank DMA starting, or a speed switch. Those are all writes that already catch the PPU up, so they can refresh it.

**Cost today and gain.** The naive experiment, which dropped IF from the catch-up set altogether, removed 193 M instructions per CGB boot. That is −8% to −9% of a 10-second CGB run, about −3% of a 30-second one, plus whatever polling games gain. It gave identical fingerprints on all six benchmark runs, but only because the boot ROM and these games don't use STAT interrupts with IE's STAT bit off while polling IF. The horizon version is exact in all cases.

**Risks.** These are small. The new horizon must be at least as conservative as running dot by dot: test it with `gpu/tests/random_scenes.rs` (add IF reads at random dots and compare against the eager build) and with `cargo benchmark --compare` against an `eager` build. Save states are untouched, since the horizon is a cache.

### Technique 2: Make catch-ups cheap (cached steady horizon, single pending-effects flag)

**The problem.** Even with technique 1, the PPU must catch up before every LY/STAT/VRAM/OAM access and every timed write. Nubo's `run_dots` re-derives its situation from scratch on every call and every loop step. It works out whether the coming dots are steady (`has_steady_shape`, with many field comparisons), whether STAT is settled (`stat_settled`, which recomputes the STAT line level), the countdown checks (`Countdowns`, each checked on every ticked dot), and at the end how long it may stay quiet (`quiet_dots`). Together these are about 5–7% of all instructions, and they are why small slices cost about 1.5 times as much per dot as large ones.

**What the studied emulators do.**
- **Nestopia: a resumable dot-indexed state machine.** The PPU's line is one long piece of straight-line code with an entry label for every dot. A catch-up jumps straight to the current dot and runs forward. After each dot the only check is "has the target been reached?" What each dot does is fixed by where it is in the line, so no decisions about what kind of dot this is are made at run time. Rendering-enabled and rendering-disabled lines are two separate machines; changing the rendering flags goes through a register write, which catches the PPU up first. Stretches where nothing happens (most of a line with rendering off, the gap to the end of a line) are crossed in one jump to the next dot that does something. At the start of VBlank it jumps to the end of the frame.
- **Mesen2: a single "something is pending" flag.** All the rare delayed effects (a register write that lands a dot later, a delayed address increment, rendering switched on or off taking effect a dot late) are gathered behind one boolean. The per-dot path tests that one flag, and only when it is set does a slower routine work through the pending effects and decide whether it is still needed. Mesen2's Game Boy PPU also keeps an idle countdown: when HBlank starts, or early in a VBlank line, it computes how many dots remain until anything happens and then only counts them down.

**How it fits Nubo.** Two exact refinements:

- **Cache the steady horizon.** When a steady stretch ends, compute the absolute dot at which the steadiness can next end and keep it. While no write, LCD switch or countdown has invalidated it, later calls skip `has_steady_shape` and `stat_settled` and run straight to `min(target, horizon)`. Every way to break steadiness goes through a PPU write (which catches up first), a countdown start or a line or mode change inside the PPU itself, so invalidation points are easy to find. Debug builds can assert that the cached answer equals the freshly computed one, like the APU's existing debug checks.
- **One "anything counting down" summary.** Keep a bitmask or flag that is non-zero while any `Countdowns` entry (or a line-state countdown) runs, set when a countdown starts and cleared when the last one reaches zero. `tick_dot` then tests one value instead of every countdown, and `has_steady_shape` gets cheaper too. A related idea: store all countdowns as "the dot at which this fires" instead of counters decremented every dot. The per-dot work then becomes one comparison with the earliest firing dot (see technique 11).

**Invariants.** A cached horizon is valid only while every input that `has_steady_shape`, `stat_settled` and `quiet_dots` read is unchanged, apart from the dot counter. The summary flag must be set exactly when at least one countdown is non-zero.

**Gain.** About 4–8% on the benchmark games; more where games poll LY/STAT or write PPU registers often (Geometrix on DMG spends 9% of its instructions in catch-ups for writes and non-IF reads).

**Risks.** Moderate. A missed invalidation silently gives a different result, so add debug assertions and random-scene tests with writes at random dots. Save states are unaffected: caches are left out with `#[serializable(skip)]` and rebuilt after loading, or simply invalidated on load.

### Technique 3: Decode each tile row once, through a lookup table, and keep the decoded row

**The problem.** Nubo's tile path (`draw_background_tile`) works out each pixel's color number by extracting one bit from each of the two bit planes, eight times per row, and then looks up the color. That line alone is 3.3% of instructions on µCity CGB, and the pixel stores another 1.5%. The single-pixel path (`draw_plain_pixel`), used whenever a catch-up ends mid-tile or an object is near, pops the FIFO bit by bit and works the color out per pixel.

**What the studied emulators do.**
- **Nestopia: a tile lookup table.** A table built once at start-up maps (pattern byte, attribute bits) to the already-decoded pixels of that byte. Loading a tile's 8 pixels into the background buffer is then a few table reads. The per-dot output is "take decoded pixel at position (dot + fine scroll), mask it, look up the output color": no bit work per dot.
- **Nestopia: bit-plane interleaving for sprites.** When a sprite's two pattern bytes are fetched, they are combined once with a few masks and shifts ("spreading" each byte's bits so the two planes interleave into 2-bit pairs), which yields all 8 two-bit pixels at fetch time. The decoded pixels are stored per sprite, in the order the sprite will output them (the flip is applied once here).
- **Mesen2: colors precomputed at palette-write time**, so the per-pixel output is a palette index lookup. Nubo already does this (`Palettes` keeps RGBA per entry).

**How it fits Nubo.**
- **A spread table.** Use a 256-entry table that spreads one byte's 8 bits into the even bits of a 16-bit word. One row's 8 color numbers are then `spread(low) | spread(high) << 1`: two lookups, one shift and one OR per row instead of 8 bit extractions per plane. The same works for objects at fetch time, with the X flip applied to the spread result by a second table (or a bit reverse) once per fetch.
- **Keep the decoded row in the FIFO.** When the fetcher pushes a row into the background FIFO, also store the 8 decoded color numbers, plus the 8 final RGBA colors if the palette inputs (BGP or the CGB palette and attributes, and the BG enable bit) are known to stay fixed while the row is drawn. A palette or LCDC write goes through `write_timing.rs`, which catches the PPU up first. Such a write simply marks the precomputed colors as stale for the pixels not yet drawn, which then fall back to looking up the colors again. With this in place, a catch-up that ends mid-tile no longer drops to a slow path: the next call continues from the same decoded row. This is the GB counterpart of Nestopia making slices cheap.

**Invariants.** The decoded row must be bit-identical to what popping the FIFO's two bit planes gives, including the DMG glitch pixel the FIFO inserts and CGB attribute effects. The stale marking must cover every write that can change a color mid-row (BGP/OBP0/OBP1, the CGB palette data registers, LCDC bit 0, and on CGB the priority bits).

**Gain.** 3–6%. It also shrinks the penalty for small slices, which helps polling games more than the numbers here show.

**Risks.** Low to moderate. If the FIFO structs gain fields, check how they are stored: the pipeline FIFOs have hand-written save state implementations, so the decoded row can be left out and rebuilt from the bit planes on load. Use `random_scenes.rs` with palette writes at random dots.

### Technique 4: A per-line fetch context, like the NES's internal VRAM address

**The problem.** The fetcher's address work is about 6–8% of instructions on CGB (`find_tile_number` 2.3%, `read_tile_data_high` 2.5%, plus the low byte, the address arithmetic and the attribute checks). Every step recomputes the tile map address from SCX, SCY and LY or the window line, the tile data address from LCDC's tile data bit, the tile number and the fine Y, and on CGB the bank, flip and attributes, plus the STOP and VRAM blocking checks.

**What the studied emulators do.** On the NES, the hardware itself keeps one internal VRAM address register that encodes coarse X and Y, fine Y and the nametable. Both emulators fetch with just that register: the name-table address, the attribute address and the pattern address come from it with a mask or two. Once per tile they step coarse X, and at the end of the line fine Y. The cost per tile is a handful of operations, because the expensive combination of scroll and line is done incrementally instead of from scratch.

**How it fits Nubo.** The Game Boy has no such register, but an emulator can keep an equivalent derived value. At the start of the line, and whenever a write to SCX, SCY, LCDC, WX/WY or VBK changes an input, compute the tile-map row base, the fine Y offset (flipped or not), the tile data base and the signed/unsigned addressing mode. Then step the map column per tile. The exceptions are the cases the SameBoy port models on purpose: SCX/SCY read at particular fetcher steps, mid-fetch LCDC and tile-select glitches, and the window starting. These must keep their exact reads. The cache is therefore just a fast way to get "the address the fetcher would compute now". Any write to an input invalidates it, and the write already catches the PPU up to the dot first.

**Invariants.** The cached address must equal the from-scratch computation at every fetcher step. Keep the from-scratch version, and in debug builds compare the two on every fetch.

**Gain.** 3–5%.

**Risks.** Moderate: the fetcher's mid-line SCX/SCY/LCDC timing is calibrated, and a cache that ignores one late read changes results. That is why the cache must be invalidated by writes rather than refreshed once per line.

### Technique 5: Swap frame buffers instead of copying

**The problem.** At every VBlank `publish_frame` copies the 160×144 RGBA frame (92 KB) into `completed_frame`. That is 2.3–4.4% of all instructions, and up to 6,000 copies a second when running at 100x.

**What the studied emulators do.** Mesen2 (both GB and NES) draws into one of two output buffers and, at frame end, hands the finished one to the video side and swaps which buffer the PPU draws into. No copy is made.

**How it fits Nubo.** Swap `frame_buffer` and the completed frame (keep them as two buffers of the same type and swap ownership). The catch: Nubo's PPU draws over the previous frame's pixels, and a pixel the next frame doesn't draw (the LCD switched off mid-frame, a frame ended early) would then show the frame from two frames ago instead of the last one. To stay exact, count the pixels drawn since the last publish. Swap when the frame was fully drawn, and copy (as today) when it wasn't. The blank-frame and Super Game Boy paths stay as they are.

Save states store `frame_buffer` today, so the swap must not change what gets stored: after a swap, the drawing buffer's contents are what the next frame overwrites. Either keep storing the drawing buffer's contents (equal to a copy-based run only if the frame was fully drawn), or store the completed frame under the existing name. Check with `cargo benchmark --check-save-states`.

**Gain.** 2–4%.

**Risks.** Low. Frontends that keep a reference to the frame across calls aren't affected, since they borrow it.

### Technique 6: A page-table memory map

**The problem.** Each CPU read classifies its address by range several times: `reads_plain_memory` in `cpu_read_byte`, again in `read_plain_memory`, `reaches_ppu` on the slow path, and a match in `peek`. It also checks the boot ROM (`in_boot_rom && boot_rom_covers`), OAM DMA and processor test mode on every read. The classification alone is 2.7% of instructions, and the boot ROM check another 0.6%.

**What the studied emulators do.**
- **Mesen2 (GB and NES mappers):** a 256-entry table, one entry per 256-byte page, holds a direct reference to the backing memory for reads and another for writes (or "none" for read-only pages), plus a per-page flag saying "this page is registers, take the slow path". A read is one table lookup and one indexed load. The boot ROM is not checked per read: its pages are *mapped into the table* at power-on and *remapped* to the cartridge when the boot ROM is switched off. Bank switches, SVBK writes and similar remap the affected pages once.
- **Nestopia:** a table with one entry per address (component, read handler, write handler), filled at start-up and updated on bank switches. Each register has its own handler, so no handler has to work out which register it serves. Zero page and stack accesses skip the table entirely, because on the NES they can only be RAM.

**How it fits Nubo.** Without `unsafe`, the safe equivalent is a 256-entry page table whose entries say *which buffer* (cartridge ROM, work RAM, HRAM, boot ROM) and *what offset into it*, or "slow path". The fast read is a bounds-checked index into one of a few known slices, chosen by a small enum `match` that the compiler turns into a jump or a select. Remap pages when:

- the cartridge's ROM bank changes (Nubo already refreshes `rom_banks` after a ROM-area write),
- SVBK changes,
- the boot ROM unmaps (at that point the boot ROM pages simply become cartridge pages: no per-read check is left),
- OAM DMA starts and ends (mark every page DMA can conflict with as slow while it runs; this replaces the per-read `dma.in_progress` test),
- processor test mode is switched on (every page slow, or a separate test table).

Mark HRAM's page as slow, because 0xFF00–0xFF7F share it, and give HRAM a second small check, or treat page 0xFF as slow with an HRAM fast check inside the slow path. Cheats remain a per-read check that is cheap when the list is empty; Nubo already gates them.

**Second half: keep the hot read path small enough to inline.** `read_byte_in_synced_cycle` isn't inlined, and its call overhead is about 3%. That is because it carries the processor-test bus recording. Move that recording behind a cold, out-of-line branch. The cleaner option is a separate instantiation of the CPU loop for test mode (see technique 7), so the normal path has no test-mode check at all. Mesen2 does the same with its debugger: a compile-time "dummy" CPU variant for the debugger, and run-time "is debugging" flags tested once per access rather than spread through the access code.

**Gain.** 3–5% together.

**Risks.** Low. The page table is a cache of `rom_banks`, `svbk`, `in_boot_rom` and the DMA state: rebuild it after loading a save state, leave it out of what states store, and assert in debug builds that a page-table read equals `peek`.

### Technique 7: A deadline-driven CPU inner loop

**The problem.** `Cpu::step` re-checks many rare conditions on every instruction: held in reset, locked up, the run state, HBlank DMA, boot ROM exit, the HALT opcode, test-mode recording, pending interrupts. Together with the outer `audio_buffers_full` check that is about 5% of instructions.

**What the studied emulators do.** Nestopia's CPU runs instructions in a tight loop *until a deadline*. The deadline is the earliest of the next APU event, the next scheduled NMI, the next scheduled IRQ and the end of the frame. The loop body is "fetch, execute" and nothing else. All rare work happens in one routine at the deadline, which then sets the next deadline. Anything that creates new rare work (an interrupt scheduled by a mapper, a DMA starting, the PPU reporting VBlank) lowers the deadline so the loop exits in time. Nestopia also picks one of three copies of the loop (no per-instruction hooks, one hook, several hooks) once per frame, so a cartridge that doesn't need a hook doesn't pay for checking.

**How it fits Nubo.** Split `Cpu::step` into a fast path for "running, nothing unusual" and the general path. Have the address bus keep a single "attention needed" flag or deadline. It is set when an interrupt becomes pending with IME on, an HBlank DMA block falls due, the boot ROM exit is requested, the audio buffer fills or the dot limit is reached, and by HALT/STOP. The emulator's run loop then repeats the fast path (execute opcode, prefetch next) while the flag is clear, and falls back to today's full `step` when it is set. Processor test mode, SGB reset and lock-up are set up once, so the fast loop is never chosen while they apply.

**Invariants.** Every condition `step` checks must, when it becomes true, set the attention flag before the next instruction boundary. Interrupt dispatch timing must not move, because the check order inside the full `step` stays exactly as it is.

**Gain.** 3–5%.

**Risks.** Moderate. A missed flag delays an interrupt by an instruction, which the test ROMs (`cargo test-roms`, `cargo cpu-tests`) and the eager comparison catch quickly.

### Technique 8: Read LY (and the STAT mode) from the timeline without running the renderer

**The problem.** Many games wait for a line by polling LY, or for a mode by polling STAT. Each such read catches the whole PPU up, renderer included, in a small slice (technique 2 makes slices cheaper, but they still cost more than large stretches).

**What the studied emulators do.** Nestopia keeps two clocks for its PPU: a line base, advanced by a whole line at a time, and the dot within the line. This makes "where is the PPU now?" a matter of arithmetic. On the NES the line and dot are a fixed function of time, because rendering doesn't change line length, so the PPU's position can be known without running it. On the Game Boy this holds for LY, since lines are always the same length, but not for the end of mode 3, which depends on scroll, objects and the window.

**How it fits Nubo.** As an exact refinement for LY reads: when the PPU is behind but in a known-steady situation (LCD on, no LCD toggle or line-153 special case inside the owed dots, no pending write effect), compute the LY value at "now + owed dots" from the line and dot counters with the same rules `tick_dot` uses, and return it without catching up. A STAT read during HBlank/VBlank/OAM scan can do the same for the mode bits, provided the owed dots stay inside the current mode. A read whose owed dots reach into mode 3 or across the mode 3 end must catch up as today.

**Gain.** 0–4% on these games (Geometrix on DMG: up to about 4%). More on LY-polling commercial games.

**Risks.** Higher than the rest: it duplicates timing rules that are calibrated to the dot (LY's visible change point differs by model and speed, and line 153 is special). Do it only through shared functions that `tick_dot` itself uses, test it against the eager build, and run `cargo test-roms`. Lower priority than techniques 1–3.

### Technique 9: APU output, merging same-moment changes and an integer BLEP

**The problem.** The APU is only 1.7–3.5% of instructions, so this is small. Each M-cycle in which channel outputs change adds one 64-tap BLEP step per changed channel per side, so up to 8 steps when several channels change together.

**What the studied emulators do.**
- **Mesen2 (NES):** each channel records its output changes in its own per-channel delta buffer, indexed by the cycle they happened in. At the end of the audio frame the mixer walks the sorted, de-duplicated list of cycles at which anything changed. At each one it adds up all the channels' deltas, computes the mixed level once (through the NES's nonlinear mixing), and feeds one band-limited step per side. Mesen2's band-limited synthesizer works in integers, so adding deltas in any order is exact.
- **Mesen2 (GB):** the same structure on a smaller scale. The channels run between each other's timer events, and one combined step per side is emitted whenever the mixed level changes.
- **Nestopia:** instead of BLEP, it integrates the mixed level over each stretch between changes, weighting each level by its duration with a triangular window. Its cost is proportional to the number of level changes, not to filter taps.

**How it fits Nubo.** Within `process_channel_deltas`, sum the left and right deltas of all channels that change in the same M-cycle, and add one BLEP step per side. In `f32`, `(a + b) × k` isn't bit-equal to `a × k + b × k`, so the audio fingerprints change once. Lazy and eager builds still agree, since both run the new code. To keep sums exact regardless of order, move the BLEP buffer to fixed-point integers (as Mesen2's synthesizer does). That changes the stored type of the BLEP buffer, so save it under a new stored name per Nubo's rules (older states would then start that buffer from silence).

**Gain.** 1–2% at most.

**Risks.** Low, but it's an output change: record new fingerprints deliberately.

### Technique 10: Skip output work for frames the frontend throws away

**What the studied emulators do.** Mesen2 marks frames run for run-ahead as hidden. Its SNES and PC Engine cores then skip drawing pixels, and audio output is dropped. Its SNES/PCE cores also skip rendering while fast-forwarding. Nestopia chooses its audio update routine once per frame: when no audio output is wanted, the APU only processes the frame counter and the events that affect the CPU, and synthesizes nothing.

**How it fits Nubo.** RetroArch's run-ahead (which Nubo's libretro core supports) runs hidden frames whose audio and video are discarded. On the Game Boy the PPU's *timing* (mode 3 length, fetches, object penalties) must still run exactly, but the color lookups and frame-buffer stores could be skipped, and the APU could skip its BLEP synthesis. Emulation stays exact, but the frame buffer and BLEP buffer differ from a full run, and both are in Nubo's save states. Run-ahead saves a state after a hidden frame, so the next shown audio and video could differ slightly from a run without skipping.

This **changes output, not emulation**. Offer it only behind the libretro "audio/video disabled" signal, never in the benchmark or in `step_until_next_audio_buffer`. The video part is exact if every pixel of the next shown frame is redrawn (the same condition as technique 5).

**Gain.** Large in run-ahead (most hidden-frame rendering work), zero in the benchmark.

**Risks.** Moderate: it splits what "exact" means. This is a product decision more than a speed task.

### Technique 11: Timestamped retroactive events for cartridge and other counters

**What the studied emulators do.** Nestopia's mapper cycle counters don't count every CPU cycle. A per-instruction hook catches the counter up over the elapsed cycles and, if it fires, asserts the interrupt *with the exact cycle at which it fired*. The CPU's poll compares timestamps, so the interrupt is taken at the right instruction boundary even though it was found late. Both emulators handle the NES's PPU address line filter (which needs "how long has the line been low") by keeping the timestamp of the last edge and taking a difference when the next edge comes, instead of counting cycles in between. Both rebase their cycle counters by the frame length each frame, which keeps the arithmetic in 32 bits.

**How it fits Nubo.** Nubo already keeps the MBC3 clock as a timestamp, and computes TIMA's overflow and the APU's deadlines ahead of time. The general pattern applies to anything new: store "the time this fires" rather than a counter decremented every M-cycle. It also applies to the PPU's countdowns (technique 2). This makes "can a stretch run in bulk?" one comparison with the earliest stored time. No separate work item is suggested.

### Already present in Nubo

These are things the studied emulators do that Nubo already does, often more thoroughly: catching up on access; running the APU lazily between channel events; precomputing palette colors at write time; dropping objects with nothing to draw from per-dot work (Nubo checks the next object's X before each plain pixel); and skipping idle stretches (Nubo's steady stretches and quiet dots go further than Mesen2's GB idle countdown).

### Accuracy-for-speed trades in the studied code, which Nubo must not adopt

- **Doing a multi-dot fetch's work on one representative dot.** For example, loading a whole sprite tile on one dot of its fetch window, which the source itself calls an approximation. On the Game Boy, mode 3 length and mid-fetch writes depend on the exact step.
- **Moving an event a dot early "for performance"**, such as setting a bus address one dot before hardware does.
- **Heuristic frame pacing while the LCD is off**: sending a blank frame whenever more than a frame's worth of time has passed, instead of keeping the PPU's own phase. Nubo already keeps the phase exactly.
- **Removing or "adaptively" raising the sprite limit**, and frame skipping that alters what is displayed. These are user options that change output.
- **Randomizing the CPU/PPU alignment at power-on**: deliberate nondeterminism, the opposite of Nubo's reproducible runs.
- **Floating-point channel levels and per-channel volume settings mixed inside the core** (Mesen2 GB): harmless there, but they would break Nubo's bit-exact audio fingerprints if copied carelessly.
- **Mesen2's GB HALT handling**, which checks interrupts at one fixed point per M-cycle per model: an emulation shortcut that Nubo's calibrated mid-cycle sampling already does better.

## 3. Suggested order of work

1. **Technique 1 (interrupt-flag horizon).** Small, exact, measured −8 to −9% on CGB runs, and it helps every IF-polling game. Test with the eager comparison and random IF reads in `random_scenes.rs`.
2. **Technique 5 (frame buffer swap).** Small and isolated, 2–4%. Check the save-state layout with `cargo save-states`.
3. **Technique 6 (page table and a lean read path).** Mechanical, 3–5%. Build the debug assertion "page-table read == `peek`" first.
4. **Technique 2 (cached steady horizon and a countdown summary).** It makes every later PPU change cheaper to measure. Add debug assertions comparing cached and fresh answers.
5. **Technique 3 (spread-table decode and decoded rows in the FIFO).** It removes most of the small-slice penalty.
6. **Technique 4 (per-line fetch context).** Do it after 3, since both touch the fetcher. Debug-compare against the from-scratch addresses.
7. **Technique 7 (deadline-driven CPU loop).** Moderate gain. Do it once the PPU work is cheaper and the CPU share grows.
8. **Technique 8 (LY/STAT read-out)**, only if profiles of LY-polling games show it is worth the risk.
9. **Technique 9 (APU delta merging, integer BLEP)** and **technique 10 (run-ahead output skipping)** as separate, deliberate output changes.

After each step, follow Nubo's rule 10: record fingerprints on the benchmark games and the test ROMs before the change, compare after it, and compare a normal build with an `eager` one. Callgrind instruction counts on 10-second runs (as above) are a stable way to measure gains of a few percent on a shared machine.

## 4. Provenance

**Mesen2**: https://github.com/SourMesen/Mesen2, commit `b9fa69ddc6d0a331fb103fdb5eef6904305703c2` (2026-06-04). License: GPL-3.0 (`LICENSE`). Files read:
- `Core/Gameboy/`: `GbMemoryManager.h`, `GbMemoryManager.cpp`, `GbCpu.h`, `GbCpu.cpp`, `GbPpu.h`, `GbPpu.cpp`, `GbTimer.cpp`, `Gameboy.cpp` (run loop only), `APU/GbApu.h`, `APU/GbApu.cpp`, `APU/GbSquareChannel.h`, `APU/GbSquareChannel.cpp`
- `Core/NES/`: `NesCpu.cpp`, `NesConsole.cpp`, `NesMemoryManager.cpp`, `NesPpu.h`, `NesPpu.cpp`, `BaseNesPpu.h`, `BaseNesPpu.cpp`, `DefaultNesPpu.h`, `BaseMapper.h`, `BaseMapper.cpp` (read path), `Mappers/Nintendo/MMC3.h` (IRQ part), `Mappers/A12Watcher.h`, `APU/NesApu.h`, `APU/NesApu.cpp`, `APU/ApuFrameCounter.h`, `APU/ApuTimer.h`, `NesSoundMixer.cpp`
- `Core/Shared/Emulator.cpp` (run-ahead), `Core/SNES/SnesPpu.cpp` and `Core/PCE/PceVpc.cpp` (render skipping only)

**Nestopia UE**: https://github.com/0ldsk00l/nestopia, commit `7b5c87d8dc3c817211545449683d1fcd897324ca` (2026-09-13). License: the source file headers say GPL-2.0-or-later; the repository's `COPYING` contains the GPL-3.0 text. Files read:
- `source/core/`: `NstCpu.hpp`, `NstCpu.cpp` (run loops, interrupt scheduling, addressing helpers), `NstPpu.cpp` (catch-up, the line state machine, tile and sprite loading, sprite evaluation, pixel output), `NstApu.cpp` (update routines, scheduling, mixing), `NstTimer.hpp`, `NstIoMap.hpp`, `NstIoPort.hpp`

Both were shallow-cloned into a temporary folder and only read; no build scripts were run. Nothing from them was copied into the Nubo repository. This report holds no code from them, and none of their identifiers, constants or timing values.

**Nubo** (read and profiled, not changed): HEAD `3f6e3c7fc29d5837835c6650551c8964e99e6935`.
- Docs: `AGENTS.md`, `crates/nubo-core/AGENTS.md`, `docs/EMULATOR_COMPARISON.md`
- `crates/nubo-core/src/`: `address_bus.rs`, `address_bus/bulk_cycles.rs`, `address_bus/dma.rs`, `address_bus/write_timing.rs`, `address_bus/cheats.rs`, `cpu.rs`, `cpu/microops.rs`, `cpu/opcodes.rs`, `cpu/interrupts.rs`, `gpu.rs`, `gpu/steady.rs`, `gpu/pipeline/plain_pixels.rs`, `gpu/palettes.rs`, `gpu/save_state.rs`, `apu.rs`, `apu/catch_up.rs`, `apu/blep.rs`, `emulator.rs`
- `crates/nubo-core/boot_roms/nubo_cgb.asm`

Profiles: callgrind on `nubo-benchmark`, 10 emulated seconds per game and model. The IF experiment was built from a temporary copy of Nubo; the profiles were kept in a temporary folder only (`base.*` and `if.*`).
