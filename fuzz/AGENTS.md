# Fuzz targets

Fuzzing feeds Nubo huge numbers of generated inputs, changing them toward the ones that reach code no input reached before, to find any that make it panic. It tests the first rule of the root `AGENTS.md`: user input never panics the core. A full review (`.claude/skills/full-review/SKILL.md`) runs these targets; nothing else does, CI included.

This folder is its own Cargo workspace, outside Nubo's, because `cargo-fuzz` needs the nightly toolchain (`rust-toolchain.toml` here). It is the one exception to the rule that Nubo uses only stable Rust: no code that ships or that CI builds may depend on it. `cargo build`, `cargo test`, `cargo fmt --all` and clippy at the root never see it.

## The targets

- `rom`: any ROM or zip file, on a model picked by the first byte.
- `state`: any save state of Tobu Tobu Girl Deluxe, one of the benchmark games.
- `state_fields`: the game's kept save states (`save-states/`) with whole fields changed, one or several at a time, to zero, their largest value or bytes from the input. It finds every field by walking the state's names and lengths, so it needs no list of them and keeps up as fields are added. Changing random bytes, as `state` does, mostly breaks the format and is rejected early; the crashes found so far needed one or two fields the console can never hold.
- `cheats`: any text of GameShark and Game Genie codes, registered while the game runs.
- `battery`: any battery save (cartridge RAM of any length and any cartridge clock) on every cartridge type.

Each one runs what it loaded for a couple of frames, saves a state and loads it again. `src/lib.rs` holds what they share.

## Running them

From the repository root, once: `cargo install cargo-fuzz --locked`, `rustup toolchain install nightly` and `cargo xtask benchmark-roms` (the `state` and `cheats` targets load the game from `target/benchmark-roms/`). Then from this folder:

```bash
mkdir -p corpus/state corpus/rom && cp ../save-states/<newest set>/tobudx.gb-*.state corpus/state/
for game in ../target/benchmark-roms/*; do (printf '\0'; cat "$game") > corpus/rom/"${game##*/}"; done
python3 -c 'import sys, zipfile; [zipfile.ZipFile(f"/tmp/{name}.zip", "w", method).write(sys.argv[1], "game.gb") for name, method in (("stored", zipfile.ZIP_STORED), ("deflated", zipfile.ZIP_DEFLATED))]' ../target/benchmark-roms/tobudx.gb
for zip in stored deflated; do (printf '\0'; cat /tmp/$zip.zip) > corpus/rom/tobudx-$zip.zip; done
cargo fuzz build
cargo fuzz run state_fields -- -max_total_time=900    # 15 minutes; the same for state, rom, cheats and battery
```

Seed `state` with the game's kept states, or it starts from nothing and rarely gets past the header; `state_fields` reads them itself. Seed `rom` with real games too, plain and zipped (stored and compressed), each after one byte that picks the model: a header the boot ROM rejects is skipped, and every new header costs a boot (`start` in `src/lib.rs`), so changes inside a game are what run fast. `corpus/`, `artifacts/` and `target/` are not committed.

## When one crashes

`cargo fuzz` saves the input in `artifacts/<target>/`. `cargo fuzz tmin <target> <file>` makes it as small as it can. A crash in Nubo's own code is a bug against the first rule: it gets a regression test in `nubo-core` that loads the same input, written like the corrupted save-state tests, and then a fix.

## Upkeep

The full review runs `cargo update` here at its start, so the targets follow their newest dependencies like the rest of Nubo. When `nubo-core`'s API changes, the review fixes the targets in the same pull request as its other changes.
