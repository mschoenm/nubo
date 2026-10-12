//! The binary format of save states.
//!
//! A save state is every component's state written one after another as raw
//! bytes, with no field names or type tags. Reading it back only works if
//! every value is read in exactly the order and size it was written, so:
//!
//! - Never reorder, add or remove fields of a type that derives
//!   `Serializable`, and never reorder the calls in a manual implementation,
//!   without bumping the save state format version in `emulator/save_state.rs`.
//! - Numbers are stored little-endian (lowest byte first), so a save state
//!   works on every platform.

use std::io::{Error, ErrorKind, Read, Result, Write};

/// A value that can be written to and read back from a save state.
///
/// Most types get this with `#[derive(Serializable)]` from the
/// `serializable_derive` crate, which handles each field in declaration order.
/// `deserialize` overwrites `self` in place, so a component keeps parts of its
/// state that are not saved, such as the loaded ROM.
pub trait Serializable {
    /// Appends this value's bytes to `writer`.
    fn serialize(&self, writer: &mut dyn Write) -> Result<()>;

    /// Replaces this value with one read from `reader`. Fails when the data
    /// ends early or holds a value this type can't have.
    fn deserialize(&mut self, reader: &mut dyn Read) -> Result<()>;
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
            }
        )*
    };
}

impl_serializable_for_numbers!(u8, u16, u32, u64, i16, f32, f64);

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
}

/// A byte array is stored as its bytes, without a length.
impl<const N: usize> Serializable for [u8; N] {
    fn serialize(&self, writer: &mut dyn Write) -> Result<()> {
        writer.write_all(self)
    }

    fn deserialize(&mut self, reader: &mut dyn Read) -> Result<()> {
        reader.read_exact(self)
    }
}

/// A `Vec` is stored as its length (a `u32`) followed by its items.
impl<T: Serializable + Default> Serializable for Vec<T> {
    fn serialize(&self, writer: &mut dyn Write) -> Result<()> {
        let length = u32::try_from(self.len())
            .map_err(|_| Error::new(ErrorKind::InvalidInput, "Vec is too long to save"))?;
        length.serialize(writer)?;
        for item in self {
            item.serialize(writer)?;
        }
        Ok(())
    }

    fn deserialize(&mut self, reader: &mut dyn Read) -> Result<()> {
        let mut length = 0u32;
        length.deserialize(reader)?;

        self.clear();

        for _ in 0..length {
            let mut item = T::default();
            item.deserialize(reader)?;
            self.push(item);
        }
        Ok(())
    }
}

/// An `Option` is stored as a 0 byte for `None`, or a 1 byte followed by the value.
impl<T: Serializable + Default> Serializable for Option<T> {
    fn serialize(&self, writer: &mut dyn Write) -> Result<()> {
        match self {
            Some(value) => {
                1u8.serialize(writer)?;
                value.serialize(writer)
            }
            None => 0u8.serialize(writer),
        }
    }

    fn deserialize(&mut self, reader: &mut dyn Read) -> Result<()> {
        let mut tag = 0u8;
        tag.deserialize(reader)?;

        match tag {
            0 => *self = None,
            1 => {
                let mut value = T::default();
                value.deserialize(reader)?;
                *self = Some(value);
            }
            _ => {
                return Err(Error::new(ErrorKind::InvalidData, "Invalid Option tag"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serializable_derive::Serializable;
    use std::io::Cursor;

    #[derive(Debug, PartialEq, Serializable)]
    enum TestEnum {
        Test1,
        Test2,
    }

    #[derive(Debug, PartialEq, Serializable)]
    struct TestStruct {
        byte_value: u8,
        word_value: u16,
        bool_value: bool,
        float_value: f32,
        array_value: [u8; 4],
        test_enum: TestEnum,
    }

    #[test]
    fn test_serialize_to_expected_bytes() {
        let test_struct = TestStruct {
            byte_value: 42,
            word_value: 1337,
            bool_value: true,
            float_value: 1.5,
            array_value: [0xDE, 0xAD, 0xBE, 0xEF],
            test_enum: TestEnum::Test2,
        };

        let mut expected = Vec::new();
        expected.push(42u8); // byte_value
        expected.extend_from_slice(&1337u16.to_le_bytes()); // word_value
        expected.push(1u8); // bool_value (true as 1)
        expected.extend_from_slice(&1.5f32.to_le_bytes()); // float_value
        expected.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]); // array_value
        expected.push(1u8); // test_enum (index 1)

        let mut buffer = Vec::new();
        test_struct.serialize(&mut buffer).unwrap();

        assert_eq!(buffer, expected);
    }

    #[test]
    fn test_deserialize_from_bytes() {
        let mut test_struct = TestStruct {
            byte_value: 0,
            word_value: 0,
            bool_value: false,
            float_value: 0.0,
            array_value: [0, 0, 0, 0],
            test_enum: TestEnum::Test1,
        };

        let mut input = Vec::new();
        input.push(100u8); // byte_value
        input.extend_from_slice(&500u16.to_le_bytes()); // word_value
        input.push(0u8); // bool_value (false as 0)
        input.extend_from_slice(&2.5f32.to_le_bytes()); // float_value
        input.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]); // array_value
        input.push(1u8); // test_enum (index 1)

        let mut cursor = Cursor::new(input);
        test_struct.deserialize(&mut cursor).unwrap();

        let expected = TestStruct {
            byte_value: 100,
            word_value: 500,
            bool_value: false,
            float_value: 2.5,
            array_value: [0x01, 0x02, 0x03, 0x04],
            test_enum: TestEnum::Test2,
        };

        assert_eq!(test_struct, expected);
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
        let mut buffer = Vec::new();
        original.serialize(&mut buffer).unwrap();
        assert_eq!(buffer, [7, 1]);

        let mut restored = WithSkippedField {
            saved: 0,
            cache: 0x1234,
            also_saved: false,
        };
        let mut reader = Cursor::new(buffer);
        restored.deserialize(&mut reader).unwrap();
        assert_eq!(reader.position(), 2);
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
        let mut buffer = Vec::new();
        Pair {
            first: 0x0102u16,
            second: 0x0304u16,
        }
        .serialize(&mut buffer)
        .unwrap();
        assert_eq!(buffer, [0x02, 0x01, 0x04, 0x03]);

        let mut pair = Pair {
            first: 0u16,
            second: 0u16,
        };
        pair.deserialize(&mut Cursor::new(buffer)).unwrap();
        assert_eq!(
            pair,
            Pair {
                first: 0x0102,
                second: 0x0304,
            }
        );
    }
}
