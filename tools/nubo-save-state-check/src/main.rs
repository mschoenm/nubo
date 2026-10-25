//! Checks that save states made by earlier versions of Nubo still load, so
//! an update never costs a player their save states.
//!
//! `save-states/` in the repository keeps sets of states, each made by Nubo
//! on the day its folder is named after, of the test cartridges in
//! `cartridges.rs` and three free homebrew games. A new set is only needed
//! when what save states store changes (their layout: every field with how
//! its value is stored, and every variant of every enum value): the check fails when this version's layout
//! isn't the one the newest set lists, and `cargo save-states --record` then
//! adds a set. Every state of every set must load into this version without a
//! value it can't read, and play on.
//!
//! `cargo test --workspace` checks the test cartridges' states on every
//! platform CI runs it on, so states made on one platform are shown to load on
//! the others, and `cargo save-states` checks the games' states too. The test
//! cartridges cover the whole layout: which fields a state stores depends only
//! on the cartridge's controller, never on what the game did, and every
//! variant of an enum is listed whichever one a state holds, as are the fields
//! of an empty `Vec`'s items and of a `None`.
//!
//! The games aren't part of Nubo: `cargo xtask benchmark-roms` downloads them
//! to `target/benchmark-roms` (under `CARGO_TARGET_DIR` when that is set),
//! where this looks for them. A game whose states
//! are kept must stay in that list (`xtask/src/benchmark_roms.rs`) at the same
//! version, since states of another version of a game don't load. Kept sets
//! are never changed or removed.

use clap::Parser;
use nubo_core::{Emulator, Key, Model, NoSaves, RtcClock};
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

/// The sample rate the core makes sound at, in samples per second.
const SAMPLE_RATE: u32 = 44100;
/// The audio buffer the core fills on each `step_until_next_audio_buffer`.
const SAMPLES_PER_BUFFER: u32 = 512;

/// The games states are kept of, and the models each one runs on. Tobu Tobu
/// Girl Deluxe supports the Super Game Boy, so its SGB state holds a game's
/// colors and border.
const GAMES: [(&str, &[Model]); 3] = [
    ("tobudx.gb", &[Model::Dmg, Model::Sgb, Model::CgbE]),
    ("ucity.gbc", &[Model::CgbE]),
    ("geometrix.gbc", &[Model::CgbE]),
];
/// How long a game plays, with `scripted_keys`, before its state is made:
/// past the title screens, into play.
const GAME_SECONDS: u32 = 20;
/// How long a test cartridge runs before its state is made: past the CGB
/// boot ROM's animation (about three seconds) and the cartridge's set-up.
const TEST_CARTRIDGE_SECONDS: u32 = 5;
/// How long a loaded state plays on to show that it works.
const CHECK_SECONDS: u32 = 2;
/// The file in each set that lists its states' layout, one entry a line.
const LAYOUT_FILE: &str = "layout.txt";

/// Cartridge hooks that start every game without save data and count the
/// real-time clock in emulated time, so every run is the same.
const NO_SAVES: NoSaves = NoSaves {
    rtc_clock: RtcClock::EmulatedTime,
};

/// Check that the save states earlier versions made still load
#[derive(Parser)]
#[command(name = "nubo-save-state-check")]
struct Args {
    /// Keep a new set of save states made by this version, because it stores
    /// other fields or variants than the newest set
    #[arg(long)]
    record: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let root = workspace_root();
    let sets = root.join("save-states");
    let roms = env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| root.join("target"), PathBuf::from)
        .join("benchmark-roms");
    let result = with_games(&roms).and_then(|cartridges| {
        if args.record {
            record(&sets, &cartridges)
        } else {
            check(&sets, &cartridges).map(|_| ())
        }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// The folder of the repository, two levels above this crate.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate is in tools/")
        .to_path_buf()
}

/// A cartridge whose states are kept: a test cartridge or a game.
struct Cartridge {
    file: &'static str,
    rom: Vec<u8>,
    models: &'static [Model],
    /// How long it runs before its state is made.
    seconds: u32,
    is_test_cartridge: bool,
}

/// The test cartridges.
fn test_cartridges() -> Vec<Cartridge> {
    cartridges::CARTRIDGES
        .iter()
        .map(|cartridge| Cartridge {
            file: cartridge.file,
            rom: cartridge.rom(),
            models: cartridge.models,
            seconds: TEST_CARTRIDGE_SECONDS,
            is_test_cartridge: true,
        })
        .collect()
}

/// The test cartridges, and the games read from `roms`.
fn with_games(roms: &Path) -> Result<Vec<Cartridge>, String> {
    let mut cartridges = test_cartridges();
    for (file, models) in GAMES {
        let rom = fs::read(roms.join(file)).map_err(|error| {
            format!(
                "failed to read {file} from {}: {error} (cargo xtask benchmark-roms downloads it)",
                roms.display()
            )
        })?;
        cartridges.push(Cartridge {
            file,
            rom,
            models,
            seconds: GAME_SECONDS,
            is_test_cartridge: false,
        });
    }
    Ok(cartridges)
}

/// A save state this version makes.
struct MadeState {
    file_name: String,
    data: Vec<u8>,
}

/// Makes a state of every cartridge on each of its models.
fn make_states(cartridges: &[Cartridge]) -> Result<Vec<MadeState>, String> {
    let mut states = Vec::new();
    for cartridge in cartridges {
        for &model in cartridge.models {
            let mut emulator = start(model, cartridge)?;
            play(&mut emulator, 0, cartridge.seconds);
            states.push(MadeState {
                file_name: state_file_name(cartridge.file, model),
                data: emulator.save_state(),
            });
        }
    }
    Ok(states)
}

/// The layout of this version's states of the test cartridges, which is the
/// layout of every state.
fn stored_layout(cartridges: &[Cartridge]) -> Result<BTreeSet<String>, String> {
    let mut layout = BTreeSet::new();
    for cartridge in cartridges
        .iter()
        .filter(|cartridge| cartridge.is_test_cartridge)
    {
        for &model in cartridge.models {
            layout.extend(start(model, cartridge)?.save_state_layout());
        }
    }
    Ok(layout)
}

/// Keeps the states this version makes as a new set in `sets`, named after
/// today's date, unless the newest set already has the same layout.
fn record(sets: &Path, cartridges: &[Cartridge]) -> Result<(), String> {
    let layout = stored_layout(cartridges)?;
    if let Some(newest) = kept_sets(sets)?.last()
        && read_layout(newest)? == layout
    {
        return Err(format!(
            "This version stores the same fields and variants as {}, so it needs no new set",
            newest.display()
        ));
    }
    let states = make_states(cartridges)?;

    let today = today();
    let folder = (1..)
        .map(|count| match count {
            1 => sets.join(&today),
            count => sets.join(format!("{today}-{count}")),
        })
        .find(|folder| !folder.exists())
        .expect("one of the names is free");
    fs::create_dir_all(&folder)
        .map_err(|error| format!("failed to create {}: {error}", folder.display()))?;
    for state in &states {
        let path = folder.join(&state.file_name);
        fs::write(&path, &state.data)
            .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    }
    let path = folder.join(LAYOUT_FILE);
    let list: String = layout.iter().map(|entry| format!("{entry}\n")).collect();
    fs::write(&path, list)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    println!(
        "Kept {} save states in {}: commit them, and never change them",
        states.len(),
        folder.display()
    );
    Ok(())
}

/// Checks that every state of every set in `sets` loads and plays on, and
/// that this version has the layout the newest set lists. When `cartridges`
/// holds only the test cartridges (as in `cargo test`), the games' states are
/// left out. Returns how many states it checked.
fn check(sets: &Path, cartridges: &[Cartridge]) -> Result<usize, String> {
    let kept = kept_sets(sets)?;
    let Some(newest) = kept.last() else {
        return Err(format!(
            "{} holds no save states: run cargo save-states --record",
            sets.display()
        ));
    };

    let mut checked = 0;
    let mut failures = Vec::new();
    for set in &kept {
        for file in state_files(set)? {
            let name = format!("{}/{}", file_name(set), file_name(&file));
            if is_of_a_game(&file)
                && !cartridges
                    .iter()
                    .any(|cartridge| !cartridge.is_test_cartridge)
            {
                continue;
            }
            checked += 1;
            match check_state(&file, cartridges) {
                Ok(()) => println!("ok      {name}"),
                Err(error) => {
                    println!("FAILED  {name}: {error}");
                    failures.push(name);
                }
            }
        }
    }
    if !failures.is_empty() {
        return Err(format!(
            "{} of {checked} kept save states don't load any more: {}",
            failures.len(),
            failures.join(", ")
        ));
    }

    let kept_layout = read_layout(newest)?;
    let layout = stored_layout(cartridges)?;
    if layout != kept_layout {
        return Err(layout_changed(newest, &kept_layout, &layout));
    }
    let sets = match kept.len() {
        1 => String::from("1 set"),
        count => format!("{count} sets"),
    };
    println!(
        "{checked} save states from {sets} load and play on, and this version has the layout of {}",
        file_name(newest)
    );
    Ok(checked)
}

/// Whether `file` is a state of one of the games.
fn is_of_a_game(file: &Path) -> bool {
    let name = file_name(file);
    GAMES
        .iter()
        .any(|(game, _)| name.starts_with(&format!("{game}-")))
}

/// Explains which fields and variants changed since the newest set and what
/// to do.
fn layout_changed(newest: &Path, kept: &BTreeSet<String>, layout: &BTreeSet<String>) -> String {
    let mut message = format!(
        "This version's save states store other fields or variants than the newest kept set, {}:\n",
        newest.display()
    );
    for entry in layout.difference(kept) {
        message.push_str(&format!("  added    {entry}\n"));
    }
    for entry in kept.difference(layout) {
        message.push_str(&format!("  removed  {entry}\n"));
    }
    message.push_str(
        "If a field or variant was only renamed in the code, keep the name it is stored \
         under with #[serializable(name = \"...\")] instead, and store a field whose type \
         changed under a new name (see serializable.rs). Otherwise run cargo save-states \
         --record and commit the new set, so later versions are checked against states of \
         this one.",
    );
    message
}

/// Loads the state in `file` into a new emulator with its cartridge, without
/// a value it can't read, and plays on for `CHECK_SECONDS`.
fn check_state(file: &Path, cartridges: &[Cartridge]) -> Result<(), String> {
    let (cartridge, model) = parse_state_file_name(file, cartridges)?;
    let state = fs::read(file).map_err(|error| format!("failed to read it: {error}"))?;
    let mut emulator = start(model, cartridge)?;
    let unreadable = emulator
        .load_state_with_report(&state)
        .map_err(|error| format!("it doesn't load: {error}"))?;
    if !unreadable.is_empty() {
        return Err(format!(
            "it loads, but these values can't be read: {}",
            unreadable.join(", ")
        ));
    }
    if play(&mut emulator, cartridge.seconds, CHECK_SECONDS) == 0 {
        return Err(String::from("it loads, but shows no picture after that"));
    }
    Ok(())
}

/// The sets in `sets`, oldest first.
fn kept_sets(sets: &Path) -> Result<Vec<PathBuf>, String> {
    let mut kept: Vec<PathBuf> = fs::read_dir(sets)
        .map_err(|error| format!("failed to read {}: {error}", sets.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    kept.sort_by_key(|path| set_order(&file_name(path)));
    Ok(kept)
}

/// The key that sorts the set named `name` among the others: its date, then
/// its number that day (1 for a day's first set, which has no `-<number>`).
/// Sorting the names as text would put `2026-10-14-10` before
/// `2026-10-14-2`.
fn set_order(name: &str) -> (String, u32) {
    match name.split_at_checked(10) {
        Some((date, "")) => (date.to_string(), 1),
        Some((date, rest)) => (
            date.to_string(),
            rest.strip_prefix('-')
                .and_then(|count| count.parse().ok())
                .unwrap_or(0),
        ),
        None => (name.to_string(), 0),
    }
}

/// The state files in `set`, sorted.
fn state_files(set: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files: Vec<PathBuf> = fs::read_dir(set)
        .map_err(|error| format!("failed to read {}: {error}", set.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "state")
        })
        .collect();
    files.sort();
    Ok(files)
}

/// The layout listed in the `LAYOUT_FILE` of `set`.
fn read_layout(set: &Path) -> Result<BTreeSet<String>, String> {
    let path = set.join(LAYOUT_FILE);
    let list = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    Ok(list.lines().map(String::from).collect())
}

/// The last part of `path`, such as `ucity.gbc`, or an empty name if it has
/// none.
fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// The name of the state of `file` on `model`, such as `ucity.gbc-CGB-E.state`.
fn state_file_name(file: &str, model: Model) -> String {
    format!("{file}-{}.state", model.name())
}

/// The cartridge and model of a state named by `state_file_name`.
fn parse_state_file_name<'a>(
    file: &Path,
    cartridges: &'a [Cartridge],
) -> Result<(&'a Cartridge, Model), String> {
    let name = file
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    cartridges
        .iter()
        .find_map(|cartridge| {
            let model = name
                .strip_prefix(cartridge.file)?
                .strip_prefix('-')?
                .parse()
                .ok()?;
            Some((cartridge, model))
        })
        .ok_or_else(|| {
            String::from("its name isn't <cartridge>-<model>.state for one of the cartridges")
        })
}

/// A new emulator of `model` with `cartridge` inserted.
fn start(model: Model, cartridge: &Cartridge) -> Result<Emulator, String> {
    let mut emulator = Emulator::new(model);
    emulator.set_sample_rate(SAMPLE_RATE);
    emulator
        .load_rom(&cartridge.rom, Box::new(NO_SAVES))
        .map_err(|error| format!("failed to load {}: {error}", cartridge.file))?;
    Ok(emulator)
}

/// Plays `seconds` of emulated time from second `from` of the script, and
/// returns how many frames the game showed.
fn play(emulator: &mut Emulator, from: u32, seconds: u32) -> u32 {
    let buffers = seconds * SAMPLE_RATE / SAMPLES_PER_BUFFER;
    let first = from * SAMPLE_RATE / SAMPLES_PER_BUFFER;
    let buffer_seconds = f64::from(SAMPLES_PER_BUFFER) / f64::from(SAMPLE_RATE);
    let mut frames = 0;
    for buffer in first..first + buffers {
        hold_keys(emulator, &scripted_keys(f64::from(buffer) * buffer_seconds));
        emulator.step_until_next_audio_buffer();
        if emulator.take_frame().is_some() {
            frames += 1;
        }
    }
    frames
}

/// The buttons the script holds at `seconds` of emulated time, as in the
/// benchmark: Start a few times to get past title screens and menus, then
/// running right while jumping.
fn scripted_keys(seconds: f64) -> Vec<Key> {
    let mut keys = Vec::new();
    if seconds < 10.0 {
        if seconds >= 4.0 && seconds % 2.0 < 0.15 {
            keys.push(Key::Start);
        }
    } else {
        keys.push(Key::Right);
        if seconds % 1.0 < 0.4 {
            keys.push(Key::A);
        }
        if seconds % 3.0 < 1.5 {
            keys.push(Key::B);
        }
    }
    keys
}

/// Every Game Boy button, to release the ones the script doesn't hold.
const ALL_KEYS: [Key; 8] = [
    Key::Down,
    Key::Up,
    Key::Left,
    Key::Right,
    Key::Start,
    Key::Select,
    Key::A,
    Key::B,
];

/// Presses `keys` and releases every other button.
fn hold_keys(emulator: &mut Emulator, keys: &[Key]) {
    for key in ALL_KEYS {
        if keys.contains(&key) {
            emulator.press_key(key);
        } else {
            emulator.release_key(key);
        }
    }
}

/// Today's date in UTC, such as `2026-10-13`.
fn today() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    date_of_day(seconds / 86_400)
}

/// The date `days` days after 1970-01-01, by Howard Hinnant's
/// `civil_from_days` (<https://howardhinnant.github.io/date_algorithms.html>).
fn date_of_day(days: u64) -> String {
    let days = days + 719_468;
    let era = days / 146_097;
    let day_of_era = days % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = era * 400 + year_of_era + u64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs with `cargo test --workspace` on every platform CI tests on, so
    /// states made on one platform must load on all of them.
    #[test]
    fn kept_states_load_and_the_newest_set_lists_this_versions_layout() {
        let sets = workspace_root().join("save-states");
        let checked = check(&sets, &test_cartridges()).unwrap();
        assert!(checked > 0);
    }

    #[test]
    fn dates_are_counted_from_1970() {
        assert_eq!(date_of_day(0), "1970-01-01");
        assert_eq!(date_of_day(11_016), "2000-02-29");
        assert_eq!(date_of_day(20_739), "2026-10-13");
    }

    #[test]
    fn sets_sort_by_date_then_number() {
        let mut names = [
            "2026-10-14-10",
            "2026-10-15",
            "2026-10-14-2",
            "2026-10-14",
            "2026-10-13-2",
            "2026-10-14-9",
        ];
        names.sort_by_key(|name| set_order(name));
        assert_eq!(
            names,
            [
                "2026-10-13-2",
                "2026-10-14",
                "2026-10-14-2",
                "2026-10-14-9",
                "2026-10-14-10",
                "2026-10-15",
            ]
        );
    }
}

mod cartridges;
