# Save state check

Keeps players' save states loading across updates. The save-state rules every change follows are in the root `CLAUDE.md`, the format itself in `crates/nubo-core/CLAUDE.md`.

```bash
cargo save-states             # Check that the kept save states (save-states/) still load, and that this version stores the fields and variants the newest set lists
cargo save-states --record    # Keep a new set of save states, after a change to the fields or variants save states store
```

- `save-states/<date>/` holds sets of states made by the version of that day, of six test cartridges the tool builds itself (`cartridges.rs`: one per memory bank controller, each setting up the sound, timer, serial port, objects, window and OAM DMA, and on the CGB its palettes, banks, VRAM DMA and double speed; pinned by `the_cartridges_never_change`, since a state only loads into the cartridge it was made with) and of the three games of `cargo xtask benchmark-roms` 20 seconds in, with `layout.txt` listing every field that version stores with how its value is stored and every variant each enum value can be stored as.
- Which fields a state stores depends only on the cartridge's controller, and every variant is listed whichever one a state holds, as are the fields of an empty `Vec`'s items and of a `None`, so the test cartridges cover the whole layout.
- `cargo save-states` loads every kept state with `load_state_with_report`, fails on any value it can't read, plays each on for 2 seconds, and fails when this version's layout isn't the one the newest set lists; `cargo test` does the same for the test cartridges' states without the games, which the nightly CI runs on Linux, macOS and Windows, so states made on one platform are shown to load on the others.
- `--record` adds a set named after today's date (UTC) only when the layout changed. Sets are never changed or removed, and a game whose states are kept must stay in `benchmark_roms.rs` at the same version (states of another version of a game don't load).
