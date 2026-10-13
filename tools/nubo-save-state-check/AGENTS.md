# Save state check

`cargo save-states` keeps players' save states loading across updates: it loads every state in `save-states/` (sets made by earlier versions), fails on any value it can't read, plays each on for 2 seconds, and fails when this version stores other fields or variants than the newest set's `layout.txt` lists. `main.rs` and `cartridges.rs` explain how; the rules for changing what save states store are in `crates/nubo-core/AGENTS.md`.

```bash
cargo xtask benchmark-roms    # Once: the games whose states are kept
cargo save-states             # Check the kept states and the layout
cargo save-states --record    # Keep a new set when the layout changed, named after today's date (UTC; a second set that day gets -2, and so on)
```

- When the layout changes on purpose, record a new set in the same pull request.
- Kept sets are never changed or removed.
- The test cartridges (`cartridges.rs`) never change, since a state only loads into the cartridge it was made with; `the_cartridges_never_change` pins them.
- The benchmark games whose states are kept stay at the same version in `xtask/src/benchmark_roms.rs`, for the same reason.
- `cargo test --workspace` (not a plain `cargo test`, since this crate isn't a default member) checks the test cartridges' states too (without the games), on Linux, macOS and Windows, so states are shown to load on every platform.
