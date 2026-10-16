//! Save states: snapshots of the whole emulator that can be restored later.
//!
//! A save state is, in this order:
//!
//! 1. `NUBO SAVE STATE` and a zero byte, 16 bytes that say what the file is.
//! 2. The format version, one byte ([`FORMAT_VERSION`]).
//! 3. The header ([`SaveStateHeader`]), a struct of named fields.
//! 4. The emulator, a struct of named fields: `cpu` and the components on the
//!    address bus, named after the hardware (`bus`, `cartridge`, `timers`,
//!    `ppu`, `apu`, `oam_dma`, `vram_dma`, `serial`, `speed_switch` and
//!    `joypad`; see `AddressBus::write_save_state_components`), so moving
//!    code from one struct to another doesn't move the values a state stores.
//!
//! The bytes are the same on every platform, and the rules in
//! `serializable.rs` keep states loading in later versions of Nubo and, as
//! far as they can, in earlier ones: a value a state doesn't have starts as it
//! does right after power-on, and values this version doesn't know or can't
//! read are skipped.
//!
//! A loaded state must go on exactly like the run it was saved from, which
//! `cargo benchmark --check-save-states` checks. So every field is stored
//! except the ones that don't belong in a save state (see `Serializable`), and
//! two tests catch a field that is missed:
//! `a_loaded_state_restores_every_field_of_the_cpu_the_address_bus_the_ppu_and_the_apu`
//! (in `tests.rs`, which lists the fields left out on purpose) compares every
//! field after loading, and `a_value_a_state_lacks_starts_at_its_power_on_value`
//! checks that a value a state lacks gets its power-on value.

use crate::address_bus::effects::NoSaves;
use crate::emulator::Emulator;
use crate::error::SaveStateError;
use crate::model::Model;
use crate::serializable::{
    FieldWriter, Serializable, collect_unreadable, note_variants, read_fields, stored_layout,
    unreadable_value,
};
use std::io::{self, Read, Write};

/// What every save state starts with. It has no line break, so no tool that
/// converts line endings can damage it.
const MAGIC: &[u8; 16] = b"NUBO SAVE STATE\0";
/// The version of the layout above and of how `serializable.rs` stores
/// values. Adding, removing and renaming fields needs no new version: change
/// it only if the layout or the way values are stored has to change, which
/// earlier versions then refuse, and keep the earlier versions loading.
const FORMAT_VERSION: u8 = 28;
/// What save states started with before format 28, whose values had no names
/// and which don't load.
const UNNAMED_FORMAT_MAGIC: &[u8] = b"HEADER";

/// The commit of Nubo's source code this build was made from, such as
/// `7c44d3955370fa02045672a5774f923007919876`, followed by `-dirty` when the
/// source had changes that weren't committed. `cargo xtask` sets it through
/// `NUBO_COMMIT` for everything it builds (every release and nightly build);
/// it is empty for a plain `cargo build`. Save states store it so a state
/// tells exactly which build made it.
pub const BUILD_COMMIT: &str = match option_env!("NUBO_COMMIT") {
    Some(commit) => commit,
    None => "",
};

/// What a save state says about itself, read without loading the state, so a
/// frontend knows which model to emulate before it loads one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveStateHeader {
    /// The version of Nubo that made the state, such as `0.4.0`.
    pub nubo_version: String,
    /// The commit the build that made the state was made from
    /// ([`BUILD_COMMIT`]), empty when it isn't known.
    pub nubo_commit: String,
    /// The hardware the state was made on, the only one it loads into.
    pub model: Model,
    /// The title in the game's cartridge header. A state only loads into a
    /// game with the same title.
    pub game_title: String,
    /// The global checksum in the game's cartridge header (0x14E-0x14F),
    /// which tells revisions of a game with the same title apart.
    pub game_checksum: u16,
}

impl SaveStateHeader {
    /// Reads the header of a save state made by [`Emulator::save_state`].
    pub fn read(data: &[u8]) -> Result<SaveStateHeader, SaveStateError> {
        split(data).map(|(header, _)| header)
    }

    /// The header of a state of `emulator`.
    fn of(emulator: &Emulator) -> Self {
        let cartridge = emulator.address_bus().cartridge();
        let checksum = cartridge.rom().get(0x14E..0x150).unwrap_or_default();
        SaveStateHeader {
            nubo_version: String::from(env!("CARGO_PKG_VERSION")),
            nubo_commit: String::from(BUILD_COMMIT),
            model: emulator.model(),
            game_title: cartridge.title().to_owned(),
            game_checksum: checksum.try_into().map_or(0, u16::from_be_bytes),
        }
    }

    /// Writes the header as named fields, which `split` reads back.
    fn write(&self, writer: &mut dyn Write) -> io::Result<()> {
        let mut fields = FieldWriter::new(writer);
        fields.field("nubo_version", &self.nubo_version)?;
        fields.field("nubo_commit", &self.nubo_commit)?;
        fields.field_with("model", String::encoding, |writer| {
            note_variants(&Model::ALL.map(Model::name));
            String::from(self.model.name()).serialize(writer)
        })?;
        fields.field("game_title", &self.game_title)?;
        fields.field("game_checksum", &self.game_checksum)?;
        fields.finish()
    }
}

/// Splits a save state into its header and the emulator's state.
fn split(data: &[u8]) -> Result<(SaveStateHeader, &[u8]), SaveStateError> {
    if let Some(rest) = data.strip_prefix(UNNAMED_FORMAT_MAGIC) {
        return Err(SaveStateError::IncompatibleVersion {
            found: rest.first().copied().unwrap_or_default(),
            expected: FORMAT_VERSION,
        });
    }
    let rest = data
        .strip_prefix(MAGIC.as_slice())
        .ok_or(SaveStateError::InvalidFormat)?;
    let (&version, mut rest) = rest.split_first().ok_or(SaveStateError::InvalidFormat)?;
    if version != FORMAT_VERSION {
        return Err(SaveStateError::IncompatibleVersion {
            found: version,
            expected: FORMAT_VERSION,
        });
    }

    let mut nubo_version = String::new();
    let mut nubo_commit = String::new();
    let mut model = None;
    let mut game_title = None;
    let mut game_checksum = 0;
    read_fields(&mut rest, |name, bytes| {
        match name {
            "nubo_version" => nubo_version.deserialize(bytes)?,
            "nubo_commit" => nubo_commit.deserialize(bytes)?,
            "model" => {
                let mut name = String::new();
                name.deserialize(bytes)?;
                let parsed = name
                    .parse()
                    .map_err(|_| unreadable_value(format!("unknown model {name}")))?;
                model = Some(parsed);
            }
            "game_title" => {
                let mut title = String::new();
                title.deserialize(bytes)?;
                game_title = Some(title);
            }
            "game_checksum" => game_checksum.deserialize(bytes)?,
            _ => return Ok(false),
        }
        Ok(true)
    })
    .map_err(|_| SaveStateError::InvalidFormat)?;

    // Without them a state can't be checked against the emulator it loads into.
    let header = SaveStateHeader {
        nubo_version,
        nubo_commit,
        model: model.ok_or(SaveStateError::InvalidFormat)?,
        game_title: game_title.ok_or(SaveStateError::InvalidFormat)?,
        game_checksum,
    };
    Ok((header, rest))
}

/// The top level of a save state's emulator: the CPU as `cpu`, and each
/// component on the address bus as a field of its own next to it, so that the
/// stored layout follows the hardware rather than which struct owns which.
impl Serializable for Emulator {
    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        let mut fields = FieldWriter::new(writer);
        fields.field("cpu", &self.cpu)?;
        self.address_bus()
            .write_save_state_components(&mut fields)?;
        fields.finish()
    }

    fn deserialize(&mut self, reader: &mut dyn Read) -> io::Result<()> {
        read_fields(reader, |name, bytes| {
            if name == "cpu" {
                self.cpu.deserialize(bytes)?;
                Ok(true)
            } else {
                self.address_bus_mut()
                    .read_save_state_component(name, bytes)
            }
        })?;
        self.address_bus_mut().finish_loading_save_state();
        Ok(())
    }
}

impl Emulator {
    /// Captures the complete emulator state so it can be restored later with
    /// [`Emulator::load_state`], by this version of Nubo or another one.
    pub fn save_state(&self) -> Vec<u8> {
        let mut data = MAGIC.to_vec();
        data.push(FORMAT_VERSION);
        SaveStateHeader::of(self)
            .write(&mut data)
            .expect("writing to a Vec never fails");
        data.extend_from_slice(&self.serialized_state());
        data
    }

    /// Restores a state captured by [`Emulator::save_state`].
    ///
    /// The save state must come from the same game (the same title and
    /// header checksum, so not another revision or a patched copy) and the
    /// same hardware model, and its data must be read to the last byte. It may come from
    /// another version of Nubo: values it doesn't have start as they do right
    /// after power-on, and values this version doesn't know, or can't read,
    /// are skipped (see [`Emulator::load_state_with_report`]). If it can't be
    /// loaded, the emulator is left exactly as it was.
    ///
    /// Loading a state replaces the cartridge RAM and clock, but doesn't pass
    /// them to the frontend to save (see
    /// [`CartridgeEffects`](crate::CartridgeEffects)): loading a state must
    /// never overwrite the player's battery save by itself. The battery save
    /// follows the loaded state the next time the game writes its RAM or
    /// clock.
    ///
    /// A cartridge clock saved while following the other [`RtcClock`] than
    /// this emulator's cartridge does is moved over to this one, keeping
    /// what it shows (see [`Emulator::rtc_state`]). The clock of a state
    /// from before Nubo noted which one it follows loads as it is.
    ///
    /// [`RtcClock`]: crate::RtcClock
    pub fn load_state(&mut self, data: &[u8]) -> Result<(), SaveStateError> {
        self.load_state_with_report(data).map(|_| ())
    }

    /// Loads a state like [`Emulator::load_state`] and returns the paths of
    /// the values it couldn't read, such as `ppu.line_state` for a state a
    /// newer Nubo made in a PPU mode this one doesn't know. Such a value keeps
    /// its power-on value, or, when its bytes are longer than it needs, what
    /// it read from their start. A state from this version has none, and neither has one
    /// from an earlier version as long as later changes followed the rules in
    /// `serializable.rs`, which `cargo save-states` checks.
    pub fn load_state_with_report(&mut self, data: &[u8]) -> Result<Vec<String>, SaveStateError> {
        let (header, mut state) = split(data)?;
        if header.model != self.model() {
            return Err(SaveStateError::DifferentModel {
                found: header.model,
                expected: self.model(),
            });
        }
        let current = SaveStateHeader::of(self);
        if header.game_title != current.game_title {
            return Err(SaveStateError::DifferentGame {
                found: header.game_title,
                expected: current.game_title,
            });
        }
        if header.game_checksum != current.game_checksum {
            return Err(SaveStateError::DifferentGameVersion {
                title: current.game_title,
            });
        }

        let previous_state = self.serialized_state();
        let power_on_state = self.power_on_state();
        let loaded = self
            .deserialize(&mut &power_on_state[..])
            .and_then(|()| collect_unreadable(|| self.deserialize(&mut state)));
        match loaded {
            Ok(unreadable) if state.is_empty() => Ok(unreadable),
            _ => {
                self.deserialize(&mut previous_state.as_slice())
                    .expect("a state the emulator just wrote can always be read back");
                Err(SaveStateError::InvalidFormat)
            }
        }
    }

    /// What a save state of this emulator stores, sorted: the path of every
    /// value, such as `header.model` or `ppu.lcdc`, and every variant each
    /// enum value can be stored as, such as `cpu.run_state = Halted`. `cargo
    /// save-states` compares it with the newest kept set (its `layout.txt`),
    /// so a version that stores other values or names than the one before
    /// can't go unnoticed.
    pub fn save_state_layout(&self) -> Vec<String> {
        let header = stored_layout(|| SaveStateHeader::of(self).write(&mut io::sink()))
            .expect("writing to a sink never fails");
        let state = stored_layout(|| self.serialize(&mut io::sink()))
            .expect("writing to a sink never fails");
        let mut layout: Vec<String> = header
            .into_iter()
            .map(|entry| format!("header.{entry}"))
            .chain(state)
            .collect();
        layout.sort();
        layout
    }

    fn serialized_state(&self) -> Vec<u8> {
        let mut state = Vec::new();
        self.serialize(&mut state)
            .expect("writing to a Vec never fails");
        state
    }

    /// The state of an emulator of the same model with the same game, just
    /// after power-on: loading it first gives every value a save state from
    /// another version doesn't have its power-on value. It is made on the
    /// first load and kept until another game is loaded, since run-ahead and
    /// rewind load a state every frame.
    fn power_on_state(&mut self) -> std::sync::Arc<[u8]> {
        if let Some(state) = &self.power_on_state.0 {
            return state.clone();
        }
        let mut emulator = Emulator::new(self.model());
        let rom = self.address_bus().cartridge().rom();
        if !rom.is_empty() {
            emulator
                .address_bus_mut()
                .load_rom(rom.to_vec(), Box::new(NoSaves::default()))
                .expect("the ROM this emulator runs loads again");
        }
        let state: std::sync::Arc<[u8]> = emulator.serialized_state().into();
        self.power_on_state.0 = Some(state.clone());
        state
    }
}

/// The state [`Emulator::load_state`] starts from, once made. It depends
/// only on the model and the game, never on how the emulator ran, so it is
/// no part of the emulator's state: its `Debug` output is the same whether
/// it was made or not, and emulators that compare equal stay equal.
#[derive(Default)]
pub(super) struct PowerOnState(Option<std::sync::Arc<[u8]>>);

impl std::fmt::Debug for PowerOnState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PowerOnState")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address_bus::constants::{CARTRIDGE_TYPE_MBC1, RAM_SIZE_2KB, ROM_SIZE_64KB};
    use crate::address_bus::effects::empty_cartridge_effects;
    use crate::address_bus::test_utils::build_rom;
    use crate::model::Model;

    fn emulator_with_game() -> Emulator {
        let mut emulator = Emulator::new(Model::Dmg);
        emulator
            .load_rom(
                &build_rom(CARTRIDGE_TYPE_MBC1, ROM_SIZE_64KB, RAM_SIZE_2KB),
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

        for length in [0, 3, MAGIC.len() + 1, saved.len() / 2] {
            assert_eq!(
                emulator.load_state(&saved[..length]),
                Err(SaveStateError::InvalidFormat)
            );
            assert_eq!(emulator.save_state(), before);
        }
    }

    #[test]
    fn rejects_a_state_from_before_values_had_names() {
        let mut emulator = emulator_with_game();
        let mut old = UNNAMED_FORMAT_MAGIC.to_vec();
        old.push(27);
        old.extend_from_slice(b"\x03DMG");
        assert_eq!(
            emulator.load_state(&old),
            Err(SaveStateError::IncompatibleVersion {
                found: 27,
                expected: FORMAT_VERSION
            })
        );
    }

    #[test]
    fn refuses_a_state_of_a_newer_format_version() {
        let mut emulator = emulator_with_game();
        let mut saved = emulator.save_state();
        saved[MAGIC.len()] = FORMAT_VERSION + 1;
        assert_eq!(
            emulator.load_state(&saved),
            Err(SaveStateError::IncompatibleVersion {
                found: FORMAT_VERSION + 1,
                expected: FORMAT_VERSION
            })
        );
    }

    #[test]
    fn the_header_tells_what_a_state_is_of_without_loading_it() {
        let emulator = emulator_with_game();
        let header = SaveStateHeader::read(&emulator.save_state()).unwrap();
        assert_eq!(header.nubo_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(header.nubo_commit, BUILD_COMMIT);
        assert_eq!(header.model, Model::Dmg);
        assert_eq!(
            header.game_title,
            emulator.address_bus().cartridge().title()
        );
        assert_eq!(
            SaveStateHeader::read(b"not a save state at all"),
            Err(SaveStateError::InvalidFormat)
        );
    }

    #[test]
    fn components_are_stored_under_the_names_of_the_hardware() {
        let paths = emulator_with_game().save_state_layout();
        let components: std::collections::BTreeSet<&str> = paths
            .iter()
            .map(|entry| entry.split(['.', ':', ' ']).next().unwrap())
            .collect();
        assert_eq!(
            components.into_iter().collect::<Vec<_>>(),
            [
                "apu",
                "bus",
                "cartridge",
                "cpu",
                "header",
                "joypad",
                "oam_dma",
                "ppu",
                "serial",
                "speed_switch",
                "timers",
                "vram_dma"
            ]
        );
        for path in [
            "header.model: text",
            "header.model = CGB-E",
            "cpu: fields",
            "cpu.run_state: variant",
            "cpu.run_state = Halted",
            "cpu.registers.program_counter: u16",
            "bus.work_ram: 32768 bytes",
            "bus.high_ram: 128 bytes",
            "ppu.lcdc: u8",
            "apu.state.square1.period: u16",
            "apu.previous_left_outputs: 4 × f32",
        ] {
            assert!(paths.iter().any(|stored| stored == path), "{path}");
        }
    }

    /// Where the value of the first field called `name` is in a save state.
    fn field_bytes(state: &[u8], name: &str) -> std::ops::Range<usize> {
        let mut record = vec![u8::try_from(name.len()).unwrap()];
        record.extend_from_slice(name.as_bytes());
        let start = state
            .windows(record.len())
            .position(|window| window == record)
            .expect("the state has the field")
            + record.len();
        let length = u32::from_le_bytes(state[start..start + 4].try_into().unwrap());
        start + 4..start + 4 + usize::try_from(length).unwrap()
    }

    #[test]
    fn a_value_this_version_cant_read_keeps_its_power_on_value_and_is_reported() {
        let mut emulator = emulator_with_game();
        for _ in 0..1000 {
            emulator.step();
        }
        let program_counter = emulator.registers().program_counter;
        // As if a newer version had saved the CPU in a run state this one
        // doesn't have: the variant's name, after its length, becomes another.
        let mut saved = emulator.save_state();
        let run_state = field_bytes(&saved, "run_state");
        saved[run_state.start + 1..run_state.end].fill(b'X');
        for _ in 0..1000 {
            emulator.step();
        }

        assert_eq!(
            emulator.load_state_with_report(&saved),
            Ok(vec![String::from("cpu.run_state")])
        );
        assert_eq!(emulator.registers().program_counter, program_counter);
        assert_eq!(
            emulator.load_state_with_report(&emulator.save_state()),
            Ok(Vec::new())
        );
    }

    /// Renames the first field called `name` in a save state, keeping its
    /// length so the lengths of the fields around it still hold.
    fn rename_field(state: &mut [u8], name: &str, new_name: &str) {
        assert_eq!(name.len(), new_name.len());
        let mut record = vec![u8::try_from(name.len()).unwrap()];
        record.extend_from_slice(name.as_bytes());
        let start = state
            .windows(record.len())
            .position(|window| window == record)
            .expect("the state has the field")
            + 1;
        state[start..start + name.len()].copy_from_slice(new_name.as_bytes());
    }

    /// A value a state doesn't have, as if it came from a version without it,
    /// starts at its power-on value, while the rest of the state still loads.
    #[test]
    fn a_value_a_state_lacks_starts_at_its_power_on_value() {
        let power_on = emulator_with_game().registers().stack_pointer;
        let mut emulator = emulator_with_game();
        for _ in 0..1000 {
            emulator.step();
        }
        let saved_stack_pointer = emulator.registers().stack_pointer;
        assert_ne!(saved_stack_pointer, power_on);

        // As if the state came from a version that called it something else.
        let mut saved = emulator.save_state();
        rename_field(&mut saved, "stack_pointer", "stack_pointez");
        let program_counter = emulator.registers().program_counter;
        for _ in 0..1000 {
            emulator.step();
        }
        emulator.load_state(&saved).unwrap();

        assert_eq!(emulator.registers().stack_pointer, power_on);
        assert_eq!(emulator.registers().program_counter, program_counter);
    }

    #[test]
    fn rejects_a_state_of_another_version_of_the_game() {
        let saved = emulator_with_game().save_state();
        let mut revision = build_rom(CARTRIDGE_TYPE_MBC1, ROM_SIZE_64KB, RAM_SIZE_2KB);
        revision[0x014E] = 0x12;
        let mut emulator = Emulator::new(Model::Dmg);
        emulator
            .load_rom(&revision, empty_cartridge_effects())
            .unwrap();
        let before = emulator.serialized_state();

        assert_eq!(
            emulator.load_state(&saved),
            Err(SaveStateError::DifferentGameVersion {
                title: String::new()
            })
        );
        assert_eq!(emulator.serialized_state(), before);
    }

    #[test]
    fn rejects_a_state_from_another_model() {
        let saved = emulator_with_game().save_state();
        let mut emulator = Emulator::new(Model::CgbE);
        emulator
            .load_rom(
                &build_rom(CARTRIDGE_TYPE_MBC1, ROM_SIZE_64KB, RAM_SIZE_2KB),
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
}
