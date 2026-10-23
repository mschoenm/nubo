# Why Gambatte runs 3 to 4.3 times as fast as Nubo: a clean-room report

This report describes ideas, algorithms and architecture only. It holds no Gambatte source code,
no pseudo-code that mirrors its functions, none of its tables, constants or timing numbers, and none
of its identifiers. Where a technique needs a table, it says what the table maps from and to; its
contents must be worked out from Nubo's own code or public hardware documentation. Every timing that
appears below is Nubo's own or general hardware knowledge (a line has 456 dots, a frame 154 lines).
The numbers about Nubo come from profiling Nubo at commit `3f6e3c7`.

## How Nubo was measured

`nubo-benchmark` was built in release mode with line tables, and Valgrind's callgrind ran it for 10
emulated seconds per game. That is the same measure `cargo xtask compare-emulators --instructions`
uses, so the totals match `docs/EMULATOR_COMPARISON.md` (Geometrix DMG: 200 M instructions per
emulated second here, 194 there).

| Millions of instructions per emulated second | Nubo total | Nubo PPU (`Gpu::run_dots`, inclusive) | Nubo APU | Nubo CPU, bus, timers, serial, scheduling | Gambatte total (from the comparison doc) |
|---|---|---|---|---|---|
| Geometrix DMG | 200 | 129 (64%) | about 7 to 10 | about 55 | 70 |
| Geometrix CGB | 246 | 144 (59%) | about 8 | about 85 | 92 |
| Tobu Tobu Girl DMG | 142 | 108 (76%) | about 8 | about 20 | 58 |
| µCity CGB | 219 | 136 (62%) | about 6 | about 70 | 84 |

What the profile shows:

- **The PPU alone costs more than all of Gambatte.** Nubo's PPU runs 108 to 144 M instructions per
  emulated second, which is 1.5 to 1.9 times Gambatte's whole emulator. Most of it is mode 3
  drawing. The plain-tile shortcut (`gpu/pipeline/plain_pixels.rs`, inclusive) alone is 23 to 34%
  of everything. That is about 35 instructions per pixel, or roughly 300 per tile, of which about
  80 go to stepping the fetcher (`find_tile_number`, `read_tile_data_*`, the FIFO bookkeeping) and
  the rest to taking each pixel's color index apart bit by bit. The per-dot path (`tick_dot`, its
  countdowns, `update_lyc_flag`, `update_stat_line`, `has_steady_shape`) and the per-line OAM scan
  (`tick_oam_scan`, `scan_oam_entries`) take the rest.
- **The CPU and bus cost about as much as all of Gambatte on the busy games.** That is 55 to 85 M
  instructions per emulated second on Geometrix and µCity. A plain ROM or RAM read costs about 40
  instructions spread over `read_byte_in_synced_cycle`, `cpu_read_byte`, `read_plain_memory` and
  `banked_read`, plus the cheat check. In Geometrix CGB, 578,000 reads in 10 s are I/O register
  reads, about 960 per frame. Each one runs the deferred M-cycles and catches the PPU up, so the PPU
  is cut into 3 to 7 pieces per line.
- **The APU is small**, 3 to 8% of the total. About a third of that is the forced catch-up at every
  DIV-APU edge (`div_apu_bit_changed`).
- Smaller items: a full frame copy each frame (`publish_frame` and memcpy, 2 to 4%), the serial
  port stepped in every bulk stretch with no transfer running (`Serial::step_many`, about 1%), and
  `bulk_cycles_available` worked out again at every `sync_now` (1 to 2%).
- **Build settings are not the gap.** A build with fat LTO and one codegen unit ran 1.9% and 2.2%
  more instructions on Geometrix DMG and Tobu Tobu Girl DMG. Wall times on the shared machine were
  too noisy to show a difference either way.

So Gambatte runs fewer instructions in three places. It does far less work per drawn pixel, it does
almost nothing between the moments when something visible happens, and its CPU path per memory
access is a few instructions long.

## 1. Summary: what accounts for the gap, ranked by estimated impact

| Rank | Technique | Where Nubo pays for its absence | Estimated gain on the benchmark games |
|---|---|---|---|
| 1 | **Split the PPU into a timing model and a fast line/tile renderer.** Draw whole tiles, and whole lines where possible, straight to final colors. Do no per-dot fetcher or FIFO simulation while nothing can observe it. | 108 to 144 M instructions per second in the PPU | PPU about 4 to 6 times cheaper: **total instructions −40% to −60%**, about 1.6 to 2.3 times the speed on its own |
| 2 | **Time-derived PPU status and predicted interrupt times** ("next event time" per source). LY, STAT, the LYC flag and VRAM/OAM accessibility are computed from the time without running the renderer. The next STAT, LYC, VBlank and HBlank-DMA moments are worked out in closed form, so the CPU runs until the earliest one. | 3 to 7 PPU catch-ups per line on games that poll or use STAT interrupts. Each one fragments the fast paths and stops the bulk M-cycles | 5 to 15% on top of #1, more on raster-effect games |
| 3 | **A lean interpreter loop**: one deadline compare per instruction instead of per-access bookkeeping. A page table for plain memory. Rare conditions moved out of the hot loop into the deadline. CPU registers kept in locals. | About 40 instructions per plain read, and 55 to 85 M instructions per second on busy games | CPU and bus about 2 times cheaper: **total −10% to −25%** |
| 4 | **Per-line object lists cached across frames**: rebuilt only when OAM (or the object size) changes, with exact handling of an OAM change in the middle of a scan. | Per-line OAM scan, about 3 to 5% | 3 to 5% |
| 5 | **Event-driven sound units and output steps**: each channel unit holds its next event time, a square wave makes events only when its level changes, the DIV-APU edge cuts a batch only when a unit acts on it, and a disabled noise generator advances in closed form. | DIV-APU edge catch-ups and per-batch mixing | 2 to 5% |
| 6 | **Lazy OAM DMA and other "time-stamped" peripherals** (OAM DMA progress, serial shift register, open-bus decay) | Per-M-cycle stepping during OAM DMA, serial stepping in bulk stretches | 1 to 3% |
| 7 | **Draw into the output buffer, or swap buffers, instead of copying a frame** | memcpy of the frame each frame | 2 to 4% |

Together these should bring Nubo to roughly Gambatte's instruction count, 60 to 100 M per emulated
second, without giving up any of Nubo's accuracy. Ranks 1 and 2 belong together: the timing model
from #1 is what makes #2 possible.

---

## 2. The techniques

### 2.1 Split the PPU into a timing model and a fast renderer (rank 1)

**The concept.** A cycle-accurate PPU has two jobs. It must say *when* things happen: mode
changes, interrupt sources, VRAM/OAM blocking, the end of mode 3, HBlank DMA. It must also produce
*pixels*. A pixel FIFO simulated dot by dot does both at once, so every pixel pays for the timing
machinery. Gambatte separates the two:

- **Time is arithmetic.** The line counter is a pair: the current line and the absolute time at
  which the next line starts. The position within a line, the frame cycle and "the next time line
  cycle X comes" are subtraction and comparison, not counting. HBlank, VBlank and mode 2 cost
  nothing per dot. Nothing runs during them at all, except at the few moments where something
  happens.
- **The renderer is a resumable state machine with a time budget.** It is advanced only when
  something needs it to be current ("bring the renderer up to time T"): before any register write
  that changes how pixels are made, at the line's events, and at the end of the frame. Each state is
  a continuation: "where in the line am I, and what comes next". The budget may go negative,
  meaning the renderer is behind and owes time.
- **A whole-tile fast path.** When the renderer is at a tile boundary and the next span holds no
  object and no window start, it draws whole tiles in a tight loop. For each tile it reads the tile
  index, then the two bitplane bytes (and the CGB attribute byte), and turns the two bytes into
  eight 2-bit color indices with a lookup table. The table maps a byte to a word with its bits
  spread out, one bit per 2-bit slot; a second variant has the bit order reversed, for horizontal
  flip. Combining the spread low byte with the spread high byte shifted by one gives all eight
  indices at once. It then writes eight final colors straight into the frame buffer through a
  palette that is already converted to output colors. That palette is updated when the game writes
  a palette register, after the renderer has caught up. There is no FIFO and no per-dot fetcher
  step.
- **Objects are merged per tile, not per pixel.** The objects on the line come from a cached
  per-line list sorted by X (see 2.4). When the renderer reaches an object, it reads the object's
  row once and turns it into the same packed form, with flip applied by choosing the table variant.
  It then merges the row into the eight pixels of the current tile with a priority routine, one
  routine for DMG rules and one for CGB rules. Object pixels that fall into the next tile are
  carried over by shifting the packed word.
- **A slow per-pixel path for the rest**: partial tiles at the start of the line (fine scroll), the
  window start, and resuming at an unaligned position after a mid-line register write.
- **Mode 3 length is predicted, not observed.** Every renderer state can answer "how many cycles
  until the drawing position reaches pixel X". The answer is the remaining pixels, plus the fixed
  start-of-line overhead, plus a fine-scroll penalty, plus a window-start penalty if the window
  starts before X, plus an object-fetch penalty for each remaining object before X. The object
  penalty depends on where the object sits relative to the background fetch grid, and on whether an
  earlier object already paid for the same fetch slot. The prediction is cached and invalidated by
  any write that can change the length of mode 3: SCX, WX, the window enable, WY reaching the
  window trigger, object enable, object size, or OAM. The "end of mode 3 on this line" time feeds
  the mode 0 interrupt, the HBlank DMA trigger, STAT mode reads and VRAM/OAM access checks (2.2).

**How it stays exact (in Gambatte's own model).** The renderer is always brought up to the exact
time of a write before the write lands, so a mid-line change takes effect at the right pixel. The
prediction is recomputed after every change that can move the end of mode 3, and it stays valid in
between because nothing else can change those inputs.

**Where Nubo differs and what it costs.** Nubo's PPU is a port of SameBoy's per-dot pixel FIFO
(`gpu/pipeline.rs`) with two shortcuts. Steady stretches run in bulk (`gpu/steady.rs`), and plain
tiles run whole (`draw_background_tile`). But even the plain-tile path still:

- runs all eight fetcher steps per tile through the general step functions (`take_tile_steps`
  calling `find_tile_number`, `read_tile_number`, `read_tile_data_*`, `fetcher_push`), with their
  glitch checks (`tile_sel_glitch`, `stop_blocks`, `last_tileset`) and FIFO struct updates;
- takes each pixel's color index apart from the two FIFO bytes with shifts and masks (78 M
  instructions in 10 s of Tobu Tobu Girl for that one line of code);
- checks `can_draw_plain_tile` or `can_draw_plain_pixel` for every tile or pixel.

Any object on a tile drops that tile to the full per-dot `tick_drawing`. Every non-plain dot goes
through `tick_dot`, which counts down ten countdowns and updates the LYC flag and STAT line on
every dot. OAM scan runs every line (see 2.4). The finished frame is then copied (see 2.7).

**How it could fit Nubo's lazy-components / bit-exact rule.** The rule is "bit for bit what
running every component one M-cycle at a time gives", and Nubo's per-dot pipeline *defines* that
result. The SameBoy-port pipeline therefore stays as the reference and as the fallback. The fast
renderer is an accelerator that must reproduce it exactly, the way `plain_pixels.rs` does today:

1. **A line renderer for undisturbed lines.** When the PPU catches up to a point past the end of
   the current line's mode 3, and the pipeline is at a known clean state (the start of mode 3, or a
   tile boundary with the fetcher at its first step and nothing glitchy pending), draw the rest of
   the line in one pass. The pass takes the background and window in whole tiles, the objects from
   the line's object list into a line buffer, and composites per pixel. No CPU write can come in
   between, because every write to PPU state catches the PPU up first (`reaches_ppu`,
   `catch_up_ppu`), and that already guarantees the line's inputs are final for the stretch being
   drawn. The pass must leave the pipeline exactly as the per-dot run would at the end of mode 3:
   the window line counter, `position_in_line`, `lcd_x`, the object state, the fetcher state, and
   any flag that carries into HBlank or the next line. Debug builds can run both and compare, as
   Nubo already does for `take_tile_steps`.
2. **A mode 3 length model derived from Nubo's own pipeline**, not from Gambatte. It gives the dot
   at which mode 3 ends (and at which the mode 0 interrupt countdown starts) for the line's inputs:
   SCX fine scroll, whether and where the window starts, and the X positions and OAM order of the
   line's objects. Where the model isn't sure (an object at X below 8, a window together with
   objects at the same tile, CGB-specific quirks, a line right after the LCD is switched on), it
   returns "unknown" and the reference pipeline runs instead. `gpu/tests/random_scenes.rs` is the
   right place to prove the model: random OAM, SCX, WX, WY, LCDC, compared dot for dot. Debug
   builds can assert, when the reference pipeline reaches the end of mode 3, that the model gave
   the same dot.
3. **A cheaper plain-tile path in the meantime.** Even before the line renderer, the tile loop
   could keep the tile map row, the tile data base and the palette mapping in locals across tiles.
   It could decode a tile row with a lookup table (byte to spread bits, built at start-up by a
   loop), and write the fetcher's end state once per run of tiles instead of stepping it eight
   times per tile. This keeps the existing structure and tests.
4. **Objects without dropping to per-dot.** The pipeline's object fetch timing (the penalty each
   object adds) is what makes objects hard. With the model from step 2 giving the timing, the
   pixels can be produced by a classic object line buffer: per line, up to ten objects in drawing
   priority order (DMG: by X, then OAM order; CGB: OAM order), each writing color index, palette
   and priority bits into a 160-entry buffer only where no higher-priority object already wrote,
   then composited with the background color index under the BG-priority rules.

**Estimated gain.** Drawing is 70 to 80% of Nubo's PPU cost. A tight tile loop costs roughly 30 to
50 instructions per tile, against about 300 now, and the per-dot overhead around mode 3 mostly
disappears for undisturbed lines. That makes the PPU 4 to 6 times cheaper, 20 to 35 M instructions
per emulated second instead of 108 to 144: **−40% to −60% of total instructions**, about 1.6 to 2.3
times the speed on its own. Busier screens with more objects gain more, because today any object
on a tile drops that tile to per-dot.

**Risks.**

- *Accuracy*: the fast path must match the reference in every case it claims. The Mealybug,
  dmg-acid2, cgb-acid2/hell and Mooneye PPU tests, the random-scene tests, and `cargo benchmark
  --compare` against an `eager` build are the safety net. Keep the fallback generous at first and
  narrow it later.
- *Save states*: none, as long as the fast path leaves exactly the fields the per-dot path would.
  Predicted times and lookup tables are caches (`#[serializable(skip)]`, rebuilt on load).
- *Complexity*: high. This is a second renderer next to the reference, and every PPU quirk Nubo
  adds later must either be handled in both or force the fallback.

### 2.2 Time-derived PPU status and predicted interrupt times (rank 2)

**The concept.** This is a classic event-driven ("next event time") emulator design. Each source
that can do something visible keeps the absolute time of its next occurrence: the timer overflow,
the serial transfer end, OAM DMA progress, HBlank DMA, the end of the requested run, the frame end,
the waking of a halted CPU, and the interrupt check itself. The CPU runs instructions while its
cycle counter is below the minimum of these times and handles an event only when it gets there.
For the PPU, Gambatte schedules only the moments the CPU could observe through an interrupt:

- the mode 0 STAT source, and only while it is enabled in STAT. Its time is the predicted end of
  mode 3 (from 2.1), recomputed after any change that moves it;
- the mode 2 source, only while enabled: a fixed point per line, or per frame when another enabled
  source makes the per-line one redundant;
- the mode 1 (VBlank) source and the VBlank interrupt: once per frame;
- the LY=LYC source: the time of the next line equal to LYC, so once per frame, recomputed on LYC,
  STAT or LCDC writes;
- one-shot STAT requests caused by register writes (the STAT-write and LYC-write quirks);
- HBlank DMA: the predicted mode 0 time of each line, only while a transfer is active;
- a rebuild of the per-line object lists, only after OAM changed (2.4).

A small "STAT line" model decides, at each source's event, whether that source's rising edge
actually raises the shared STAT interrupt line, given which other sources are high at that moment.
That is the well-known STAT blocking behavior. Register reads are answered from the time: LY from
the line counter (with its known early-increment quirks), the STAT mode from the line position and
the predicted mode 3 end, and the LYC flag from the comparison at that time. The renderer runs on a
read only if an event is due before the read's time. VRAM and OAM accessibility for CPU accesses is
answered the same way.

Two more "everything is an event" choices keep the CPU loop tight:

- **Interrupt dispatch is an event.** The "check for an interrupt" event is scheduled only when IME
  is on or the CPU is halted, and IF & IE is non-zero. Its time is the earliest moment the CPU may
  take it, which accounts for the one-instruction EI delay. Writes to IF and IE, EI, DI, HALT and
  every interrupt request update it. The instruction loop never looks at IF or IE.
- **The minimum of the event times is kept up to date as they change**, so reading it costs one
  load. Gambatte uses a tournament tree (a binary min-tree) over a fixed set of event slots. With
  about ten slots, a plain array plus a cached minimum that is recomputed only when a slot changes
  works just as well. A disabled event holds the largest time.

**Where Nubo differs and what it costs.** Nubo already has the "how long can the PPU be left
behind" idea (`quiet_dots`) and bulk M-cycles. The difference is in how far ahead it can see and
what forces a catch-up:

- Any CPU read of LY, STAT, IF, IE, VRAM or OAM catches the PPU up (`reaches_ppu` then
  `catch_up_ppu`) and runs the deferred M-cycles. Games that poll LY or STAT in a wait loop catch
  the PPU up hundreds of times per frame (about 960 per frame in Geometrix CGB). Each catch-up ends
  a bulk stretch and recomputes `bulk_cycles_available`. Inside mode 3 it can also leave the
  pipeline off a tile boundary, which drops the next dots out of the plain-tile path.
- When IE enables STAT and any STAT source is enabled, `quiet_dots` only looks to the end of the
  current line, or to the end of mode 3 (`has_steady_shape` or `dots_before_last_pixels`). Then the
  PPU must be run at least two or three times per line, even if the only enabled source is LY=LYC
  for a line 100 lines away. Gambatte would schedule one event for that line.
- `bulk_cycles_available` works out a fresh minimum of eight limits whenever the budget runs out
  (`sync_now`). An event table would keep that minimum and change it only when one limit changes.

**How it could fit Nubo.** The timing model from 2.1 is the precondition. With it, the PPU can
answer "what would STAT, LY, the LYC flag, VRAM and OAM accessibility be at dot D" without drawing,
and "at which dot does the STAT line next rise", exactly as its per-dot run would. Then:

- `reaches_ppu` reads of LY, STAT, VRAM and OAM become *queries* answered from the model, plus a
  catch-up only when the model says an event (a dot-timed change of state) falls before the read.
  Writes still catch up exactly as now, since `write_timing.rs` stays the authority.
- `quiet_dots` becomes "dots until the next STAT line rising edge, the VBlank events or the HBlank
  DMA trigger", computed from the enabled sources and LYC. That can be a whole frame for a game
  that uses only an LYC interrupt.
- The address bus keeps a small table of deadlines (PPU event, TIMA overflow, serial end, the
  DIV-APU edge the APU needs, APU catch-up, audio buffer full, dot limit). `bulk_cycles_available`
  becomes a read of the cached minimum when nothing is running that needs every M-cycle.

*Exactness:* every answer from the model must equal what the per-dot pipeline would show. The
existing debug checks pattern fits: in debug builds, when the PPU later does catch up, assert that
the state it reaches matches what the model answered for that dot. `random_scenes.rs` can compare
model queries against per-dot runs at random dots. The test ROMs that read STAT and LY at every dot
offset (Mooneye's PPU tests, Gambatte's own STAT and LY tests, GBMicrotest) cover the edges.

**Estimated gain.** 5 to 15% on top of 2.1 for the benchmark games, through fewer catch-ups, longer
bulk stretches and fewer fast-path breaks. More on games with raster effects or LY polling, less on
games that halt in VBlank.

**Risks.** *Accuracy*: the STAT line's edge cases (blocking, the DMG STAT-write quirk, LYC writes
near line changes, line 153's early LY=0) are subtle. Each query must reproduce Nubo's calibrated
dot positions, not Gambatte's. *Save states*: event times are derived, so recompute them on load
and don't store them. *Complexity*: medium to high, and it depends on 2.1.

### 2.3 A lean interpreter loop with a single deadline and a page table (rank 3)

**The concept.** These are classic interpreter techniques:

- **One deadline, kept in a register.** The cycle counter is a local variable for the whole run
  loop and is written back only when leaving it. Each memory access adds one M-cycle to it and does
  nothing else. The loop compares the counter with the next event time once per instruction. Every
  rare condition is itself an event that pulls the deadline in: end of the requested run, frame
  end, HDMA due, a halted CPU waking, an interrupt to dispatch, a debugger hook. The fast loop never
  checks those conditions.
- **A page table for memory.** There is one pointer (or offset) per 4 KiB page for reads and one for
  writes, covering ROM bank 0, the switchable ROM bank, cartridge RAM when it is plain RAM, and the
  WRAM banks and echo. Bank switches update the table. A null entry means "take the slow handler":
  VRAM (whose access depends on the PPU mode), OAM and I/O, cartridge RAM with an RTC or other
  hardware behind it, and pages a running OAM DMA conflicts with (they are switched to the slow
  path when DMA starts and back when it ends). The entry is biased so that the full address indexes
  it directly. A plain read is then a table load, a null check and a byte load. HRAM and the
  "plain" I/O range have their own short path.
- **Debug hooks compiled out.** The interpreter is instantiated twice, with and without
  memory/trace callbacks, and the run picks one at entry, so the normal loop has no hook checks.
- **Lazy flags.** Instead of building F after every ALU operation, the loop keeps the raw results
  (the 9-bit sum for carry, the operands for half carry, the result for zero) and builds F only
  when an instruction or a save state reads it. This is a classic interpreter trick; it is worth
  measuring before adopting, because modern CPUs handle flag building cheaply.
- **Halt jumps to the deadline.** A halted CPU moves its counter straight to the next event time,
  rounded to the M-cycle grid.

**Where Nubo differs and what it costs.**

- Every access goes through `sync()`, which updates `deferral_budget` and `deferred_cycles`. That
  is cheap on its own, but `sync_now` runs `deferrable_cycles` and `bulk_cycles_available` (eight
  `min`s and a dozen state checks) every time the budget runs out: 550,000 times in 10 s of
  Geometrix CGB.
- A plain read costs about 40 instructions. That is `read_byte_in_synced_cycle` (bus address
  latch, processor-test-mode check), `cpu_read_byte` (`reads_plain_memory` match, DMA check,
  test-mode check), `read_plain_memory` (boot ROM check, address match, `working_ram_index` with the
  SVBK bank, `banked_read`) and `apply_cheats`.
- `running_step` checks `hdma_in_progress`, `take_boot_rom_exit`, `held_in_reset`, `is_locked_up`,
  `clear_recorded_bus_activity` (test mode), the HALT opcode and the interrupt dispatch on every
  instruction.
- CPU registers live in `self.registers`, behind `&mut self`, across non-inlined calls
  (`cpu_read_byte_elsewhere`, `sync_now`), so the compiler must reload them after such calls.

**How it could fit Nubo.** All of these keep the per-M-cycle semantics unchanged, so they fit the
bit-exact rule by construction:

- Keep the deferred-cycle design, but give the bus a cached deadline (2.2) and let the CPU's
  instruction loop compare the M-cycles it has run against it once per instruction. The deferred
  M-cycles are still run before any non-plain access, as now.
- Add a 16-entry page table on `AddressBus`. Each entry is either "plain, at offset X in a backing
  store" or "slow", for reads and for writes. Make it safe Rust: offsets into one `Vec<u8>` or a
  small enum of backing arrays, no raw pointers, since `unsafe` is forbidden. Update it in
  `update_rom_view`, on SVBK and cartridge RAM enable writes, when the boot ROM is unmapped, while
  cheats patch a page (only those pages go slow), and while OAM DMA runs (all conflicting pages go
  slow, as `cpu_read_byte` already does for any DMA). The plain path becomes: the entry is plain,
  so read the byte and call `sync()`.
- Move processor test mode out of the hot path with a const generic on the step function, or a
  separate `step` for test mode, the same way Gambatte keeps its hooks out.
- Move rare per-instruction checks (boot ROM exit, HDMA in progress, held in reset, locked up) into
  states that make the deadline zero, so the fast loop doesn't see them.
- Consider running a batch of instructions in one call (`Cpu::run_until(deadline)`) with
  registers copied into locals at entry and written back at exit and before any slow access.

**Estimated gain.** The CPU and bus cost is 20 to 85 M instructions per second, depending on how
much the game halts. Halving it is realistic: **−10% to −25% of total instructions** on Geometrix
and µCity, little on Tobu Tobu Girl, which mostly halts.

**Risks.** *Accuracy*: low, as long as every non-plain access still runs the deferred cycles first.
The JSON CPU tests (`cargo cpu-tests`) and the eager comparison catch mistakes. *Save states*: none
(the page table is a cache; rebuild it on load). *Complexity*: medium. Cheats and DMA conflicts
need care so that a page is never plain while it must not be.

### 2.4 Per-line object lists cached across frames (rank 4)

**The concept.** The OAM scan finds, for each visible line, the first ten objects whose Y range
covers it. Gambatte doesn't simulate the scan each line. It keeps a snapshot of each OAM entry's Y
(and X) and builds, for all 144 lines at once, the list of objects on each line. It rebuilds the
lists only after OAM or the object size changed. The rebuild is an event at the end of the next
line's scan, and no event is scheduled while OAM is unchanged. A line's list is sorted by X only
when that line is drawn, with an insertion sort, which is cheap for ten entries. To stay exact when
OAM changes in the middle of a line's scan, the snapshot is updated progressively: a progress
marker remembers how far the current line's scan had got at the time of the change (from the time within
the line), and copies the changed entries into the snapshot only once the simulated scan position
has passed them. The current line therefore sees old values for entries the scan had already read
and new values for the rest.

**Where Nubo differs.** `tick_oam_scan` and `scan_oam_entries` read OAM entries for every line,
two dots per entry, inside the steady stretches or dot by dot. That is about 3 to 5% of the total
in the profiles (the `visible_object_count` and `oam_y` lines and `tick_oam_scan`).

**How it could fit Nubo.** Cache the list for each line (and the per-line object data the pipeline
reads at mode 3 start), and mark it stale when OAM is written: by the CPU, by OAM DMA (once per
transfer), by the OAM corruption bug, or by an LCDC object-size change. A scan that runs while the
cache is valid takes the cached result for its line. A scan that overlaps a change, or anything
unusual (`tall_objects_glitch`, OAM DMA holding OAM, the first line after the LCD is switched on,
STOP blocking OAM), runs the existing per-entry scan. This keeps the exact per-entry behavior
where it matters and skips it where nothing changed. Debug builds can compare the cached list
against a fresh scan.

**Estimated gain.** 3 to 5%. It is also a building block for 2.1's line renderer, which needs the
line's object list up front.

**Risks.** Low to medium. The OAM corruption bug and DMA interactions must invalidate the cache.
Save states: a cache, so skip it and rebuild it on load.

### 2.5 Event-driven sound units and cheap output steps (rank 5)

**The concept.**

- Each channel is advanced lazily, on a register access to it or at the end of a run, from its last
  update time to "now".
- Within a channel, each unit (waveform position, envelope, length counter, sweep, noise LFSR) holds
  the absolute time of its next event, and the channel's loop jumps from the nearest event to the
  next one. Positions in between are computed on demand: for example, the duty position at time T
  is a division of the elapsed time by the period.
- The square wave's waveform unit produces an event only when the output *level* changes, skipping
  duty steps that keep the same level. How far ahead the next level change is depends on the duty
  pattern and the current position, which is a small lookup by (duty, position).
- A disabled noise channel doesn't step its LFSR every period. It records when the LFSR was last
  brought up to date and catches up in closed form when observed, applying several shifts at a time
  with XOR identities for the 15-bit and 7-bit modes.
- The output goes into a **delta buffer** at a fixed internal rate. A channel adds only the
  difference between its new and old level, at the sample where the level changes, and a single
  running prefix sum turns the deltas into levels when the buffer is handed out. Both stereo sides
  are packed into one 32-bit word (SIMD within a register), with a bias so that the low half never
  borrows from the high half, so one add serves both sides.
- Frame sequencer clocks (length, envelope, sweep) are unit events that exist only while the unit
  is active. A channel with its envelope period at 0 and length disabled gets no sequencer events.

**Where Nubo differs.** Nubo's APU is already lazy and cuts batches only where a channel's output
can change (`apu/catch_up.rs`), which is the same idea. Two costs remain:

- Every DIV-APU edge (twice per frame sequencer step, about 1,024 times per second) forces
  `div_apu_bit_changed` to catch the APU up, and it limits every bulk stretch
  (`cycles_until_div_apu_edge`). In Tobu Tobu Girl that costs 44 M instructions in 10 s, about 4,200
  instructions per edge.
- Each output change is mixed with a 64-tap band-limited step (`apu/blep.rs`), per side.

**How it could fit Nubo.** Make the DIV-APU edge an event only when some unit acts on it: an active
length counter, an envelope with a non-zero period, an active sweep. Otherwise let the frame
sequencer's step count be derived from the system counter when next needed, so the edge neither
cuts bulk stretches nor forces a catch-up. That keeps exactness because a step that changes nothing
the channels read is already judged quiet in debug builds (`catch_up.rs` says so). The noise LFSR
jump-ahead also fits if Nubo ever steps a silent LFSR. Keep the band-limited output: it is a
quality choice (see section 3). The `LazyLock` dereference that shows up per sample in
`enqueue_audio_samples` (about 28 M instructions in 10 s, under `once.rs`) can be hoisted out of
the per-sample loop.

**Estimated gain.** 2 to 5%.

**Risks.** Low. The APU's lazy, bulk and one-M-cycle tests in `apu/tests.rs` cover it. Save
states: unchanged if the sequencer step is still stored as it is now.

### 2.6 Lazy OAM DMA and time-stamped peripherals (rank 6)

**The concept.** A peripheral that progresses at a fixed rate stores *when* it started (or was last
brought up to date), and its visible state is computed when observed:

- OAM DMA copies the bytes due since its last update when anything could see OAM, or the source or
  bus being conflicted, and then at its end event. While it runs, the memory pages it conflicts with
  use the slow path, so ordinary RAM accesses stay fast.
- The serial port computes how many bits have shifted from the elapsed time when SB or SC is read,
  and schedules only its completion.
- TIMA is computed from the elapsed time and the TAC rate on read. Its overflow time is scheduled.
  DIV comes from a time stamp.
- The cartridge open-bus value remembers the last driven byte and when it was driven. A read later
  than the decay time sees the pulled-up value. No per-cycle decay runs.

**Where Nubo differs.** While OAM DMA runs, `bulk_cycles_available` returns 0 and every M-cycle
goes through `sync_all_but_timers`'s DMA branch, which catches the PPU up and steps it dot by dot
(`step_dots`). That happens for 160 M-cycles per transfer, usually once per frame. `run_bulk_cycles`
calls `Serial::step_many` even with no transfer clocked (about 1% in Geometrix CGB and µCity CGB).
Timers are already in closed form in bulk (`timers.run_cycles`), but they are stepped every M-cycle
outside bulk.

**How it could fit Nubo.** Nubo's DMA is detailed: SameBoy-style bus conflicts, plus Gambatte's
tests for the source cases. Keep the per-byte semantics, but let the DMA run in bulk while the CPU
only touches HRAM (the usual DMA wait loop), copying the bytes due in one go. Any non-HRAM access
first brings the DMA up to date, as `run_deferred_cycles` does for other components. Skip
`Serial::step_many` when no transfer is clocked by either side.

**Estimated gain.** 1 to 3%.

**Risks.** Medium for DMA: the PPU's view of OAM during DMA (`set_oam_dma_access`, the
half-M-cycle delay) must stay exact, so the PPU still has to see each DMA byte at its own dot. The
bulk DMA path should hand the PPU the same sequence. Low for serial.

### 2.7 Draw into the output buffer, or swap buffers, instead of copying (rank 7)

**The concept.** Gambatte's renderer writes final colors straight into a buffer the frontend
provides. There is no per-frame copy.

**Where Nubo differs.** `publish_frame` copies `frame_buffer` into `completed_frame` each frame. The
memcpy shows up at 2 to 4% of all instructions.

**How it could fit Nubo.** Swap the two buffers (`std::mem::swap` of the `Vec`s) at the frame end
instead of copying. The SGB and blank-frame cases keep their own handling. No effect on results.

**Estimated gain.** 2 to 4%. **Risks.** Very low. Save states: the frame buffers are already
handled; check that the swap doesn't change which buffer a save state stores.

---

## 3. Things that look like speed but aren't (don't chase these)

- **Gambatte's PPU accuracy model.** Gambatte is not a pixel FIFO. It approximates mid-scanline
  effects with its own timing rules, and tests that depend on FIFO details (such as Mealybug
  Tearoom's mid-scanline register tests) are where FIFO emulators tend to be ahead. Check the GB
  Emulator Shootout's table before relying on any one test. No OAM corruption bug emulation turned
  up in the files searched. Its speed comes partly from not modeling those details. Nubo must keep its SameBoy-port pipeline as the reference and
  take only the *structure* (timing model plus fast renderer plus fallback), with every timing
  derived from Nubo's own pipeline and calibration. Never copy its penalty rules or offsets; Nubo's
  are calibrated against Mooneye and SameBoy and may differ.
- **Audio quality.** In the comparison, Gambatte's output is a raw high-rate delta signal, turned
  into 44,100 Hz by its cheapest resampler. Nubo's 64-tap band-limited steps are a quality feature.
  The APU is only 3 to 8% of Nubo's instructions anyway. A cheaper output stage would change Nubo's
  sound, which is a product decision, not a speed fix.
- **Build flags.** Fat LTO with one codegen unit didn't lower Nubo's instruction count (it rose by
  about 2%). Gambatte's `-O2 -fomit-frame-pointer` is unremarkable. The gap is algorithmic.
- **Simplified DMA conflicts and STOP handling.** Gambatte's handling of OAM DMA bus conflicts and
  of STOP is simpler than Nubo's SameBoy-derived model (STOP is marked unfinished in its own
  comments). Nothing there to adopt for speed. Nubo's detail costs little, because it only runs
  while DMA or STOP is active.
- **A 32-bit cycle counter that is periodically rebased.** Gambatte keeps its time stamps small and
  subtracts a constant from all of them now and then. Nubo's `u64` dot counter doesn't need this,
  and adding it would only add risk.
- **Quiet benchmark screens.** The three benchmark games spend much time on title screens. Both
  emulators skip ahead there, so the measured gap is mostly the per-frame drawing and CPU costs
  above, not halting. Run the comparison on busier games before and after each step, so the gains
  aren't judged on idle screens alone.

## 4. Suggested order of work

1. **Measure per component.** Add a small script around callgrind (or `cargo xtask compare-emulators
   --instructions`) that reports Nubo's PPU, CPU+bus and APU instructions per emulated second, as
   in the first table. Run it, plus `cargo benchmark --compare` against an `eager` build, after
   every step.
2. **Cheap, low-risk wins** (a few % each, no change to results): swap frame buffers instead of
   copying (2.7); skip `Serial::step_many` without a clocked transfer (2.6); hoist processor-test-
   mode checks out of the hot path with a const generic (2.3); hoist the BLEP table `LazyLock`
   out of the per-sample loop (2.5).
3. **Cheaper plain tiles** (2.1, step 3): table-driven tile row decoding, the fetcher's end state
   written once per run of tiles, and tile map and data addressing kept in locals. This is
   self-contained in `plain_pixels.rs` and checked by `random_scenes.rs`.
4. **Cached per-line object lists** (2.4). This is needed by step 5.
5. **The line renderer with an object line buffer and the mode 3 length model** (2.1, steps 1, 2
   and 4), with a generous fallback to the reference pipeline. This is the big one: expect most of
   the gain here.
6. **Time-derived STAT/LY/LYC and VRAM/OAM access, and predicted STAT line edges** (2.2), built on
   the model from step 5. `quiet_dots` then sees up to the next real event.
7. **A deadline table on the bus and a page table for plain memory** (2.2 and 2.3), then the
   instruction-batch loop with registers in locals.
8. **DIV-APU edges only when a sound unit needs them; bulk OAM DMA** (2.5 and 2.6).

Run the full checklist from `AGENTS.md` after each step: test ROMs, CPU tests, save states, and the
benchmark fingerprints against the previous build and against an `eager` build.

## 5. Provenance

Gambatte-core at commit `5a41a68c25402421fb1983ddadc9faf2418ddb0f`, cloned into an isolated
scratch directory. Nothing from it was copied into the Nubo repository, built or run. Files read
(whole or in part):

- `libgambatte/src/cpu.h`, `libgambatte/src/cpu.cpp` (start, main loop, HALT, STOP, the end)
- `libgambatte/src/memory.h`, `libgambatte/src/memory.cpp` (events, DMA, halt, stop, counter
  reset, input, OAM DMA update, start of I/O reads)
- `libgambatte/src/minkeeper.h`
- `libgambatte/src/interruptrequester.h`, `libgambatte/src/interruptrequester.cpp`
- `libgambatte/src/interrupter.cpp` (interrupt dispatch)
- `libgambatte/src/tima.h`, `libgambatte/src/tima.cpp` (first half)
- `libgambatte/src/video.h`, `libgambatte/src/video.cpp`
- `libgambatte/src/video/ppu.h`, `libgambatte/src/video/ppu.cpp` (the state machine outline, the
  whole-tile path, the mode 3 prediction, update and LCDC handling)
- `libgambatte/src/video/sprite_mapper.h`, `libgambatte/src/video/sprite_mapper.cpp`
- `libgambatte/src/video/ly_counter.h`, `libgambatte/src/video/ly_counter.cpp`
- `libgambatte/src/video/next_m0_time.h`, `libgambatte/src/video/next_m0_time.cpp`
- `libgambatte/src/sound.h`, `libgambatte/src/sound.cpp`
- `libgambatte/src/sound/channel2.cpp` (update loop), `libgambatte/src/sound/channel4.cpp` (LFSR
  and update loop)
- `libgambatte/src/sound/duty_unit.h`, `libgambatte/src/sound/duty_unit.cpp` (first half)
- `libgambatte/src/sound/sound_unit.h`
- `libgambatte/src/mem/memptrs.h`, `libgambatte/src/mem/memptrs.cpp` (searched for the page table
  updates only)
- `libgambatte/src/gambatte.cpp` (the run entry point)
- `libgambatte/include/gambatte.h` (searched for the speed-up flags only, to confirm the comparison
  harness doesn't use them)

Nubo files used for the comparison: `crates/nubo-core/AGENTS.md`, `docs/EMULATOR_COMPARISON.md`,
`docs/ACCURACY.md`, `crates/nubo-core/src/address_bus.rs`, `address_bus/bulk_cycles.rs`,
`address_bus/dma.rs`, `address_bus/write_timing.rs`, `cpu.rs`, `cpu/microops.rs`,
`cpu/opcodes.rs`, `cpu/interrupts.rs`, `gpu.rs`, `gpu/steady.rs`, `gpu/pipeline/plain_pixels.rs`,
`apu/catch_up.rs`, `apu/blep.rs`, `xtask/emulator_harnesses/gambatte.cpp` (Nubo's own harness), and
callgrind profiles of `nubo-benchmark` on the three benchmark games.
