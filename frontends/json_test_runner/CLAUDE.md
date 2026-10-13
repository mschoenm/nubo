# JSON test runner

`cargo cpu-tests` runs the v2 tests of [adtennant/GameboyCPUTests](https://github.com/adtennant/GameboyCPUTests) in parallel, checking registers, the number of M-cycles, bus activity on every M-cycle and the final RAM. A missing or empty test folder, or a file whose test run panics, counts as a failure. The 241 files are downloaded at a pinned commit into `target/json-cpu-tests/` and checked against `v2.sha256`; pass a directory to use a local copy instead. The emulator runs in `processor_test_mode` (flat 64 KB RAM) for these tests.
