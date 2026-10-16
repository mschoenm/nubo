# nubo-core

The emulator itself: a plain Rust library with no platform dependencies and no C dependencies, so it also builds for wasm32. Frontends use only what `lib.rs` re-exports (`nubo_core::Emulator`, `nubo_core::Model`, ...). Each file's module docs explain that file; this guide explains how the files work together.

## Where Things Are

| Part | Files | Notes |
|---|---|---|
| Public API | `emulator.rs`, `error.rs` | `Emulator` owns the CPU, which owns the address bus and through it everything else. It is `Send` (`CartridgeEffects: Send`), so an app may move it to another thread |
| Save states | `emulator/save_state.rs`, `serializable.rs`, `crates/nubo-serializable-derive/` | See Save States below |
| Hardware models | `model.rs`, `cpu/post_boot.rs`, `address_bus/boot_rom.rs` | DMG-0 to AGB, ordered like SameBoy's model numbers |
| Time | `clock.rs` | How M-cycles, PPU dots and the system counter behind DIV relate, at both speeds |
| CPU | `cpu.rs`, `cpu/` | Instructions, interrupts, HALT, STOP, timers |
| Address bus | `address_bus.rs`, `address_bus/` | Memory map, one M-cycle's order, cartridges and their controllers (MBCs), DMA, write timing, speed switch, STOP mode, cheats |
| PPU | `gpu.rs`, `gpu/` | Line timeline, pixel pipeline (SameBoy port), bulk stretches (`steady.rs`), palettes |
| APU | `apu.rs`, `apu/` | Four channels (SameBoy port), lazy catch-up (`catch_up.rs`), band-limited output |
| Input, serial | `joypad.rs`, `controller.rs`, `serial.rs` | |
| Super Game Boy | `sgb.rs`, `sgb/` | The Super Nintendo's side: commands sent through JOYP, colors, transfers, borders (SameBoy port), Nubo's own border with its night and day looks (`nubo_border.rs`), Nubo's intro, the menu's settings and the attraction mode (`attraction.rs`). The PPU owns it and hands it each finished frame |
| Boot ROMs | `bios.rs`, and their sources in `crates/nubo-core/boot_roms/` | Nubo's own, timed to the cycle like the ones they replaced |

## How Time Moves

The CPU drives everything: each M-cycle (machine cycle, one CPU step) it asks the address bus to move the hardware on. The address bus's module docs list the order of one M-cycle, its variants, and what a new per-cycle component has to join (including `bulk_cycles_available`). `Emulator::step_until_next_audio_buffer` runs until the APU has filled an audio buffer, which is what paces the Nubo app; `Emulator::run_frame` runs to the next frame boundary (a multiple of 70,224 dots since power-on) instead, for a host that calls it once per frame, and caps the stretches run in one go there (`dot_limit`). At vblank the PPU hands over the finished frame, which frontends pull with `Emulator::frame_buffer` or `Emulator::take_frame`.

## Lazy Components, Exact Results

Running every component every M-cycle is slow, so the core takes shortcuts. They must never change the result: everything must come out bit for bit as if every component ran one M-cycle at a time.

- **The PPU runs behind.** The address bus owes it dots (`ppu_owed_dots`) for as long as nothing it does can be seen: no frame, HBlank DMA signal or interrupt the CPU could act on (`Gpu::run_dots` reports how long that is). It catches up before any CPU access to PPU state, IF or IE (`reaches_ppu`), except a read of IF while its quiet dots hold no interrupt request at all (`AddressBus::ppu_read_can_wait`), and around OAM DMA, speed switches, STOP and the boot ROM exit. CPU writes to PPU state run their M-cycle dot by dot, so `write_timing.rs` stays exact. While catching up, the PPU runs steady stretches in bulk, and a line's first dots and HBlank's first dots while the STAT line can't rise (`gpu/steady.rs`), runs of plain tiles in mode 3 a whole tile at a time, with the fetcher's eight steps per tile in one go (`gpu/pipeline/plain_pixels.rs`), and everything else dot by dot.
- **The APU runs behind.** `Apu::step` only counts M-cycles, and the channels catch up in batches cut wherever their output can change (`apu/catch_up.rs`).
- **Plain M-cycles run in bulk.** While the CPU only reads ROM or uses work RAM and HRAM, or is halted with nothing to wake it, `address_bus/bulk_cycles.rs` runs whole stretches at once.
- **The rule:** any new way for the CPU or another component to see or change hardware state must first run the waiting M-cycles and catch the PPU and APU up. The APU's fields are private to `apu/` so this can't be skipped there; debug builds check that `peek` and `poke` only run on a caught-up PPU. `Emulator::memory_mut`, which lets a frontend change memory between frames (RetroArch's memory editor and cheats), is one such way and catches up first (`AddressBus::catch_up_ppu_before_change`).
- **The checks:** the `eager` feature runs every M-cycle on its own with nothing left behind; `cargo benchmark --compare` between an eager and a normal build of the same commit shows whether the shortcuts change anything. `gpu/tests/random_scenes.rs` compares bulk and dot-by-dot PPU runs (18 random scenes in CI, 10,000 with a new seed each night), and `apu/tests.rs` compares lazy, bulk and one-M-cycle-at-a-time APU runs on every model.

## Save States

Players must never lose a save state to an update. The format is in `emulator/save_state.rs`, how values are stored in `serializable.rs`. What every change needs to know:

- Every value is stored with its name, so adding, removing or reordering fields never breaks loading: a value a state lacks starts at its power-on value, and values this version doesn't know are skipped. (The layout check below still asks for a new kept set.)
- Stored names are permanent. A field or variant renamed in the code keeps its stored name; a field whose type or meaning changes (another size, other units, other values) is stored under a new name. How a stored name is kept depends on how the type is stored:
  - Types with `#[derive(Serializable)]`: `#[serializable(name = "stored_name")]` on the field or variant. The derive refuses two stored fields or variants with the same stored name.
  - Hand-written implementations (`Emulator`, `Apu`, `CartridgeMapper`, `Palettes`, the pipeline FIFOs, `BlepBuffer`, `OamDmaAccess`, the address bus's own `bus` fields in `AddressBus::write_save_state_components`, where `working_ram` is stored as `work_ram`, and the save state header in `SaveStateHeader::write`): the stored name is the text the implementation writes (`FieldWriter::field` or `field_with` for a field, `write_variant` for a variant) and matches when reading; keep that text.
  - The PPU (`gpu/save_state.rs`): its macro stores each field under its Rust name, so renaming a stored PPU field changes its stored name. Don't, unless you first teach the macro to keep the old one (for example, let the `saved:` list take `field as "stored_name"` and write that text instead of `stringify!`). `cargo test --workspace` and `cargo save-states` catch the changed layout either way.
- Values are fixed-size and little-endian, never `usize`, and floats are stored as their IEEE 754 bits, so a state loads on every platform.
- Caches, values that are always zero between `Emulator` calls, and what belongs to the player rather than the console (the buttons held, the Super Game Boy menu's choices) are left out with `#[serializable(skip)]` (or with a comment in a hand-written implementation). A load reads into the values already there, `Option`s included, so these stay as they are.
- When what save states store changes, `cargo test --workspace` and `cargo save-states` fail and list what was added and removed (the check is in `tools/nubo-save-state-check`, which a plain `cargo test` doesn't build); keep a new set with `cargo save-states --record` in the same pull request (see `tools/nubo-save-state-check/AGENTS.md`).
- A loaded state must go on exactly like the run it was saved from. `a_loaded_state_restores_every_field_of_the_cpu_the_address_bus_the_ppu_and_the_apu` (in `emulator/tests.rs`, with the fields left out on purpose) and `cargo benchmark --check-save-states` catch a field that is missed.
- The PPU, the APU and the address bus (its time since power-on and PPU dot debt) check every loaded value a damaged state could index or overflow with, and a failed load leaves the emulator untouched: input from players never makes the core panic.
- Run-ahead and rewind in RetroArch save and load a state every frame, so both stay fast: lists of bytes are written and read in one go (`Serializable::serialize_items`), and the power-on state a load starts from is made once per game (`PowerOnState`).
- An MBC3 clock's `base_timestamp` is a time on the clock it follows (`RtcClock`), which states store as `rtc_clock`; a state made on the other clock is moved over to this emulator's when it loads, keeping what the clock shows (`RtcState::rebase`).

## Accuracy References

- **SameBoy** (Expat license, notice in `apu.rs`) is the main reference, and several parts are ported from it: the pixel pipeline (`gpu/pipeline.rs`), the APU, write timing, DMA bus conflicts, the speed switch, STOP mode, the serial clock and the Super Game Boy (`sgb.rs`). For DMA bus conflicts, Gambatte's hardware tests win where they disagree with SameBoy.
- **Gambatte** is GPL-2.0-only: a reference only (its test ROMs, traces of a local build), never a source of code or tables. The PPU's line start and WY checks, and the freeze lengths of the speed switch, are fitted to its tests.
- **Calibration:** the PPU's dot positions against the Mooneye PPU tests and an instrumented SameBoy; each model's post-boot state against Mooneye's boot tests, GBMicrotest's poweron tests and Gambatte's display_startstate and ch1_init_pos tests. Change any of them only together with `cargo test-roms`. `clone_with_rom_matches_a_real_boot` checks that the test runner's boot cache gives the same state as a real boot.
