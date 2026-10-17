# Nightly review

Every night at 04:07 Amsterdam time a Claude cloud routine reviews the day's changes and one small part of the code in detail, fixes what is clearly better, and opens a pull request for the morning when it changed something. This file is its instructions; the routine's prompt only points here, so the job changes through pull requests like the code. It never starts a full review of the repository (the `full-review` skill): only the maintainer asks for one.

The nightly CI (`.github/workflows/nightly.yml`) runs first, at 01:17 Amsterdam time. Besides the slow test suites it measures speed, instructions and battery use (`cargo xtask benchmark-roms`, `nubo measure-efficiency`), appends them to the metrics log in the repository's wiki (`cargo xtask metrics`) and picks tonight's part (`cargo xtask review-units next`). CI is the only writer of the wiki, and only its scheduled runs write to it; the review only reads it.

## 1. Set up

Every run starts from a fresh clone of `main`.

```bash
sudo apt-get update && sudo apt-get install -y libasound2-dev libudev-dev pkg-config
rustup update stable && rustup target add wasm32-unknown-unknown
git fetch --unshallow origin || true
```

Commands taking longer than a minute or two (builds, `cargo test-roms`, benchmarks) go in the background with their output to a file, and you wait for them; foreground commands are cut off after a few minutes. The machine has 4 cores, so pass `--jobs 4` to `cargo benchmark`.

## 2. Find tonight's work

1. Read `AGENTS.md` first, especially its Rules and Style, and the `AGENTS.md` in the folders of the code you review (`crates/nubo-core/`, `crates/nubo-frontend/`, `frontends/*/`, `tools/*/`, `xtask/`), which describe it.
2. Clone the wiki: `git clone --depth 1 https://github.com/mschoenm/nubo.wiki.git /tmp/wiki`. Its `tonight.json` says what to review: `part.files` (the part to read in detail), `changes_since` (the commit the day's changes start after; `null` on the first night and after a night whose commit is no longer in main's history, such as after a force-push) and `changed_files`. Use it if its `date` is today (Amsterdam time, as the nightly CI dates it); an older one means tonight's nightly CI hasn't finished (step 5), so go on with step 3.
3. If the wiki or today's `tonight.json` is missing, pick the part yourself: copy `/tmp/wiki/rotation.json` to `/tmp/rotation.json` if it exists, run `cargo xtask review-units next --state /tmp/rotation.json --date "$(TZ=Europe/Amsterdam date +%F)" --commit "$(git rev-parse HEAD)"`, and say in your report that the wiki wasn't up to date.
4. The day's changes are `git log --reverse <changes_since>..origin/main`; without `changes_since`, the commits of the last 26 hours. Read their diffs, all files including docs. Beyond about 3,000 changed lines, review the files with the most changes and list the rest as not reviewed.
5. Read last night's CI: `gh api 'repos/mschoenm/nubo/actions/workflows/nightly.yml/runs?event=schedule&per_page=1'` gives the newest scheduled run (runs started by hand don't count), with its `run_started_at` (UTC), `status`, `conclusion`, `head_sha` and `html_url`. GitHub holds scheduled runs back, sometimes for hours, when Actions is busy, and under heavy load can drop one, so it is one of three cases:
   - **Tonight's run, finished** (started after tonight's 01:17 Amsterdam time, `status` `completed`): read its `conclusion` and the Regressions section of `/tmp/wiki/Nightly-metrics.md`. When it passed, `nightly-build.yml` should have published it as the `nightly` pre-release: the `nightly` tag points to its `head_sha` (`gh api repos/mschoenm/nubo/git/ref/tags/nightly --jq .object.sha`) and the release has 8 downloads, two Flatpaks, the Windows installer and zip and four RetroArch cores (`gh api repos/mschoenm/nubo/releases/tags/nightly --jq '.assets | length'`). If not, the newest `nightly-build.yml` run (read like the nightly run) says why, or that it is still running.
   - **Tonight's run, still going** (`status` `queued` or `in_progress`): review with the metrics and part the wiki has (step 3 if `tonight.json` is not today's). A night takes about 20 minutes, so judge it by how long ago `run_started_at` was, whenever it started: under an hour, report under Metrics that the nightly CI started late and is still running; over an hour, it is stuck: report the problem `Nightly: the CI run is stuck` with its link (section 6).
   - **No run since tonight's 01:17:** report under Metrics that the nightly CI is late and hasn't started yet, and review as in the case before.

   A late night is no problem, unless the night before had no scheduled run either (the newest one started before yesterday's 01:17): then report the problem `Nightly: the scheduled CI run didn't start` (section 6).
6. Look for an open pull request of an earlier night: `gh api 'repos/mschoenm/nubo/pulls?state=open&per_page=100' --jq '.[] | select(.head.ref | startswith("claude/nightly-")) | .number'`.

## 3. Review

Read every file of tonight's part in full, and the day's diffs. Check:

- **The style rules in AGENTS.md:** a short `///` comment on every type and non-trivial function that a junior programmer can follow, Game Boy terms explained in plain words, few `//` comments, whole-word names without `get_`, acronyms written as words (`RtcState`), modules declared at the bottom of the file, `#[expect(..., reason = "...")]` on the smallest item instead of `#[allow]`, no `unsafe`, and ported code credited at the top of the file, in `NOTICE` and in the README's Credits.
- **Clear, idiomatic Rust:** run `cargo clippy --workspace --all-targets -- -W clippy::pedantic` and look at what it says about tonight's files (a report only: the curated lints in `Cargo.toml` are what CI enforces). Beyond clippy: needless clones and allocations, hand-written loops an iterator says more clearly, unclear ownership or error handling, functions doing several things, names that don't say what a thing is.
- **Docs that drifted:** doc comments and comments that no longer match the code, references to functions, fields, constants or files that no longer exist (search for them), and the passages of the `AGENTS.md` files (the root one and the folders' ones), `docs/CI.md`, `README.md`, `BUILDING.md`, `tools/nubo-test-rom-runner/FAILING_TESTS.md` and `docs/ACCURACY_PLAN.md` that describe the reviewed code.
- **Bugs in the day's changes.**

Leave alone: dependency versions (Dependabot updates them), the timing calibration (dot positions, cycle counts and the like, which only test ROM work changes), `.github/workflows/`, `rust-toolchain.toml`, `tools/nubo-test-rom-runner/passing.txt` and the wiki.

## 4. Fix

Fix whatever you are sure makes the code clearer, more idiomatic, better documented or correct, however many files that takes, as long as every change keeps what the program does. A change that would alter behaviour, a bug fix whose effect you can't verify, and anything debatable become suggestions in the report instead. Keep each change easy to review: one subject per commit, with a message in the style of `git log` (a short title in the imperative, a body saying why) ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## 5. Verify

Before changing anything, record the emulator's output on the unchanged code:

```bash
cargo test-roms acid2   # downloads the test ROMs
cargo xtask benchmark-roms
cargo benchmark target/test-roms/v7.0 target/benchmark-roms --seconds 4 --jobs 4 --record /tmp/before.txt
```

After your changes, all of these must pass:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy -p nubo --target wasm32-unknown-unknown --locked -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items --locked
cargo test --workspace --locked
cargo test-roms
cargo cpu-tests
cargo benchmark target/test-roms/v7.0 target/benchmark-roms --seconds 4 --jobs 4 --compare /tmp/before.txt
```

The last one proves every frame, audio sample and end state is unchanged. A change that makes any check fail, or any fingerprint differ, is reverted and reported as a suggestion. When you changed `crates/nubo-core`, also run `cargo test-roms --verify-boot`. Changes to the Iced app can't be fingerprinted: there keep to changes that clearly keep behaviour, and rely on its tests.

## 6. Report

GitHub's GraphQL API is blocked in routines, so `gh pr create` and `gh issue` don't work: use the REST API with `gh api`, and write long text to a file and pass it with `-F body=@file`.

What still needs someone to act goes into issues, which stay open until it's done; the rest goes into the night's pull request:

- **Issues** (labelled `nightly`): every **problem** (a regressed metric, a failed or stuck nightly CI job, a nightly build that couldn't be published, a run of this review that couldn't finish), with its numbers, the CI run's link and the commits since the last good night; and every **bug** among the suggestions. A bug is anything the program does that it shouldn't, however small or rare: a crash or panic, memory safety, lost or damaged saves or save states, but also a question or dialog that disappears unanswered, a setting or choice that sticks after the action it belonged to failed, a command that refuses valid input or gives the wrong error, and anything that doesn't do what its docs or the UI say. When in doubt, it is a bug. Give each a short, fixed title that names it, such as `Nightly: the CI run is stuck` or `RetroArch core: save states stop working after a restart onto a Super Game Boy`, and say what you found, where (file and line) and the fix you'd propose.
- **The pull request:** everything else you didn't change stays in its Suggestions: code that works as it should but could be clearer, tidying, duplicated code, docs and small improvements. A finding never goes only into the Suggestions because its fix is small, debatable or would change behaviour; if it is a bug, it gets an issue, and the Suggestions may point to it.

Before opening an issue, look at the open ones: `gh api 'repos/mschoenm/nubo/issues?state=open&labels=nightly&per_page=100'` (pull requests show up there too; skip those with a `pull_request` field). When one already covers the finding, add tonight's numbers or details as a comment instead of opening another. Open a new one with `gh api repos/mschoenm/nubo/issues -f title="<title>" -F body=@/tmp/issue.md -f 'labels[]=nightly'`.

Each night, also read the open `nightly` issues against the day's changes: close one that a commit fixed, with a comment naming the commit (`gh api repos/mschoenm/nubo/issues/<number>/comments -F body=@/tmp/comment.md`, then `gh api -X PATCH repos/mschoenm/nubo/issues/<number> -f state=closed -f state_reason=completed`). Close only what you checked is fixed, and never an issue without the `nightly` label.

- **When you changed something and no earlier nightly PR is open:** push the branch `claude/nightly-<date>` (based on `origin/main`) and open a pull request with `gh api repos/mschoenm/nubo/pulls -f title="Nightly review <date>" -f head=claude/nightly-<date> -f base=main -F body=@/tmp/pr.md`. Its description has these sections:
  1. **Issues:** the issues you opened, commented on or closed tonight, each with its number and title; leave the section out when there are none.
  2. **Summary:** what changed and why, in a few lines.
  3. **Changes:** a table of file, what changed and why.
  4. **Verification:** the commands above and their results, including that the fingerprints matched.
  5. **Metrics:** last night against the 14-night medians from the wiki page, with anything flagged.
  6. **Suggestions:** what you didn't change and why, most important first, apart from the problems and bugs that went into issues.
  7. **Reviewed:** the commit range of the day's changes and tonight's part, with its files.
- **When an earlier nightly PR is still open:** make no new changes; open or update tonight's issues as above, and add the rest of tonight's findings to the pull request as a comment, listing the issues first: `gh api repos/mschoenm/nubo/issues/<number>/comments -F body=@/tmp/comment.md`.
- **When there is nothing to change:** no pull request; the issues, if any, are the night's report.
- **When the run can't finish** (setup fails, a tool is missing): open the issue `Nightly review failed: <date>` saying where and why.

Never merge, push to `main`, force-push, delete branches, write to the wiki, or close an issue you haven't checked is fixed. End with a short summary of what you reviewed, what you changed and what you reported.
