//! Save states: snapshots of the whole emulator that can be restored later.

use crate::emulator::Emulator;
use crate::error::SaveStateError;
use crate::model::Model;
use crate::serializable::Serializable;
use std::io::Cursor;

/// The version of the save state layout. Bump it whenever the serialized data
/// changes, so older save states are rejected instead of misread.
const FORMAT_VERSION: u8 = 27;
const HEADER_MARKER: &[u8] = b"HEADER";
const STATE_MARKER: &[u8] = b"STATE";

/// The parts of a save state file.
///
/// The file layout is: `HEADER`, the format version, the length of the
/// model's name, the model's name (such as `CGB-E`), the length of the game
/// title, the game title, `STATE`, and then the serialized emulator.
struct SaveStateFile<'a> {
    version: u8,
    model: &'a str,
    title: &'a str,
    /// The emulator's state, as written by [`Serializable::serialize`].
    state: &'a [u8],
}

/// Splits a string of at most 255 bytes, preceded by its length, off the
/// front of `data`.
fn split_short_string(data: &[u8]) -> Option<(&str, &[u8])> {
    let (&length, rest) = data.split_first()?;
    let (string, rest) = rest.split_at_checked(usize::from(length))?;
    Some((std::str::from_utf8(string).ok()?, rest))
}

/// Appends `string`, preceded by its length, to `data`.
fn push_short_string(data: &mut Vec<u8>, string: &str) {
    let length = u8::try_from(string.len()).expect("model names and cartridge titles are short");
    data.push(length);
    data.extend_from_slice(string.as_bytes());
}

impl<'a> SaveStateFile<'a> {
    /// Splits a save state file into its parts without reading the state itself.
    fn parse(data: &'a [u8]) -> Result<Self, SaveStateError> {
        let rest = data
            .strip_prefix(HEADER_MARKER)
            .ok_or(SaveStateError::InvalidFormat)?;
        let (&version, rest) = rest.split_first().ok_or(SaveStateError::InvalidFormat)?;
        if version != FORMAT_VERSION {
            return Ok(SaveStateFile {
                version,
                model: "",
                title: "",
                state: &[],
            });
        }
        let (model, rest) = split_short_string(rest).ok_or(SaveStateError::InvalidFormat)?;
        let (title, rest) = split_short_string(rest).ok_or(SaveStateError::InvalidFormat)?;
        let state = rest
            .strip_prefix(STATE_MARKER)
            .ok_or(SaveStateError::InvalidFormat)?;
        Ok(SaveStateFile {
            version,
            model,
            title,
            state,
        })
    }
}

impl Emulator {
    /// Captures the complete emulator state so it can be restored later with
    /// [`Emulator::load_state`].
    pub fn save_state(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(HEADER_MARKER);
        data.push(FORMAT_VERSION);
        push_short_string(&mut data, self.model().name());
        push_short_string(&mut data, self.address_bus().cartridge().title());
        data.extend_from_slice(STATE_MARKER);
        data.extend_from_slice(&self.serialized_state());
        data
    }

    /// Restores a state captured by [`Emulator::save_state`].
    ///
    /// The save state must come from the same game, the same hardware model
    /// and a Nubo version with the same save state format, and its data must
    /// be read to the last byte. If it can't be loaded, the emulator is left
    /// exactly as it was.
    ///
    /// Loading a state replaces the cartridge RAM and clock, but doesn't pass
    /// them to the frontend to save (see
    /// [`CartridgeEffects`](crate::CartridgeEffects)): loading a state must
    /// never overwrite the player's battery save by itself. The battery save
    /// follows the loaded state the next time the game writes its RAM or
    /// clock.
    pub fn load_state(&mut self, data: &[u8]) -> Result<(), SaveStateError> {
        let file = SaveStateFile::parse(data)?;
        if file.version != FORMAT_VERSION {
            return Err(SaveStateError::IncompatibleVersion {
                found: file.version,
                expected: FORMAT_VERSION,
            });
        }

        let found_model = file
            .model
            .parse::<Model>()
            .map_err(|_| SaveStateError::InvalidFormat)?;
        if found_model != self.model() {
            return Err(SaveStateError::DifferentModel {
                found: found_model,
                expected: self.model(),
            });
        }

        let current_title = self.address_bus().cartridge().title();
        if file.title != current_title {
            return Err(SaveStateError::DifferentGame {
                found: file.title.to_string(),
                expected: current_title.to_string(),
            });
        }

        let previous_state = self.serialized_state();
        let mut reader = Cursor::new(file.state);
        let read_all = self.deserialize(&mut reader).is_ok()
            && usize::try_from(reader.position()).is_ok_and(|read| read == file.state.len());
        if !read_all {
            self.deserialize(&mut Cursor::new(previous_state))
                .expect("a state the emulator just wrote can always be read back");
            return Err(SaveStateError::InvalidFormat);
        }
        Ok(())
    }

    fn serialized_state(&self) -> Vec<u8> {
        let mut state = Vec::new();
        self.serialize(&mut state)
            .expect("writing to a Vec never fails");
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address_bus::constants::{CART_TYPE_MBC1, RAM_SIZE_2KB, ROM_SIZE_64KB};
    use crate::address_bus::effects::empty_cartridge_effects;
    use crate::address_bus::test_utils::build_rom;
    use crate::model::Model;

    fn emulator_with_game() -> Emulator {
        let mut emulator = Emulator::new(Model::Dmg);
        emulator
            .load_rom(
                &build_rom(CART_TYPE_MBC1, ROM_SIZE_64KB, RAM_SIZE_2KB),
                empty_cartridge_effects(),
            )
            .unwrap();
        emulator
    }

    #[test]
    fn restores_a_saved_state() {
        let mut emulator = emulator_with_game();
        let saved = emulator.save_state();
        for _ in 0..1000 {
            emulator.step();
        }
        emulator.load_state(&saved).unwrap();
        assert_eq!(emulator.save_state(), saved);
    }

    #[test]
    fn rejects_truncated_data_without_changing_the_emulator() {
        let mut emulator = emulator_with_game();
        let saved = emulator.save_state();
        for _ in 0..1000 {
            emulator.step();
        }
        let before = emulator.save_state();

        for length in [0, 3, HEADER_MARKER.len() + 1, saved.len() / 2] {
            assert_eq!(
                emulator.load_state(&saved[..length]),
                Err(SaveStateError::InvalidFormat)
            );
            assert_eq!(emulator.save_state(), before);
        }
    }

    #[test]
    fn rejects_a_state_from_another_version() {
        let mut emulator = emulator_with_game();
        let mut saved = emulator.save_state();
        saved[HEADER_MARKER.len()] = FORMAT_VERSION + 1;
        assert!(matches!(
            emulator.load_state(&saved),
            Err(SaveStateError::IncompatibleVersion { .. })
        ));
    }

    #[test]
    fn rejects_a_state_from_another_model() {
        let saved = emulator_with_game().save_state();
        let mut emulator = Emulator::new(Model::CgbE);
        emulator
            .load_rom(
                &build_rom(CART_TYPE_MBC1, ROM_SIZE_64KB, RAM_SIZE_2KB),
                empty_cartridge_effects(),
            )
            .unwrap();

        assert_eq!(
            emulator.load_state(&saved),
            Err(SaveStateError::DifferentModel {
                found: Model::Dmg,
                expected: Model::CgbE,
            })
        );
    }

    #[test]
    fn rejects_data_left_over_after_the_state_without_changing_the_emulator() {
        let mut emulator = emulator_with_game();
        let mut saved = emulator.save_state();
        saved.push(0);
        for _ in 0..1000 {
            emulator.step();
        }
        let before = emulator.save_state();

        assert_eq!(
            emulator.load_state(&saved),
            Err(SaveStateError::InvalidFormat)
        );
        assert_eq!(emulator.save_state(), before);
    }

    /// The format version and the length of `emulator_with_game`'s save state
    /// in that format. Nearly every change to the layout changes the length.
    const PINNED_LAYOUT: (u8, usize) = (27, 145_383);

    #[test]
    fn the_save_state_layout_still_matches_its_format_version() {
        assert_eq!(
            (FORMAT_VERSION, emulator_with_game().save_state().len()),
            PINNED_LAYOUT,
            "the save state layout changed: bump FORMAT_VERSION (once per release) \
             and update PINNED_LAYOUT to the new version and length"
        );
    }
}
