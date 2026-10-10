use std::io::{Read, Write, Result};

pub trait Serializable {
    fn serialize(&self, writer: &mut dyn Write) -> Result<()>;
    fn deserialize(&mut self, reader: &mut dyn Read) -> Result<()>;
}

impl Serializable for u8 {
    fn serialize(&self, writer: &mut dyn Write)-> Result<()> {
        writer.write_all(&[*self])
    }
    
    fn deserialize(&mut self, reader: &mut dyn Read)-> Result<()> {
        let mut buf = [0u8; 1];
        reader.read_exact(&mut buf)?;
        *self = buf[0];
        Ok(())
    }
}

impl Serializable for u16 {
    fn serialize(&self, writer: &mut dyn Write)-> Result<()> {
        writer.write_all(&self.to_le_bytes())
    }
    
    fn deserialize(&mut self, reader: &mut dyn Read)-> Result<()> {
        let mut buf = [0u8; 2];
        reader.read_exact(&mut buf)?;
        *self = u16::from_le_bytes(buf);
        Ok(())
    }
}

impl Serializable for bool {
    fn serialize(&self, writer: &mut dyn Write)-> Result<()> {
        (*self as u8).serialize(writer)
    }
    
    fn deserialize(&mut self, reader: &mut dyn Read)-> Result<()> {
        let mut value = 0u8;
        value.deserialize(reader)?;
        *self = value != 0;
        Ok(())
    }
}

impl Serializable for f32 {
    fn serialize(&self, writer: &mut dyn Write)-> Result<()> {
        writer.write_all(&self.to_le_bytes())
    }
    
    fn deserialize(&mut self, reader: &mut dyn Read)-> Result<()> {
        let mut buf = [0u8; 4];
        reader.read_exact(&mut buf)?;
        *self = f32::from_le_bytes(buf);
        Ok(())
    }
}

impl Serializable for f64 {
    fn serialize(&self, writer: &mut dyn Write)-> Result<()> {
        writer.write_all(&self.to_le_bytes())
    }
    
    fn deserialize(&mut self, reader: &mut dyn Read)-> Result<()> {
        let mut buf = [0u8; 8];
        reader.read_exact(&mut buf)?;
        *self = f64::from_le_bytes(buf);
        Ok(())
    }
}

impl<const N: usize> Serializable for [u8; N] {
    fn serialize(&self, writer: &mut dyn Write)-> Result<()> {
        writer.write_all(self)
    }
    
    fn deserialize(&mut self, reader: &mut dyn Read)-> Result<()> {
        reader.read_exact(self)?;
        Ok(())
    }
}

impl<T: Serializable + Default> Serializable for Vec<T> {
    fn serialize(&self, writer: &mut dyn Write)-> Result<()> {
        (self.len() as u32).serialize(writer)?;
        for item in self {
            item.serialize(writer)?;
        }
        Ok(())
    }
    
    fn deserialize(&mut self, reader: &mut dyn Read)-> Result<()> {
        let mut len = 0u32;
        len.deserialize(reader)?;
        
        self.clear();
        self.reserve(len as usize);
        
        for _ in 0..len {
            let mut item = T::default();
            item.deserialize(reader)?;
            self.push(item);
        }
        Ok(())
    }
}

impl<T: Serializable + Default> Serializable for Option<T> {
    fn serialize(&self, writer: &mut dyn Write)-> Result<()> {
        match self {
            Some(value) => {
                1u8.serialize(writer)?;
                value.serialize(writer)
            }
            None => 0u8.serialize(writer)
        }
    }
    
    fn deserialize(&mut self, reader: &mut dyn Read)-> Result<()> {
        let mut tag = 0u8;
        tag.deserialize(reader)?;
        
        match tag {
            0 => *self = None,
            1 => {
                let mut value = T::default();
                value.deserialize(reader)?;
                *self = Some(value);
            }
            _ => return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid Option tag"
            ))
        }
        Ok(())
    }
}

impl Serializable for u32 {
    fn serialize(&self, writer: &mut dyn Write)-> Result<()> {
        writer.write_all(&self.to_le_bytes())
    }
    
    fn deserialize(&mut self, reader: &mut dyn Read)-> Result<()> {
        let mut buf = [0u8; 4];
        reader.read_exact(&mut buf)?;
        *self = u32::from_le_bytes(buf);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serializable_derive::Serializable;
    use std::io::Cursor;
    use super::*;

    #[derive(Debug, PartialEq, Serializable)]
    enum TestEnum {
        Test1,
        Test2
    }

    #[derive(Debug, PartialEq, Serializable)]
    struct TestStruct {
        byte_value: u8,
        word_value: u16,
        bool_value: bool,
        float_value: f32,
        array_value: [u8; 4],
        test_enum: TestEnum
    }

    #[test]
    fn test_serialize_to_expected_bytes() {
        let test_struct = TestStruct {
            byte_value: 42,
            word_value: 1337,
            bool_value: true,
            float_value: 3.14159,
            array_value: [0xDE, 0xAD, 0xBE, 0xEF],
            test_enum: TestEnum::Test2
        };

        let mut expected = Vec::new();
        expected.push(42u8);                           // byte_value
        expected.extend_from_slice(&1337u16.to_le_bytes()); // word_value
        expected.push(1u8);                            // bool_value (true as 1)
        expected.extend_from_slice(&3.14159f32.to_le_bytes()); // float_value
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
            test_enum: TestEnum::Test1
        };

        let mut input = Vec::new();
        input.push(100u8);                             // byte_value
        input.extend_from_slice(&500u16.to_le_bytes()); // word_value
        input.push(0u8);                               // bool_value (false as 0)
        input.extend_from_slice(&2.718f32.to_le_bytes()); // float_value
        input.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]); // array_value
        input.push(1u8); // test_enum (index 1)

        let mut cursor = Cursor::new(input);
        test_struct.deserialize(&mut cursor).unwrap();

        let expected = TestStruct {
            byte_value: 100,
            word_value: 500,
            bool_value: false,
            float_value: 2.718,
            array_value: [0x01, 0x02, 0x03, 0x04],
            test_enum: TestEnum::Test2
        };

        assert_eq!(test_struct, expected);
    }
}
