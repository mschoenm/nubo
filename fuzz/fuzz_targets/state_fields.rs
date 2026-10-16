//! Save states of the benchmark game with whole fields changed, as a damaged
//! file or a crafted one can have them: one or several fields set to zero,
//! their largest value or bytes from the input. Changing random bytes, as the
//! `state` target does, mostly breaks the format and is rejected before
//! anything is checked; the crashes found so far needed one or two fields
//! that the console can never hold, in an otherwise good state.
//!
//! The input picks a kept state, then any number of changes, each three bytes
//! and then the bytes a change writes: which field (two bytes, counted
//! modulo the number of fields) and how to change it (one byte).
#![no_main]

use libfuzzer_sys::fuzz_target;
use nubo_core::SaveStateHeader;
use nubo_fuzz::{game, run_frames, start};
use std::sync::OnceLock;

/// Where the values of a state's fields are: the start and length of each
/// field's bytes, without the fields that only hold other fields.
struct Fields {
    state: Vec<u8>,
    values: Vec<(usize, usize)>,
}

/// The game's kept states, one per model, each with its fields.
fn kept_states() -> &'static [Fields] {
    static KEPT: OnceLock<Vec<Fields>> = OnceLock::new();
    KEPT.get_or_init(|| {
        let folder = concat!(env!("CARGO_MANIFEST_DIR"), "/../save-states");
        let newest = std::fs::read_dir(folder)
            .expect("the kept save states are in save-states/")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.is_dir())
            .max_by_key(|path| set_order(path))
            .expect("save-states/ has a set");
        ["DMG", "CGB-E", "SGB"]
            .iter()
            .map(|model| {
                let state = std::fs::read(newest.join(format!("tobudx.gb-{model}.state")))
                    .expect("the newest set has the game's states");
                let mut values = Vec::new();
                // The magic (16 bytes) and the format version (1), then the
                // header's fields and the emulator's.
                let after_header = walk(&state, 17, &mut Vec::new()).expect("the header reads");
                walk(&state, after_header, &mut values).expect("the emulator's state reads");
                Fields { state, values }
            })
            .collect()
    })
}

/// How save-state sets sort: by date, then by the number after it, as `cargo
/// save-states` does (`2026-10-14-10` after `2026-10-14-2`).
fn set_order(path: &std::path::Path) -> (String, u32) {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let (date, number) = name
        .get(..10)
        .map_or((&*name, ""), |date| (date, &name[10..]));
    (
        date.to_owned(),
        number.trim_start_matches('-').parse().unwrap_or(1),
    )
}

/// Reads the fields that start at `start`, up to the empty name that ends
/// them, and adds where each value is to `values`, going into a value that
/// is itself a list of fields. Returns where the fields end, or `None` when
/// the bytes there aren't a list of fields.
fn walk(state: &[u8], start: usize, values: &mut Vec<(usize, usize)>) -> Option<usize> {
    let mut position = start;
    loop {
        let name_length = usize::from(*state.get(position)?);
        position += 1;
        if name_length == 0 {
            return Some(position);
        }
        position += name_length;
        let length = u32::from_le_bytes(state.get(position..position + 4)?.try_into().ok()?);
        position += 4;
        let length = usize::try_from(length).ok()?;
        let end = position
            .checked_add(length)
            .filter(|&end| end <= state.len())?;
        match fields_in_a_row(&state[..end], position) {
            Some(mut inner) if length > 1 => values.append(&mut inner),
            // An empty value, such as an empty list, has nothing to change.
            _ if length == 0 => {}
            _ => values.push((position, length)),
        }
        position = end;
    }
}

/// The values in the bytes from `start` to the end of `state` when they are
/// one or more lists of fields in a row, such as a struct or an array of
/// structs (the PPU's object FIFO stores its eight entries that way), or
/// `None` when they aren't.
fn fields_in_a_row(state: &[u8], start: usize) -> Option<Vec<(usize, usize)>> {
    let mut values = Vec::new();
    let mut position = start;
    while position < state.len() {
        position = walk(state, position, &mut values)?;
    }
    Some(values)
}

/// Changes the value at `start`, `length` bytes long: to zeros, to all ones,
/// to one more or one less than it was in its first byte, or to bytes taken
/// from the input, which are used up.
fn change(state: &mut [u8], (start, length): (usize, usize), how: u8, input: &mut &[u8]) {
    // A big value, such as VRAM or the frame buffer, gets only its first
    // bytes changed: any contents are valid there.
    let value = &mut state[start..start + length.min(8)];
    match how % 5 {
        0 => value.fill(0),
        1 => value.fill(0xFF),
        2 => value[0] = value[0].wrapping_add(1),
        3 => value[0] = value[0].wrapping_sub(1),
        _ => {
            let taken = input.len().min(value.len());
            value[..taken].copy_from_slice(&input[..taken]);
            *input = &input[taken..];
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&pick, mut input)) = data.split_first() else {
        return;
    };
    let kept = &kept_states()[usize::from(pick) % kept_states().len()];
    let mut state = kept.state.clone();
    while let [high, low, how, rest @ ..] = input {
        input = rest;
        let field = usize::from(u16::from_be_bytes([*high, *low])) % kept.values.len();
        change(&mut state, kept.values[field], *how, &mut input);
    }
    let Ok(header) = SaveStateHeader::read(&state) else {
        return;
    };
    let mut emulator = start(header.model, game()).expect("the benchmark game loads");
    if emulator.load_state(&state).is_err() {
        return;
    }
    run_frames(&mut emulator, 2);
    let again = emulator.save_state();
    emulator
        .load_state(&again)
        .expect("a state the emulator saved loads again");
});
