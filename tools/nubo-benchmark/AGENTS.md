# Benchmark

`cargo benchmark` measures speed (the target is at least 20 times real time on both DMG and CGB) and fingerprints every frame (and the audio buffer it arrived with, which frontends pace themselves by), audio sample and end state, so a change can be shown to alter nothing at all. `main.rs` explains how.

```bash
cargo xtask benchmark-roms                                   # Download the free benchmark games into target/benchmark-roms
cargo benchmark target/benchmark-roms                        # Time them on DMG and CGB
cargo benchmark target/benchmark-roms --record before.txt    # Fingerprints before a change...
cargo benchmark target/benchmark-roms --compare before.txt   # ...and the check after it
cargo benchmark target/test-roms/v7.0 --seconds 4 --jobs <cores> --compare before.txt   # The same over all test ROMs (run cargo test-roms once to download them)
cargo benchmark <roms> --check-save-states                   # A state saved halfway goes on exactly like the run
cargo benchmark <roms> --record lazy.txt                     # Then the lazy shortcuts against running every M-cycle on its own:
cargo run --release -p nubo-benchmark --features nubo-core/eager -- <roms> --compare lazy.txt
```

Record and compare with the same ROMs, seconds and models; `--shard 2/4` runs only the second of four equal parts of the ROMs, as the nightly run does to split the work over machines.
