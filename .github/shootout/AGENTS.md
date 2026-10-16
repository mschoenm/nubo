# The GB Emulator Shootout's harness class

`nubo.py` is Nubo's class in the GB Emulator Shootout: the file the Shootout runs as
`emulators/nubo.py`, sent there in
[gbdev/GBEmulatorShootout#102](https://github.com/gbdev/GBEmulatorShootout/pull/102) from the
maintainer's fork (branch `add-nubo` of `mschoenm/GBEmulatorShootout`). The Shootout workflow
(`.github/workflows/shootout.yml`) copies it into a checkout of the Shootout and runs every test with
it, so CI checks exactly what the Shootout runs.

**Below its header comment, `nubo.py` is the Shootout's copy byte for byte; don't change it here
alone.** That covers its comments and line breaks too: don't reword, rewrap or restyle them, whatever
a review, a linter or the Style rules in the root `AGENTS.md` say, since the Shootout's copy keeps its
own wording. The header comment (the lines before `from util import *`) is Nubo's own and isn't sent.

When the class really must change (a new command-line option, a new release file name), change it in
both places together: here, and on the fork's branch (or in a new pull request to the Shootout once
#102 is merged). Keep `speed` and `startup_time` equal to `HARNESS_SPEED` and
`HARNESS_STARTUP_MILLISECONDS` in `tools/nubo-test-rom-runner/src/shootout.rs`. The rest of the
plan is in `docs/SHOOTOUT_PLAN.md` (Parts 2 to 4).
