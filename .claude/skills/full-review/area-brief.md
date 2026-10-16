# Area reviewer's brief

You review one area of Nubo, a cycle-accurate Game Boy and Game Boy Color emulator in Rust, as part of a full review (`.claude/skills/full-review/SKILL.md`). Your task names the area, the files in it (from `review.py areas`), the repository's checkout and the file to write your findings to.

This is a read-only review. Don't edit, create or delete files in the repository, don't run git commands that change anything, and don't build or test (other jobs share the machine and its disk). Read with the file tools and read-only shell commands (`cat`, `sed -n`, `grep`, `find`, `wc`).

First read the root `AGENTS.md` (its Rules and Style are the yardstick) and the `AGENTS.md` of each folder you review. Then read the open issues your task lists: the ones labelled `code-review` (found by earlier full reviews) and `nightly`. Don't report what one of them already covers; say in your final reply which ones still apply in your area and which look fixed.

## What to look for

1. **Bugs:**
   - logic errors and off-by-ones;
   - integer overflow and underflow, including the panics of debug builds;
   - wrong masks and bit operations;
   - `unwrap`, `expect` and indexing that can fail;
   - panics reachable from user input (ROMs, zip files, save states, cheat codes, battery saves, settings and library files, the command line);
   - wrong error handling, races, leaks and wrong edge cases.
2. **Breaks of the Rules in `AGENTS.md`:**
   - user input that panics the core;
   - a new way to see or change hardware state that doesn't first run the waiting M-cycles and catch the PPU and APU up;
   - stored save-state names or types that changed, or `usize` in stored values;
   - `unsafe` outside `frontends/nubo-libretro/src/ffi.rs`;
   - std's `Instant`, `SystemTime` or `Duration` in code the browser build shares;
   - fonts other than `theme::BODY_FONT` and `theme::HEADING_FONT`;
   - code, tables or data ported without credit, and anything that came from Gambatte.
3. **Idiomatic Rust:**
   - needless clones and allocations;
   - `String` where `&str` fits;
   - loops an iterator says more clearly;
   - needless `Box`, `Rc` or `RefCell`;
   - confusing shadowing;
   - dead code;
   - `pub` that could be private.

   Not matters of taste.
4. **The Style rules:**
   - a missing doc comment, or one that only repeats the name, is wrong or doesn't explain a Game Boy term;
   - too many `//` comments;
   - abbreviations, and `get_` prefixes;
   - modules not declared at the bottom of the file;
   - `#[allow]` instead of `#[expect(..., reason = "...")]`.
5. **Docs:** doc comments, comments and `AGENTS.md` text in your area that no longer match the code, broken intra-doc links, typos and unclear wording.

## Before you report a bug

Trace a realistic path from a real caller or input to the failure, reading the callers. If you can't make it concrete, drop it or mark its confidence low. Hardware quirks are often deliberate: check the comments and the tests. Timing values are calibrated against test ROMs, so never call a timing constant a bug without strong evidence. For a loaded file or save state, say exactly which values reach the failure.

## How to write a finding

Write your findings to the file your task names, in Markdown, one section each, numbered from 1:

```markdown
### <AREA>-<n>: <a short title that says what is wrong>
- category: bug | rule | idiom | style | docs
- severity: high | medium | low
- location: path/from/the/root.rs:LINE (more places if needed)
- confidence: high | medium | low
- what: what is wrong, in one to three sentences
- evidence: the path from the input or caller to the failure, with the code that proves it
- suggested fix: concrete; a code sketch is fine
- small-and-safe: yes | no
```

`small-and-safe` is `yes` only when the fix is a few lines and can't change emulation output, the save-state layout, `nubo-core`'s public API or what players see, unless the finding is a small, obvious bug whose fix is exactly that change. Fixes to docs, comments and typos are `yes`.

Read every file of your area in full, not excerpts. Quality over quantity, but report small real things too: a typo or a wrong doc comment counts.

## Your final reply

The number of findings in each category, the five most important in one line each, and the open issues that still apply or look fixed.
