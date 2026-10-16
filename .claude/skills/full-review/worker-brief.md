# Fix worker's brief

You fix a list of findings from a full review (`.claude/skills/full-review/SKILL.md`) on one theme branch, one commit per finding. Your task names the branch, its worktree, the work folder that holds `findings/<AREA>.md`, the findings to fix and any files you must leave alone.

## Before you start

- Work only in your own git worktree. Never touch the main checkout or another worker's worktree.
- Read the root `AGENTS.md` and the `AGENTS.md` of each folder you change, before changing code there.
- For each finding, read it, then read the code and check it yourself. Skip it, and say why in your final reply, when:
  - it is wrong;
  - `main` already fixed it or made it moot;
  - its fix would change emulation output, the save-state layout or what players see beyond what the finding is about;
  - it needs a file your task says to leave alone.

## How to fix

- **The smallest change** that fixes the finding. No changes beside it. Match the code around it.
- **Save states:** never rename a stored field or variant without keeping its stored name (`#[serializable(name = "...")]`), and never change a stored type. Don't rename `nubo-core`'s public API.
- **Tests:** a bug fix gets a regression test whenever one is cheap. Check it fails without the fix: take the fix back out, run the test, put the fix back.
- **Loaded state:** a finding about a loaded file or save state gets a test that loads a crafted one, written like the corrupted save-state tests in `nubo-core`.
- **`Cargo.lock`:** a change to any `Cargo.toml` (features included) goes in the same commit as the `Cargo.lock` that a build then writes. Check with `git status` that a build leaves nothing changed.

## Commits

One commit per finding; one that spans several files is still one commit.

- **The subject** is a plain imperative sentence, like the repository's history ("Give the current speed in the README and ACCURACY.md"), without tags or finding IDs.
- **The body** says why in one to three sentences, and may end with `Review finding: <ID>`.
- **The trailers** follow a blank line, exactly as your task gives them.

Never amend, rebase or force-push commits already pushed.

## Checks

Use `CARGO_INCREMENTAL=0`. Run them after every few commits and at the end; all must pass:

1. `cargo fmt --all`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo clippy -p nubo --target wasm32-unknown-unknown -- -D warnings`, if you touched `frontends/nubo` or `nubo-frontend`.
4. `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items`
5. `cargo test --workspace`
6. `cargo xtask build-web --quick`, if you touched the app.
7. If you touched code (not only docs) in `nubo-core`, all three of these:
   - `cargo test-roms` gives the totals of `tools/nubo-test-rom-runner/passing.txt` with nothing newly failing;
   - `cargo cpu-tests` passes;
   - the fingerprints match the baseline your task names: `cargo benchmark target/benchmark-roms --compare <baseline>/games.txt` and `cargo benchmark target/test-roms/v7.0 --seconds 4 --jobs 2 --compare <baseline>/testroms.txt`. A difference means your change altered emulation: take it back unless the finding is about changing output, and report it.
8. `cargo save-states`, if anything save states store or check changed.
9. `cargo deny check licenses bans sources`, if a dependency or a feature changed.

Copy the test ROMs and benchmark games from the main checkout's `target/` into your worktree's `target/` instead of downloading them again.

**The disk is shared and fills up:**
- run one release, coverage or benchmark build at a time;
- delete your worktree's `target/debug` and `target/doc` once their checks have passed.

## Push and report

When every check passes, `git push -u origin <your branch>`. Retry up to four times on a network error, after 2, 4, 8 and 16 seconds.

Your final reply:
- a table: finding, commit, one line on what you did;
- the skipped findings, each with its reason;
- the summary lines of the checks;
- anything you noticed outside your findings, as a note for the coordinator, not fixed.
