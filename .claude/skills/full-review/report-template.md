# Report template

Each full review's report is `docs/reviews/<YYYY-MM-DD>.md`, dated by the day the review started. Write it for the maintainer, in the repository's plain style. `docs/reviews/2026-10-16.md` is the first one and the example to follow. Its sections:

1. **Title and opening paragraph:**
   - the date and the commits of `main` reviewed (from, and up to where `main` moved during the review);
   - what was looked for;
   - what was fixed, and what was filed as issues instead;
   - the earlier review's open issues and the nightly issues the review took in.
2. **How it was done:**
   - the ten areas;
   - the number of findings per category;
   - the ledger: how many findings were fixed, filed, noted, moot or wrong;
   - the second reviewer's rounds and what they caught;
   - its stress tests;
   - the proof that emulation didn't change: test ROMs against `passing.txt`, the CPU tests, the kept save states and the fingerprints against the baseline.
3. **The pull requests:** a table of number, theme, branch, commits and what, in their merge order. Then the bugs fixed, the most important first.
4. **Found and not fixed:**
   - a table of the issues filed, the most important named first;
   - the smaller notes without an issue;
   - the earlier issues that are still open.
5. **Tools and other checks:**
   - the spell checker, unused dependencies and advisories;
   - pedantic and nursery Clippy, as counts;
   - fuzzing: targets, how long each ran, runs, crashes;
   - coverage per crate;
   - binary sizes, against the last nightly metrics.
6. **The nightly checks:** the hand-started nightly run on the branch merging every pull request, next to the run on `main` started at the same time, with links. Compare instruction counts, which don't depend on the machine, rather than wall-clock speed.
7. **Merging:** the order, and each conflict between the pull requests with how to resolve it.
8. **Recommendations:** at most a handful, the most important first.

Add the review's line to the table in `docs/reviews/README.md` in the same pull request as the report, and bring the earlier rows' Still open column up to date (the issues of each still open, with today's date).
