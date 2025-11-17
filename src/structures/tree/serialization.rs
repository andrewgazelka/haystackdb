use std::io::{self, Write};

pub trait TreeSerialization {
    /// Writes the serialized value to the provided writer.
    fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()>;

    /// Returns the size in bytes of the serialized representation.
    fn serialized_size(&self) -> usize;

    /// Serializes the value to a byte vector.
    ///
    /// This is a convenience method that allocates a Vec. Prefer using `write_to`
    /// when possible to avoid allocations.
    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.serialized_size());
        self.write_to(&mut buf).expect("writing to Vec should not fail");
        buf
    }
}

pub trait TreeDeserialization {
    fn deserialize(data: &[u8]) -> Self
    where
        Self: Sized;
}

impl TreeDeserialization for i32 {
    fn deserialize(data: &[u8]) -> Self {
        let mut bytes = [0; 4];
        bytes.copy_from_slice(&data[..4]);
        i32::from_le_bytes(bytes)
    }
}

impl TreeSerialization for i32 {
    fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&self.to_le_bytes())
    }

    fn serialized_size(&self) -> usize {
        4
    }
}

impl TreeDeserialization for String {
    fn deserialize(data: &[u8]) -> Self {
        let mut bytes = Vec::new();
        let mut i = 4;
        while i < data.len() {
            let len = data[i..i + 4].try_into().unwrap();
            let len = i32::from_le_bytes(len) as usize;
            let start = i + 4;
            let end = start + len;
            bytes.extend_from_slice(&data[start..end]);
            i = end;
        }
        String::from_utf8(bytes).unwrap()
    }
}

impl TreeSerialization for String {
    fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&(self.len() as i32).to_le_bytes())?;
        writer.write_all(self.as_bytes())
    }

    fn serialized_size(&self) -> usize {
        4 + self.len()
    }
}
