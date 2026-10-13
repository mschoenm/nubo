# Test ROM runner

`cargo test-roms` runs every test ROM suite on the hardware models each test is meant for, and compares the results with `passing.txt`. The module docs explain the suites, how each one passes, the boot cache and the hang detection.

```bash
cargo test-roms               # Every suite, about 30 seconds
cargo test-roms gambatte/oamdma   # Only tests whose name contains the filter
cargo test-roms --update      # Record this run's results in passing.txt
cargo test-roms --verbose     # Say why each test fails
cargo test-roms --verify-boot # Run the real boot ROM for every test instead of the boot cache (about 2.5 minutes; the nightly run does this)
```

## Working With It

- The run fails both when a listed test stops passing and when an unlisted one starts passing. When results change on purpose, run `cargo test-roms --update` and commit `passing.txt`.
- An unfiltered run also fails on listed tests that weren't run at all (a renamed ROM, a changed model rule); an unfiltered `--update` removes them, a filtered one leaves tests outside the filter alone.
- `FAILING_TESTS.md` explains every test that still fails, what was tried and what to try next. Update it when a fix makes one of them pass.
- Failing screenshot tests write the emulator's screen and a `.diff.png` (the reference with the differing pixels in red) to `target/test-roms/failures/<model>/`.
- The ROMs are downloaded at pinned versions and checked against checksums, into `target/test-roms/`. Results depend on their exact content, so they stay pinned.
- Timeouts count emulated time, so results don't depend on how busy the machine is.
- A new test suite needs:
  - its download in `download.rs` (pinned, with checksums);
  - its test cases in `suites.rs` (collected by `discover`);
  - its name in the suite list of `main.rs`'s module docs and in the README's Test ROMs section;
  - its download folder in the CI cache, `.github/actions/test-roms/action.yml` (`path`; the key already follows `download.rs`), or every CI run downloads it again.
- The fingerprint comparison in the root `AGENTS.md` runs over `target/test-roms/v7.0` only; pass the other download folders too when a change could affect their tests.
