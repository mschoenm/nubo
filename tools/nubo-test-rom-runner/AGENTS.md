# Test ROM runner

`cargo test-roms` runs every test ROM suite on the hardware models each test is meant for, and compares the results with `passing.txt`. The module docs explain the suites, how each one passes, the boot cache and the hang detection.

```bash
cargo test-roms               # Every suite, about 30 seconds
cargo test-roms gambatte/oamdma   # Only tests whose name contains the filter
cargo test-roms --update      # Record this run's results in passing.txt
cargo test-roms --verbose     # Say why each test fails
cargo test-roms --verify-boot # Run the real boot ROM for every test instead of the boot cache (about 2.5 minutes; the nightly run does this)
cargo test-roms shootout      # Only the GB Emulator Shootout's tests, with Nubo's score and rank on its published table
```

## Working With It

- The run fails both when a listed test stops passing and when an unlisted one starts passing. When results change on purpose, run `cargo test-roms --update` and commit `passing.txt`.
- An unfiltered run also fails on listed tests that weren't run at all (a renamed ROM, a changed model rule); an unfiltered `--update` removes them, a filtered one leaves tests outside the filter alone.
- `FAILING_TESTS.md` explains every test that still fails, what was tried and what to try next. Update it when a fix makes one of them pass.
- Failing screenshot tests write the emulator's screen and a `.diff.png` (the reference with the differing pixels in red) to `target/test-roms/failures/<model>/` (`failures/shootout/<model>/` for the Shootout's tests).
- The ROMs are downloaded at pinned versions and checked against checksums, into `target/test-roms/`. Results depend on their exact content, so they stay pinned.
- Timeouts count emulated time, so results don't depend on how busy the machine is.
- A new test suite needs:
  - its download in `download.rs`: a pinned version with checksums (like the shootout's `shootout.sha256`), an `ensure_*` function and a folder in `RomDirs`;
  - a `Source` variant and its test cases in `suites.rs` (collected by `discover`);
  - its name in the suite list of `main.rs`'s module docs and in the README's Test ROMs section;
  - its download folder in the CI cache, `.github/actions/test-roms/action.yml`: in `path`, and its checksum file in `key` (which lists `download.rs` and each checksum file), or every CI run downloads it again.
- The GB Emulator Shootout's tests (`shootout.rs`) run as its harness judges them, under the IDs `shootout/<name as the Shootout writes it>`; the collection's copies of the same ROMs keep running with their own checks.
  - `shootout/list.rs` was generated once, by a throwaway script, from the Shootout's `testroms/*.py` and `main.py` at the pinned `main` commit (`SHOOTOUT_COMMIT` in `download.rs`), without running its Python; `shootout.rs`'s module docs give the rules. When that pin moves, regenerate the list and `shootout.sha256` together: a ROM may come from the collection only when its SHA-256 matches the Shootout's, and everything else the tests use is downloaded and listed in `shootout.sha256`.
  - `shootout_published.txt` is the Shootout's published table. `cargo xtask shootout-scoreboard <commit>` rewrites it from the `gh-pages` branch; update it in a pull request of its own when the Shootout publishes again. After the counts per suite and model and the total, a run that includes every Shootout test prints `  shootout  261/264  rank 1  leader DocBoy 255  published 2026-06-04 72fd745`, counted on the published tests only; the nightly run reads that line into its metrics, so keep its format.
  - `HARNESS_SPEED` and `HARNESS_STARTUP_MILLISECONDS` must stay the `speed` and `startup_time` of the Shootout's harness class for Nubo (`.github/shootout/nubo.py`, `docs/SHOOTOUT_PLAN.md` Part 2).
- The fingerprint comparison in the root `AGENTS.md` runs over `target/test-roms/v7.0` only; pass the other download folders too when a change could affect their tests.
