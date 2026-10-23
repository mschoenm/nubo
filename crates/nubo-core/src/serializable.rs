//! The binary format of save states.
//!
//! Save states must keep loading after Nubo changes, because players can lose
//! hours of play otherwise, and as far as possible older versions of Nubo
//! should load the states of newer ones too. So every struct is stored as its
//! fields, each with its name and its length in bytes ([`FieldWriter`], read
//! by [`read_fields`]), and enum variants by name. Reading a state matches
//! fields by name, which makes most changes safe in both directions without
//! anything else to do:
//!
//! - **Adding a field**: a state that doesn't have it, from an older version,
//!   leaves it at its power-on value (`Emulator::load_state` starts from a
//!   just-started emulator), and an older version skips it.
//! - **Removing or reordering fields**: fields a struct doesn't know are
//!   skipped.
//! - **Renaming a field or a variant in the code**: keep its stored name with
//!   `#[serializable(name = "stored_name")]`. Stored names are permanent;
//!   change one only when it would be misleading for what the value means now.
//! - **Changing a field's type or meaning** (another size, other units, other
//!   values): store it under a new name, so versions on either side leave it
//!   at its power-on value instead of misreading it.
//!
//! `cargo test --workspace` and `cargo save-states` fail when the fields,
//! their types or the enum variants a state can store change
//! ([`stored_layout`]), listing them: then keep the stored name of a field or
//! variant only renamed in the code, store a field whose type changed under a
//! new name, or keep a new set of states with `cargo save-states --record`,
//! which later versions are checked against. (The check is in the
//! nubo-save-state-check crate, which a plain `cargo test` doesn't build.)
//!
//! A value that can't be read from its bytes, such as a variant a newer
//! version added or a field whose size changed under the same name, keeps its
//! power-on value, or, when its bytes are longer than it needs, what it read
//! from their start; either way the rest of the state still loads
//! ([`collect_unreadable`] lists them, and the save states kept in
//! `save-states/` must have none).
//! Only damaged data, whose names and lengths don't add up, and values a
//! component's own checks reject make a state fail to load.
//!
//! Values inside a field are plain bytes, the same on every platform: numbers
//! little-endian (lowest byte first) in their own fixed size (never `usize`),
//! floating-point numbers as their IEEE 754 bits, `bool`s as one byte, strings
//! as UTF-8 after their length, arrays as their items, and `Vec`s and
//! `Option`s with their length or presence first.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fmt;
use std::io::{Error, ErrorKind, Read, Result, Write};

/// Writes the fields of a struct, each with its name and length, so that
/// other versions of Nubo can find the ones they know and skip the others.
/// [`read_fields`] reads them back. Derived implementations use it for every
/// struct, and manual ones should too.
pub struct FieldWriter<'a> {
    writer: &'a mut dyn Write,
    /// The bytes of the field being written, which are only written once
    /// their length is known.
    value: Vec<u8>,
}

impl<'a> FieldWriter<'a> {
    /// Starts writing a struct's fields to `writer`; [`FieldWriter::finish`]
    /// ends it.
    pub fn new(writer: &'a mut dyn Write) -> Self {
        FieldWriter {
            writer,
            value: Vec::new(),
        }
    }

    /// Writes `value` as the field `name`.
    pub fn field<T: Serializable>(&mut self, name: &str, value: &T) -> Result<()> {
        self.field_with(name, T::encoding, |writer| value.serialize(writer))
    }

    /// Writes the field `name` with the bytes `write` writes, for a value that
    /// isn't one `Serializable` value, such as the items of an array.
    /// `encoding` says how they are stored, as [`Serializable::encoding`]
    /// does for a value (see [`array_encoding`]).
    pub fn field_with(
        &mut self,
        name: &str,
        encoding: impl FnOnce() -> String,
        write: impl FnOnce(&mut dyn Write) -> Result<()>,
    ) -> Result<()> {
        debug_assert!(!name.is_empty(), "an empty name ends the fields");
        self.value.clear();
        within_field(name, || {
            note_field(encoding);
            write(&mut self.value)
        })?;
        let length = u32::try_from(self.value.len())
            .map_err(|_| Error::new(ErrorKind::InvalidInput, "a field is too long to save"))?;
        write_name(self.writer, name)?;
        length.serialize(self.writer)?;
        self.writer.write_all(&self.value)
    }

    /// Ends the struct's fields.
    pub fn finish(self) -> Result<()> {
        0u8.serialize(self.writer)
    }
}

/// Reads the fields a [`FieldWriter`] wrote, calling `read_field` with each
/// field's name and bytes. `read_field` returns whether it knows the field.
///
/// The bytes of fields it doesn't know (fields of another version) are
/// skipped. A known field whose value can't be read from its bytes keeps the
/// value it had, and one that leaves bytes over keeps what it read from the
/// start of them; both are noted for [`collect_unreadable`]. Damaged data and
/// values a component's checks reject fail the whole read.
pub fn read_fields(
    reader: &mut dyn Read,
    mut read_field: impl FnMut(&str, &mut &[u8]) -> Result<bool>,
) -> Result<()> {
    let mut value = Vec::new();
    loop {
        let name = read_name(reader).map_err(damaged)?;
        if name.is_empty() {
            return Ok(());
        }
        let mut length = 0u32;
        length.deserialize(reader).map_err(damaged)?;

        // Read through `take` rather than into a buffer of the stored length,
        // so a damaged length can't make it allocate gigabytes.
        value.clear();
        (&mut *reader)
            .take(u64::from(length))
            .read_to_end(&mut value)
            .map_err(damaged)?;
        if u32::try_from(value.len()) != Ok(length) {
            return Err(damaged(Error::new(
                ErrorKind::UnexpectedEof,
                format!("the saved {name} ends early"),
            )));
        }

        let mut bytes = value.as_slice();
        within_field(&name, || match read_field(&name, &mut bytes) {
            Ok(true) if !bytes.is_empty() => {
                note_unreadable();
                Ok(())
            }
            Ok(_) => Ok(()),
            Err(error) if is_unreadable_value(&error) => {
                note_unreadable();
                Ok(())
            }
            Err(error) => Err(error),
        })?;
    }
}

/// Writes a name of 1 to 255 bytes, preceded by its length. Fields and enum
/// variants are stored by name.
pub fn write_name(writer: &mut dyn Write, name: &str) -> Result<()> {
    let length = u8::try_from(name.len())
        .map_err(|_| Error::new(ErrorKind::InvalidInput, "a name is too long to save"))?;
    length.serialize(writer)?;
    writer.write_all(name.as_bytes())
}

/// How `count` values of type `T` stored one after another are stored, such
/// as `4 × f32`, for [`FieldWriter::field_with`].
pub fn array_encoding<T: Serializable>(count: usize) -> String {
    format!("{count} × {}", T::encoding())
}

/// Writes `variant`, one of an enum's `variants` (the stored names of all of
/// them), by its name. [`stored_layout`] lists all the variants, so a variant
/// renamed in the code without keeping its stored name shows up even when no
/// state uses it.
pub fn write_variant(writer: &mut dyn Write, variant: &str, variants: &[&str]) -> Result<()> {
    note_variants(variants);
    write_name(writer, variant)
}

/// Notes that the value being written is one of `variants`, for
/// [`stored_layout`]. [`write_variant`] does this; values that store a choice
/// from a fixed list in another way call it themselves.
pub fn note_variants(variants: &[&str]) {
    TRACKING.with_borrow_mut(|tracking| {
        let path = tracking.path.join(".");
        if let Some(layout) = &mut tracking.layout {
            layout.extend(variants.iter().map(|variant| format!("{path} = {variant}")));
        }
    });
}

/// Reads a name written by [`write_name`], or the empty name that ends a
/// struct's fields.
pub fn read_name(reader: &mut dyn Read) -> Result<String> {
    let mut length = 0u8;
    length.deserialize(reader)?;
    let mut bytes = vec![0; usize::from(length)];
    reader.read_exact(&mut bytes)?;
    String::from_utf8(bytes).map_err(|_| unreadable_value("a name isn't text"))
}

/// The error for a value this version can't read from its bytes, such as an
/// enum variant it doesn't have. [`read_fields`] leaves such a value as it was
/// instead of failing.
pub fn unreadable_value(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::InvalidData, UnreadableValue(message.into()))
}

/// What [`unreadable_value`] wraps.
#[derive(Debug)]
struct UnreadableValue(String);

impl fmt::Display for UnreadableValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for UnreadableValue {}

/// Saved data whose names and lengths don't add up, which no version wrote.
#[derive(Debug)]
struct DamagedData(Error);

impl fmt::Display for DamagedData {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "the save state is damaged: {}", self.0)
    }
}

impl std::error::Error for DamagedData {}

/// Marks `error`, found while reading the names and lengths of fields, as
/// damaged data, so the fields around it don't take it for an unreadable
/// value.
fn damaged(error: Error) -> Error {
    if is_damaged(&error) {
        error
    } else {
        Error::new(ErrorKind::InvalidData, DamagedData(error))
    }
}

fn is_damaged(error: &Error) -> bool {
    matches!(error.get_ref(), Some(inner) if inner.is::<DamagedData>())
}

/// Whether `error` means a value couldn't be read from its bytes, rather than
/// damaged data or a value a component's checks reject: one of
/// [`unreadable_value`], or bytes that end before the value does.
fn is_unreadable_value(error: &Error) -> bool {
    if is_damaged(error) {
        return false;
    }
    match error.get_ref() {
        Some(inner) if inner.is::<UnreadableValue>() => true,
        _ => error.kind() == ErrorKind::UnexpectedEof,
    }
}

/// While [`collect_unreadable`] or [`stored_layout`] runs: the names of the
/// fields being read or written, outermost first, and what they found.
#[derive(Default)]
struct Tracking {
    path: Vec<String>,
    unreadable: Option<Vec<String>>,
    layout: Option<BTreeSet<String>>,
}

thread_local! {
    static TRACKING: RefCell<Tracking> = RefCell::default();
}

/// Runs `run` inside the field `name`, for the paths [`collect_unreadable`]
/// and [`stored_layout`] report.
fn within_field<T>(name: &str, run: impl FnOnce() -> T) -> T {
    // `TRACKING` isn't borrowed while `run` runs, since it enters fields of
    // its own.
    let tracking =
        TRACKING.with_borrow(|tracking| tracking.unreadable.is_some() || tracking.layout.is_some());
    if !tracking {
        return run();
    }
    TRACKING.with_borrow_mut(|tracking| tracking.path.push(name.to_owned()));
    let result = run();
    TRACKING.with_borrow_mut(|tracking| tracking.path.pop());
    result
}

/// Notes the field being written and how its value is stored, such as
/// `ppu.lcdc: u8`, for [`stored_layout`].
fn note_field(encoding: impl FnOnce() -> String) {
    TRACKING.with_borrow_mut(|tracking| {
        let path = tracking.path.join(".");
        if let Some(layout) = &mut tracking.layout {
            layout.insert(format!("{path}: {}", encoding()));
        }
    });
}

/// While [`stored_layout`] runs, lists what a `T` stores for a value that
/// holds none right now (an empty `Vec`, a `None`), so the layout has its
/// fields whatever the value is.
fn note_layout_of<T: Serializable + Default>() -> Result<()> {
    if TRACKING.with_borrow(|tracking| tracking.layout.is_some()) {
        T::default().serialize(&mut std::io::sink())?;
    }
    Ok(())
}

/// Notes the field being read as unreadable, for [`collect_unreadable`].
fn note_unreadable() {
    TRACKING.with_borrow_mut(|tracking| {
        let path = tracking.path.join(".");
        if let Some(unreadable) = &mut tracking.unreadable {
            unreadable.push(path);
        }
    });
}

/// Runs `read` and returns the paths of the values it couldn't read from
/// their bytes and left as they were, such as `ppu.line_state`. A state from
/// this version, or one whose changes all followed the module docs, has none.
pub fn collect_unreadable(read: impl FnOnce() -> Result<()>) -> Result<Vec<String>> {
    TRACKING.with_borrow_mut(|tracking| {
        *tracking = Tracking {
            unreadable: Some(Vec::new()),
            ..Tracking::default()
        }
    });
    let result = read();
    let unreadable = TRACKING.with_borrow_mut(|tracking| std::mem::take(tracking).unreadable);
    result.map(|()| unreadable.unwrap_or_default())
}

/// Runs `write` and returns what it stores: the path of every field it
/// wrote with how its value is stored ([`Serializable::encoding`]), such as
/// `ppu.lcdc: u8`, and for each enum value every variant it can be stored
/// as, such as `cpu.run_state = Halted`. A field of the items of a `Vec`
/// appears once, and the fields of an empty `Vec`'s items or of a `None` are
/// listed too.
pub fn stored_layout(write: impl FnOnce() -> Result<()>) -> Result<BTreeSet<String>> {
    TRACKING.with_borrow_mut(|tracking| {
        *tracking = Tracking {
            layout: Some(BTreeSet::new()),
            ..Tracking::default()
        }
    });
    let result = write();
    let layout = TRACKING.with_borrow_mut(|tracking| std::mem::take(tracking).layout);
    result.map(|()| layout.unwrap_or_default())
}

/// A value that can be written to and read back from a save state.
///
/// Most types get this with `#[derive(Serializable)]` from the
/// `nubo-serializable-derive` crate, which stores each field with its name (see the
/// module docs for what that allows).
/// `deserialize` overwrites `self` in place, so a component keeps parts of its
/// state that are not saved, such as the loaded ROM.
///
/// Fields that don't belong in a save state, such as caches and values that
/// are always zero between `Emulator` calls, are marked
/// `#[serializable(skip)]`, or left out of a hand-written implementation with
/// a comment saying why.
pub trait Serializable {
    /// Appends this value's bytes to `writer`.
    fn serialize(&self, writer: &mut dyn Write) -> Result<()>;

    /// Replaces this value with one read from `reader`. Fails when the data
    /// ends early or holds a value this type can't have.
    fn deserialize(&mut self, reader: &mut dyn Read) -> Result<()>;

    /// How the value is stored, as [`stored_layout`] lists it next to its
    /// field: `u8`, `bool`, `text`, `16 bytes`, `list of u16`, `optional
    /// fields`, `variant` for an enum (whose variants are listed apart) and
    /// `fields` for a struct (whose fields are). A field whose type changes
    /// is stored another way, so the layout shows the change.
    fn encoding() -> String
    where
        Self: Sized,
    {
        String::from("fields")
    }

    /// Appends `items` one after the other, as a `Vec` stores them. Types
    /// stored as their plain bytes write them all at once, which is the
    /// same bytes, only faster.
    fn serialize_items(items: &[Self], writer: &mut dyn Write) -> Result<()>
    where
        Self: Sized,
    {
        for item in items {
            item.serialize(writer)?;
        }
        Ok(())
    }

    /// Reads `count` items written by [`Serializable::serialize_items`].
    fn deserialize_items(count: u32, reader: &mut dyn Read) -> Result<Vec<Self>>
    where
        Self: Sized + Default,
    {
        let mut items = Vec::new();
        for _ in 0..count {
            let mut item = Self::default();
            item.deserialize(reader)?;
            items.push(item);
        }
        Ok(items)
    }
}

/// Reads exactly `length` bytes, or fails like `read_exact` when they run
/// out. It reads through `take`, so a damaged length can't make it allocate
/// gigabytes.
fn read_bytes(reader: &mut dyn Read, length: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(length).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != length {
        return Err(Error::new(ErrorKind::UnexpectedEof, "the data ends early"));
    }
    Ok(bytes)
}

/// Implements `Serializable` for number types by storing their little-endian bytes.
macro_rules! impl_serializable_for_numbers {
    ($($number:ty),*) => {
        $(
            impl Serializable for $number {
                fn serialize(&self, writer: &mut dyn Write) -> Result<()> {
                    writer.write_all(&self.to_le_bytes())
                }

                fn deserialize(&mut self, reader: &mut dyn Read) -> Result<()> {
                    let mut bytes = [0; size_of::<$number>()];
                    reader.read_exact(&mut bytes)?;
                    *self = <$number>::from_le_bytes(bytes);
                    Ok(())
                }

                fn encoding() -> String {
                    String::from(stringify!($number))
                }
            }
        )*
    };
}

impl_serializable_for_numbers!(u16, u32, u64, i16, f32, f64);

impl Serializable for u8 {
    fn serialize(&self, writer: &mut dyn Write) -> Result<()> {
        writer.write_all(&[*self])
    }

    fn deserialize(&mut self, reader: &mut dyn Read) -> Result<()> {
        let mut bytes = [0];
        reader.read_exact(&mut bytes)?;
        *self = bytes[0];
        Ok(())
    }

    fn encoding() -> String {
        String::from("u8")
    }

    fn serialize_items(items: &[Self], writer: &mut dyn Write) -> Result<()> {
        writer.write_all(items)
    }

    fn deserialize_items(count: u32, reader: &mut dyn Read) -> Result<Vec<Self>> {
        read_bytes(reader, u64::from(count))
    }
}

/// A `bool` is one byte: 1 for true and 0 for false. Any non-zero byte reads as true.
impl Serializable for bool {
    fn serialize(&self, writer: &mut dyn Write) -> Result<()> {
        u8::from(*self).serialize(writer)
    }

    fn deserialize(&mut self, reader: &mut dyn Read) -> Result<()> {
        let mut value = 0u8;
        value.deserialize(reader)?;
        *self = value != 0;
        Ok(())
    }

    fn encoding() -> String {
        String::from("bool")
    }
}

/// A byte array is stored as its bytes, without a length.
impl<const N: usize> Serializable for [u8; N] {
    fn serialize(&self, writer: &mut dyn Write) -> Result<()> {
        writer.write_all(self)
    }

    /// Changes nothing if the bytes run out (on the heap, since some arrays
    /// are larger than the stack allows in debug builds).
    fn deserialize(&mut self, reader: &mut dyn Read) -> Result<()> {
        let bytes = read_bytes(reader, N as u64)?;
        self.copy_from_slice(&bytes);
        Ok(())
    }

    fn encoding() -> String {
        format!("{N} bytes")
    }

    fn serialize_items(items: &[Self], writer: &mut dyn Write) -> Result<()> {
        writer.write_all(items.as_flattened())
    }

    fn deserialize_items(count: u32, reader: &mut dyn Read) -> Result<Vec<Self>> {
        let bytes = read_bytes(reader, u64::from(count) * N as u64)?;
        Ok(bytes.as_chunks::<N>().0.to_vec())
    }
}

/// A string is stored as its length in bytes (a `u32`) followed by its UTF-8
/// bytes.
impl Serializable for String {
    fn serialize(&self, writer: &mut dyn Write) -> Result<()> {
        let length = u32::try_from(self.len())
            .map_err(|_| Error::new(ErrorKind::InvalidInput, "a string is too long to save"))?;
        length.serialize(writer)?;
        writer.write_all(self.as_bytes())
    }

    fn deserialize(&mut self, reader: &mut dyn Read) -> Result<()> {
        let mut length = 0u32;
        length.deserialize(reader)?;
        let bytes = read_bytes(reader, u64::from(length))?;
        *self = String::from_utf8(bytes).map_err(|_| unreadable_value("a string isn't UTF-8"))?;
        Ok(())
    }

    fn encoding() -> String {
        String::from("text")
    }
}

/// A `Vec` is stored as its length (a `u32`) followed by its items.
impl<T: Serializable + Default> Serializable for Vec<T> {
    fn serialize(&self, writer: &mut dyn Write) -> Result<()> {
        let length = u32::try_from(self.len())
            .map_err(|_| Error::new(ErrorKind::InvalidInput, "Vec is too long to save"))?;
        length.serialize(writer)?;
        if self.is_empty() {
            note_layout_of::<T>()?;
        }
        T::serialize_items(self, writer)
    }

    /// Changes nothing if an item can't be read.
    fn deserialize(&mut self, reader: &mut dyn Read) -> Result<()> {
        let mut length = 0u32;
        length.deserialize(reader)?;
        *self = T::deserialize_items(length, reader)?;
        Ok(())
    }

    fn encoding() -> String {
        format!("list of {}", T::encoding())
    }
}

/// A `Box` is stored as the value it holds.
impl<T: Serializable> Serializable for Box<T> {
    fn serialize(&self, writer: &mut dyn Write) -> Result<()> {
        (**self).serialize(writer)
    }

    fn deserialize(&mut self, reader: &mut dyn Read) -> Result<()> {
        (**self).deserialize(reader)
    }

    fn encoding() -> String {
        T::encoding()
    }
}

/// An `Option` is stored as a 0 byte for `None`, or a 1 byte followed by the value.
///
/// A value that is already there is read into in place, as a plain field
/// is, so what its type leaves out of save states (caches, and the Super
/// Game Boy menu's choices, which are the player's) stays as it is and isn't
/// made again on every load. Stored values are still all replaced: a load
/// first reads the power-on state, then the saved one.
impl<T: Serializable + Default> Serializable for Option<T> {
    fn serialize(&self, writer: &mut dyn Write) -> Result<()> {
        match self {
            Some(value) => {
                1u8.serialize(writer)?;
                value.serialize(writer)
            }
            None => {
                note_layout_of::<T>()?;
                0u8.serialize(writer)
            }
        }
    }

    fn deserialize(&mut self, reader: &mut dyn Read) -> Result<()> {
        let mut tag = 0u8;
        tag.deserialize(reader)?;

        match tag {
            0 => *self = None,
            1 => match self {
                Some(value) => value.deserialize(reader)?,
                None => {
                    let mut value = T::default();
                    value.deserialize(reader)?;
                    *self = Some(value);
                }
            },
            _ => return Err(unreadable_value("an Option's tag isn't 0 or 1")),
        }
        Ok(())
    }

    fn encoding() -> String {
        format!("optional {}", T::encoding())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nubo_serializable_derive::Serializable;

    #[derive(Debug, Default, PartialEq, Serializable)]
    enum Mode {
        #[default]
        Idle,
        Busy,
    }

    #[derive(Debug, Default, PartialEq, Serializable)]
    struct Original {
        counter: u16,
        flag: bool,
        bytes: [u8; 4],
        mode: Mode,
        level: f32,
    }

    fn original() -> Original {
        Original {
            counter: 1337,
            flag: true,
            bytes: [0xDE, 0xAD, 0xBE, 0xEF],
            mode: Mode::Busy,
            level: 1.5,
        }
    }

    fn saved<T: Serializable>(value: &T) -> Vec<u8> {
        let mut bytes = Vec::new();
        value.serialize(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn fields_are_stored_with_their_names_and_lengths() {
        #[derive(Serializable)]
        struct Small {
            a: u8,
            b: u16,
        }
        assert_eq!(
            saved(&Small { a: 7, b: 0x0102 }),
            [
                1, b'a', 1, 0, 0, 0, 7, // a
                1, b'b', 2, 0, 0, 0, 0x02, 0x01, // b
                0,    // the end of the fields
            ]
        );
    }

    #[test]
    fn a_struct_reads_back_what_it_wrote() {
        let mut read = Original {
            counter: 0,
            flag: false,
            bytes: [0; 4],
            mode: Mode::Idle,
            level: 0.0,
        };
        read.deserialize(&mut saved(&original()).as_slice())
            .unwrap();
        assert_eq!(read, original());
    }

    #[test]
    fn a_field_an_older_state_lacks_keeps_its_value() {
        #[derive(Debug, PartialEq, Serializable)]
        struct WithNewField {
            counter: u16,
            flag: bool,
            bytes: [u8; 4],
            mode: Mode,
            level: f32,
            added_later: u32,
        }
        let mut read = WithNewField {
            counter: 0,
            flag: false,
            bytes: [0; 4],
            mode: Mode::Idle,
            level: 0.0,
            added_later: 99,
        };
        read.deserialize(&mut saved(&original()).as_slice())
            .unwrap();
        assert_eq!(read.counter, 1337);
        assert_eq!(read.mode, Mode::Busy);
        assert_eq!(read.added_later, 99);
    }

    #[test]
    fn fields_that_are_gone_or_moved_are_skipped() {
        #[derive(Debug, PartialEq, Serializable)]
        struct Reordered {
            level: f32,
            counter: u16,
        }
        let mut read = Reordered {
            level: 0.0,
            counter: 0,
        };
        read.deserialize(&mut saved(&original()).as_slice())
            .unwrap();
        assert_eq!(
            read,
            Reordered {
                level: 1.5,
                counter: 1337
            }
        );
    }

    #[test]
    fn a_field_or_variant_renamed_in_the_code_keeps_its_stored_name() {
        #[derive(Debug, PartialEq, Serializable)]
        enum RenamedMode {
            Idle,
            #[serializable(name = "Busy")]
            Working,
        }
        #[derive(Debug, PartialEq, Serializable)]
        struct Renamed {
            #[serializable(name = "counter")]
            ticks: u16,
            mode: RenamedMode,
        }
        let mut renamed = Renamed {
            ticks: 0,
            mode: RenamedMode::Idle,
        };
        renamed
            .deserialize(&mut saved(&original()).as_slice())
            .unwrap();
        assert_eq!(
            renamed,
            Renamed {
                ticks: 1337,
                mode: RenamedMode::Working
            }
        );

        let mut read = original();
        read.counter = 0;
        read.mode = Mode::Idle;
        read.deserialize(&mut saved(&renamed).as_slice()).unwrap();
        assert_eq!(read.counter, 1337);
        assert_eq!(read.mode, Mode::Busy);
    }

    #[test]
    fn a_field_whose_type_changed_under_the_same_name_is_reported() {
        #[derive(Debug, PartialEq, Serializable)]
        struct Wider {
            counter: u32,
            level: f32,
        }
        #[derive(Debug, PartialEq, Serializable)]
        struct Narrower {
            counter: u8,
        }
        let state = saved(&original());

        let mut wider = Wider {
            counter: 7,
            level: 0.0,
        };
        let unreadable = collect_unreadable(|| wider.deserialize(&mut state.as_slice())).unwrap();
        assert_eq!(unreadable, ["counter"]);
        assert_eq!(
            wider,
            Wider {
                counter: 7,
                level: 1.5
            }
        );

        let mut narrower = Narrower { counter: 0 };
        let unreadable =
            collect_unreadable(|| narrower.deserialize(&mut state.as_slice())).unwrap();
        assert_eq!(unreadable, ["counter"]);
    }

    #[test]
    fn an_unknown_variant_leaves_its_field_and_the_rest_still_loads() {
        #[derive(Debug, PartialEq, Serializable)]
        enum FewerModes {
            Idle,
        }
        #[derive(Debug, PartialEq, Serializable)]
        struct Older {
            counter: u16,
            mode: FewerModes,
        }
        let mut read = Older {
            counter: 0,
            mode: FewerModes::Idle,
        };
        let unreadable =
            collect_unreadable(|| read.deserialize(&mut saved(&original()).as_slice())).unwrap();
        assert_eq!(unreadable, ["mode"]);
        assert_eq!(
            read,
            Older {
                counter: 1337,
                mode: FewerModes::Idle
            }
        );
    }

    #[test]
    fn an_unknown_variant_on_its_own_is_an_unreadable_value() {
        #[derive(Debug, Serializable)]
        enum Fewer {
            Idle,
        }
        let mut bytes = Vec::new();
        Mode::Busy.serialize(&mut bytes).unwrap();
        let error = Fewer::Idle.deserialize(&mut bytes.as_slice()).unwrap_err();
        assert!(is_unreadable_value(&error));
    }

    #[test]
    fn damaged_data_inside_a_field_fails_the_whole_read() {
        #[derive(Serializable)]
        struct Outer {
            inner: Original,
        }
        let mut bytes = saved(&Outer { inner: original() });
        // The first name inside `inner` now claims more bytes than the field
        // holds: names and lengths that don't add up, which no version writes.
        let inner_start = 1 + "inner".len() + 4;
        bytes[inner_start] = 200;
        let mut read = Outer { inner: original() };
        assert!(read.deserialize(&mut bytes.as_slice()).is_err());
    }

    #[test]
    fn a_field_cut_short_is_refused_without_reading_past_the_state() {
        let state = saved(&original());
        for length in 0..state.len() {
            let mut read = original();
            assert!(read.deserialize(&mut &state[..length]).is_err());
        }
    }

    #[test]
    fn an_option_reads_into_the_value_already_there() {
        #[derive(Debug, Default, PartialEq, Serializable)]
        struct Kept {
            stored: u8,
            #[serializable(skip)]
            players: u8,
        }
        let saved_state = saved(&Some(Kept {
            stored: 7,
            players: 1,
        }));

        let mut read = Some(Kept {
            stored: 0,
            players: 9,
        });
        read.deserialize(&mut saved_state.as_slice()).unwrap();
        assert_eq!(
            read,
            Some(Kept {
                stored: 7,
                players: 9
            })
        );

        let mut read: Option<Kept> = None;
        read.deserialize(&mut saved_state.as_slice()).unwrap();
        assert_eq!(
            read,
            Some(Kept {
                stored: 7,
                players: 0
            })
        );
    }

    #[test]
    fn the_layout_lists_the_fields_of_values_that_are_not_there() {
        #[derive(Serializable)]
        struct Outer {
            maybe: Option<Original>,
            items: Vec<Original>,
        }
        let outer = Outer {
            maybe: None,
            items: Vec::new(),
        };
        let layout = stored_layout(|| outer.serialize(&mut std::io::sink())).unwrap();
        assert!(layout.contains("maybe: optional fields"));
        assert!(layout.contains("maybe.counter: u16"));
        assert!(layout.contains("items: list of fields"));
        assert!(layout.contains("items.mode = Busy"));
    }

    #[test]
    fn the_layout_names_nested_fields_once_and_every_variant() {
        #[derive(Serializable)]
        struct Outer {
            inner: Original,
            items: Vec<Original>,
        }
        let outer = Outer {
            inner: original(),
            items: vec![original(), original()],
        };
        let layout = stored_layout(|| outer.serialize(&mut std::io::sink())).unwrap();
        assert!(layout.contains("inner: fields"));
        assert!(layout.contains("inner.counter: u16"));
        assert!(layout.contains("inner.bytes: 4 bytes"));
        assert!(layout.contains("items.mode: variant"));
        assert!(layout.contains("inner.mode = Idle"));
        assert!(layout.contains("inner.mode = Busy"));
        assert_eq!(
            layout
                .iter()
                .filter(|path| path.starts_with("items."))
                .count(),
            7
        );
    }

    #[test]
    fn the_layout_shows_a_field_whose_type_changed() {
        #[derive(Serializable)]
        struct Narrow {
            value: u8,
        }
        #[derive(Serializable)]
        struct Wide {
            value: u16,
        }
        let layout_of = |value: &dyn Fn(&mut dyn Write) -> Result<()>| {
            stored_layout(|| value(&mut std::io::sink())).unwrap()
        };
        let narrow = layout_of(&|writer| Narrow { value: 1 }.serialize(writer));
        let wide = layout_of(&|writer| Wide { value: 1 }.serialize(writer));
        assert!(narrow.contains("value: u8"));
        assert!(wide.contains("value: u16"));
    }

    #[derive(Debug, PartialEq, Serializable)]
    struct WithSkippedField {
        saved: u8,
        #[serializable(skip)]
        cache: u16,
        also_saved: bool,
    }

    #[test]
    fn skipped_fields_are_not_written_and_keep_their_value_when_read() {
        let original = WithSkippedField {
            saved: 7,
            cache: 0xBEEF,
            also_saved: true,
        };
        let bytes = saved(&original);
        assert!(!bytes.windows(5).any(|window| window == b"cache"));

        let mut restored = WithSkippedField {
            saved: 0,
            cache: 0x1234,
            also_saved: false,
        };
        restored.deserialize(&mut bytes.as_slice()).unwrap();
        assert_eq!(
            restored,
            WithSkippedField {
                saved: 7,
                cache: 0x1234,
                also_saved: true,
            }
        );
    }

    #[derive(Debug, PartialEq, Serializable)]
    struct Pair<T: Serializable> {
        first: T,
        second: T,
    }

    #[test]
    fn generic_structs_serialize_each_field() {
        let bytes = saved(&Pair {
            first: 0x0102u16,
            second: 0x0304u16,
        });
        let mut pair = Pair {
            first: 0u16,
            second: 0u16,
        };
        pair.deserialize(&mut bytes.as_slice()).unwrap();
        assert_eq!(
            pair,
            Pair {
                first: 0x0102,
                second: 0x0304,
            }
        );
    }

    /// A `Vec` of bytes or byte arrays is written at once and read at once,
    /// in the same bytes as item by item.
    #[test]
    fn byte_lists_store_the_same_bytes_as_item_by_item() {
        let bytes: Vec<u8> = (0..=255).collect();
        let pixels: Vec<[u8; 4]> = (0..64u8)
            .map(|i| [i, i ^ 0x55, i.wrapping_mul(3), 0xFF])
            .collect();
        let mut written = Vec::new();
        bytes.serialize(&mut written).unwrap();
        pixels.serialize(&mut written).unwrap();

        let mut by_item = Vec::new();
        256u32.serialize(&mut by_item).unwrap();
        for byte in &bytes {
            by_item.write_all(&[*byte]).unwrap();
        }
        64u32.serialize(&mut by_item).unwrap();
        for pixel in &pixels {
            by_item.write_all(pixel).unwrap();
        }
        assert_eq!(written, by_item);

        let mut reader = written.as_slice();
        let mut read_bytes = Vec::new();
        let mut read_pixels = Vec::new();
        read_bytes.deserialize(&mut reader).unwrap();
        read_pixels.deserialize(&mut reader).unwrap();
        assert_eq!((read_bytes, read_pixels), (bytes, pixels));
        assert!(reader.is_empty());
    }

    #[test]
    fn byte_lists_that_end_early_do_not_read() {
        let mut written = Vec::new();
        vec![[1u8, 2, 3, 4]; 10].serialize(&mut written).unwrap();
        written.pop();
        let mut pixels = vec![[9u8; 4]];
        let error = pixels.deserialize(&mut written.as_slice()).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::UnexpectedEof);
        assert_eq!(pixels, vec![[9u8; 4]]);

        let mut bytes = Vec::new();
        u32::MAX.serialize(&mut bytes).unwrap();
        bytes.push(7);
        let mut list = Vec::<u8>::new();
        assert!(list.deserialize(&mut bytes.as_slice()).is_err());
    }
}
