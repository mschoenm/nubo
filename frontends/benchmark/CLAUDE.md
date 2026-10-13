# Benchmark

Runs ROMs (files or folders) on each chosen model for a fixed stretch of emulated time with scripted button presses, and reports the speed as emulated time per second spent inside the core (20x real time is the minimum target on both DMG and CGB). It also hashes every frame, every audio sample and the final save state; `--record` writes these fingerprints and `--compare` checks a later build against them, so an optimization can be shown to change nothing at all, across the games and all test ROMs, beyond what the test ROMs' pass/fail results show. `--shard 2/6` runs only the second of six equal parts of the ROMs (every sixth, so each folder is spread over all parts), which the nightly CI uses to split its checks over several machines.

```bash
cargo benchmark ~/Roms/GameBoy                       # Time every game in a folder on DMG and CGB
cargo benchmark ~/Roms/GameBoy --record before.txt   # Record fingerprints of frames, audio and end state
cargo benchmark ~/Roms/GameBoy --compare before.txt  # Check a later build produces exactly the same
cargo benchmark ~/Roms/GameBoy --check-save-states   # Check a save state taken halfway goes on exactly like the run
cargo benchmark ~/Roms/GameBoy --shard 2/4 --record part2.txt   # Only the second of four equal parts of the ROMs
cargo benchmark target/test-roms/v7.0 --seconds 4 --jobs 12 --compare before.txt   # Same for all test ROMs
cargo benchmark target/test-roms/v7.0 --seconds 6 --jobs 12 --check-save-states   # Save states of all test ROMs
cargo run --release -p nubo-benchmark --features nubo-core/eager -- ~/Roms/GameBoy --compare lazy.txt   # Check the lazy shortcuts against running every M-cycle on its own
```
