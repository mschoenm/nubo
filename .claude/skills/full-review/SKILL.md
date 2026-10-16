---
name: full-review
description: Review the whole Nubo repository for bugs, breaks of the AGENTS.md rules, idiomatic Rust, style and docs, with ten area reviewers, fixes on five theme branches, a second reviewer in its own cloud session, fuzzing and a report. Use only when the maintainer explicitly asks for a full review; never start one on your own.
---

# Full review

A full review reads every file of the repository, fixes what is wrong and small, files what needs a decision as issues, and writes a report. It costs a lot of tokens, so **it runs only when the maintainer asks for one**: no routine, skill or agent starts it, the nightly review included. It always covers the whole repository; the nightly review covers each day's changes.

The files next to this one:
- `areas.txt`: the ten areas;
- `review.py`: the checks for areas and the ledger;
- `area-brief.md`, `worker-brief.md` and `second-reviewer-brief.md`: the briefs for the three kinds of helpers;
- `report-template.md`: the report's sections.

Keep a work folder in the scratchpad, with `PROGRESS.md` (decisions, branches, commits, what is next, so the review can go on after a long pause), `findings/`, `ledger.md` and the baseline. Update `PROGRESS.md` after every step.

## 1. Start

Ask the maintainer once, in one question each with the default first:

- **Fixes:** only small, safe ones (default), or larger ones too?
- **Pull requests:** one per theme (default; the five themes are in step 5)?
- **Issues:** one for each real finding that isn't fixed, labelled `code-review` (default)?
- **Codex** on the pull requests: as the `merge` skill says (default), or not?

These are never asked, because they're fixed:
- **Scope:** the whole repository.
- **Second reviewer:** a separate cloud session.
- **Fuzzing:** always.

List the open pull requests and the files they change. A file an open pull request changes is left alone until it merges. Ask the maintainer whether to wait for it, then do the findings that need it after it merges, from the new `main`.

Read the last report in `docs/reviews/` (the list is `docs/reviews/README.md`), the open issues labelled `code-review` and the open issues labelled `nightly`.

## 2. Set up and record a baseline

1. Install what the root `AGENTS.md` lists for Linux, then:
   - `rustup update` and `rustup target add wasm32-unknown-unknown`;
   - `rustup toolchain install nightly`;
   - `cargo install --locked cargo-deny cargo-fuzz typos-cli cargo-machete cargo-llvm-cov cargo-bloat`.
2. `cargo xtask benchmark-roms`, then `cargo test-roms`, which must match `passing.txt`.
3. Record the fingerprints of `main` in the work folder's `baseline/`:
   - `cargo benchmark target/benchmark-roms --record baseline/games.txt`;
   - `cargo benchmark target/test-roms/v7.0 --seconds 4 --jobs 4 --record baseline/testroms.txt`.
4. In `fuzz/`, `cargo update`.

Record the baseline again whenever `main` moves under the review and the fixes are rebased onto it.

## 3. Review the areas

1. Run `python3 .claude/skills/full-review/review.py areas`. If a file is in no area, add its folder to `areas.txt` (in the docs branch) before going on.
2. Start ten area reviewers at once, one per area, each with `area-brief.md`. Give each:
   - its area's files;
   - the findings file to write, `findings/<AREA>.md`;
   - the open `code-review` and `nightly` issues;
   - the last report's notes on its area.

## 4. Verify every finding and keep a ledger

Read each finding, then the code it names, yourself. Write one line per finding in `ledger.md`: `- <ID> <OUTCOME>: <what was done, or why>`. The outcome is one of these:

| Outcome | Meaning |
|---|---|
| `FIX-docs`, `FIX-style`, `FIX-idioms`, `FIX-bugs` or `FIX-tooling` | Fixed on that theme's branch |
| `ISSUE` | Real, but needs a decision or a bigger change. Filed as an issue; the line names its number. |
| `NOTE` | Real but too small or too much a matter of taste for an issue; it goes in the report. |
| `MOOT` | `main` fixed it or made it moot meanwhile. |
| `WRONG` | It doesn't hold. The line says why. |

A finding that is partly wrong keeps its real part and says which part didn't hold. When a worker skips a finding or a reviewer overturns one, change its line. `python3 .claude/skills/full-review/review.py ledger <work folder>` must pass before the issues are filed, before the report is written and before the pull requests are opened.

## 5. Fix on five theme branches

| Theme | Branch | What |
|---|---|---|
| Docs | `<your branch>` | Docs and doc comments; the report |
| Style | `<your branch>-style` | The Style rules of `AGENTS.md` |
| Idioms | `<your branch>-idioms` | Idiomatic cleanups and dead code, behavior unchanged |
| Bug fixes | `<your branch>-fixes` | Small bug fixes and their tests; the nightly issues they fix |
| Tooling | `<your branch>-tooling` | The tools, xtask and the workflows |

How the fixes are made:
- **Workers:** each branch gets its own worktree (`git worktree add`) and a fix worker in the background with `worker-brief.md`. The worker gets its findings, the work folder, the baseline, the files to leave alone and the commit trailers.
- **The disk:** start at most two workers at a time, and check `df -h` between them. The disk filled up twice in the first review, with four workers building at once.
- **Nightly issues:** a nightly issue a branch fixes is named with `Fixes #<number>` in that pull request's description.
- **Outside the findings:** what a worker notices outside its findings is verified like a finding and gets its own ledger line, with the ID `EXTRA-<n>`.

## 6. The second reviewer

Start a separate cloud session as soon as the first branch has commits: `create_session` from the Claude Code Remote server, with `second-reviewer-brief.md` as its first message. Add the repository's branches and this session's ID, so it can answer with `send_message`.

Send it rounds:
- one per branch whenever a batch of commits is pushed;
- one for the findings that are only reported (`ISSUE` and `NOTE`);
- one for the merges of `main` into the branches.

Fix every PROBLEM it finds with a new commit, and include that commit in its next round. Fix the NITs that are right. A finding it overturns changes its ledger line.

## 7. Tool passes and fuzzing

Run these one at a time, each in the background with its output in the work folder:

- `typos`: fix the real typos on the style branch.
- `cargo machete` and `cargo deny check advisories`.
- `cargo clippy --workspace --all-targets -- -W clippy::pedantic -W clippy::nursery`: count by lint for the report. Check the few lints that can point at bugs, such as infinite iterators and formatting arguments; the rest are taste.
- `cargo llvm-cov --workspace --summary-only --ignore-run-fail`: line coverage per crate, without test files. Delete `target/llvm-cov-target` afterwards.
- `cargo bloat --release -p nubo --crates -n 40`, against the last nightly metrics' sizes.
- **Fuzzing,** as `fuzz/AGENTS.md` says: each target for 15 minutes, then `state` and `rom` for an hour more.
  - A crash in Nubo's code is a `FIX-bugs` finding with a regression test.
  - A crash in a target is fixed on the tooling branch.
  - A target that no longer builds is fixed on the tooling branch.

## 8. File the issues

One issue per `ISSUE` finding, labelled `code-review`, after searching the open issues for one that already covers it. Closely related findings can share one issue. Every issue says:
- **What:** what is wrong, and a concrete case;
- **Where:** the files and lines;
- **Fix proposed:** the fix, and why the review didn't make it;
- the finding's ID and the review's date.

Close earlier `code-review` issues that this review's fixes resolve, with `Fixes #<number>` in the pull request. Name in the report those that are still open.

## 9. The nightly check

When every branch has passed its checks and its second-reviewer rounds:

1. Make a branch `<your branch>-all` from `main` and merge the five branches into it. Note every conflict and its resolution for the report and the pull requests.
2. Run the full local checks on it, and compare the fingerprints with the baseline.
3. Push it, and start `nightly.yml` by hand on it and on `main` at the same moment, so both runs get the same day's machines.
4. Compare the two runs:
   - every job's result;
   - test ROMs and the GB Emulator Shootout;
   - instructions per emulated second, which mustn't rise beyond noise;
   - efficiency and build sizes.

   The Metrics job prints `night.json` in its log; read it from there when its artifact can't be downloaded. Wall-clock speed differs between shared machines, so compare instruction counts. A real speed doubt is settled by benchmarking both builds one after the other on one machine.

## 10. Report and pull requests

1. **The report:** write it from `report-template.md` as `docs/reviews/<date>.md`, add its line to `docs/reviews/README.md`, and commit both on the docs branch.
2. **The report page:** publish it as a private artifact (the Artifact tool, with its `artifact-design` skill), linking the pull requests and issues.
3. **The pull requests:** open the five, without waiting for each other, in their merge order:
   1. docs
   2. style
   3. idioms
   4. bug fixes
   5. tooling

   Each description says what the pull request does and what was checked, its place in the order, the known conflicts with the others and the issues it fixes. Subscribe to each.
4. **Seeing them through:** drive each to green as the root `AGENTS.md` says.
   - When one merges, merge `main` into the others that now conflict, run their checks and push.
   - Merge none of them until the maintainer asks; then follow the `merge` skill.
5. **Cleaning up:** delete the worktrees. Delete `<your branch>-all` if this session may; otherwise tell the maintainer to.

## Lessons from the first review

- **`Cargo.lock`:** a change to any `Cargo.toml` goes with the `Cargo.lock` a build writes, in the same commit. In the first review a feature change went without it, and every build afterwards rewrote the lock (fixed in #168).
- **Claims:** a finding's claim is verified in the code before it becomes a fix or an issue. Half of one finding didn't hold, and the issue had to say so.
- **The ledger:** keep it from the start. Without one, the first review could only account for its findings afterwards, by searching the workers' reports.
- **Damaged save states:** read loaded state for values the console can't be in, not just random bytes. The crashes the first review found came from save states with one or two fields set to such values; reading found them, and fuzzing didn't.
- **Tests:** a test that passes both with and without its fix proves nothing. Take the fix out and run the test once.
- **Merge order:** pull requests that change the same file conflict once the first merges. Name the conflicts before the maintainer merges.
- **Timing tests:** a test that depends on the machine's speed can fail under the slow coverage build. Run coverage with `--ignore-run-fail` and note the test in the report.
