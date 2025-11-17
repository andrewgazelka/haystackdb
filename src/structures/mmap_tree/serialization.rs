use std::io::{self, Read, Write};

/// Trait for serializing keys and values stored in B-Tree nodes to bytes.
///
/// This trait is used by the memory-mapped B-Tree implementation to persist
/// keys and values to disk. Types that can be stored in the B-Tree must
/// implement this trait to define their binary representation.
///
/// # Examples
///
/// ```ignore
/// impl TreeSerialization for i32 {
///     fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
///         writer.write_all(&self.to_le_bytes())
///     }
///
///     fn serialized_size(&self) -> usize {
///         4
///     }
/// }
/// ```
pub trait TreeSerialization {
    /// Writes the serialized value to the provided writer.
    ///
    /// The bytes will be written to the memory-mapped file backing the B-Tree.
    fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()>;

    /// Returns the size in bytes of the serialized representation.
    ///
    /// This is used to pre-calculate buffer sizes and avoid unnecessary allocations.
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

/// Trait for deserializing keys and values from bytes back into B-Tree node types.
///
/// This trait is the counterpart to `TreeSerialization` and is used to reconstruct
/// keys and values when loading B-Tree nodes from the memory-mapped file.
///
/// Uses a reader that automatically tracks position, making deserialization composable
/// without manual offset tracking.
///
/// # Examples
///
/// ```ignore
/// impl TreeDeserialization for i32 {
///     fn read_from<R: Read>(reader: &mut R) -> io::Result<Self> {
///         let mut bytes = [0; 4];
///         reader.read_exact(&mut bytes)?;
///         Ok(i32::from_le_bytes(bytes))
///     }
/// }
/// ```
pub trait TreeDeserialization {
    /// Reads and deserializes a value from the provided reader.
    ///
    /// The reader's position advances automatically as data is read.
    ///
    /// # Arguments
    ///
    /// * `reader` - Reader to deserialize from (e.g., `Cursor<&[u8]>`)
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or data is malformed.
    fn read_from<R: Read>(reader: &mut R) -> io::Result<Self>
    where
        Self: Sized;

    /// Convenience method to deserialize from a byte slice.
    ///
    /// Creates a cursor internally and calls `read_from`.
    fn deserialize(data: &[u8]) -> Self
    where
        Self: Sized,
    {
        let mut cursor = std::io::Cursor::new(data);
        Self::read_from(&mut cursor).expect("deserialization failed")
    }
}

impl TreeDeserialization for i32 {
    fn read_from<R: Read>(reader: &mut R) -> io::Result<Self> {
        let mut bytes = [0; 4];
        reader.read_exact(&mut bytes)?;
        Ok(i32::from_le_bytes(bytes))
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

impl TreeDeserialization for u64 {
    fn read_from<R: Read>(reader: &mut R) -> io::Result<Self> {
        let mut bytes = [0; 8];
        reader.read_exact(&mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    }
}

impl TreeSerialization for u64 {
    fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&self.to_le_bytes())
    }

    fn serialized_size(&self) -> usize {
        8
    }
}

impl TreeDeserialization for u128 {
    fn read_from<R: Read>(reader: &mut R) -> io::Result<Self> {
        let mut bytes = [0; 16];
        reader.read_exact(&mut bytes)?;
        Ok(u128::from_le_bytes(bytes))
    }
}

impl TreeSerialization for u128 {
    fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&self.to_le_bytes())
    }

    fn serialized_size(&self) -> usize {
        16
    }
}

impl TreeSerialization for usize {
    fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&self.to_le_bytes())
    }

    fn serialized_size(&self) -> usize {
        8
    }
}

impl TreeDeserialization for usize {
    fn read_from<R: Read>(reader: &mut R) -> io::Result<Self> {
        let mut bytes = [0; 8];
        reader.read_exact(&mut bytes)?;
        Ok(usize::from_le_bytes(bytes))
    }
}

impl TreeDeserialization for String {
    fn read_from<R: Read>(reader: &mut R) -> io::Result<Self> {
        let len = u64::read_from(reader)? as usize;
        let mut bytes = vec![0u8; len];
        reader.read_exact(&mut bytes)?;
        String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

impl TreeSerialization for u32 {
    fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&self.to_le_bytes())
    }

    fn serialized_size(&self) -> usize {
        4
    }
}

impl TreeSerialization for u8 {
    fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&[*self])
    }

    fn serialized_size(&self) -> usize {
        1
    }
}

impl TreeDeserialization for u8 {
    fn read_from<R: Read>(reader: &mut R) -> io::Result<Self> {
        let mut byte = [0u8; 1];
        reader.read_exact(&mut byte)?;
        Ok(byte[0])
    }
}

impl<T: TreeSerialization> TreeSerialization for [T] {
    fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        (self.len() as u64).write_to(writer)?;
        for item in self {
            item.write_to(writer)?;
        }
        Ok(())
    }

    fn serialized_size(&self) -> usize {
        8 + self.iter().map(|item| item.serialized_size()).sum::<usize>()
    }
}

impl<T: TreeSerialization, const N: usize> TreeSerialization for [T; N] {
    fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        // Fixed-size arrays don't need length prefix since size is known at compile time
        for item in self {
            item.write_to(writer)?;
        }
        Ok(())
    }

    fn serialized_size(&self) -> usize {
        self.iter().map(|item| item.serialized_size()).sum::<usize>()
    }
}

impl<T: TreeDeserialization, const N: usize> TreeDeserialization for [T; N] {
    fn read_from<R: Read>(reader: &mut R) -> io::Result<Self> {
        // Fixed-size arrays don't have length prefix
        let mut result = Vec::with_capacity(N);
        for _ in 0..N {
            result.push(T::read_from(reader)?);
        }
        result
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "array size mismatch"))
    }
}

impl<T: TreeSerialization> TreeSerialization for Vec<T> {
    fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        self.as_slice().write_to(writer)
    }

    fn serialized_size(&self) -> usize {
        self.as_slice().serialized_size()
    }
}

impl<T: TreeDeserialization> TreeDeserialization for Vec<T> {
    fn read_from<R: Read>(reader: &mut R) -> io::Result<Self> {
        let len = u64::read_from(reader)? as usize;
        let mut result = Vec::with_capacity(len);
        for _ in 0..len {
            result.push(T::read_from(reader)?);
        }
        Ok(result)
    }
}

impl TreeSerialization for String {
    fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        self.as_bytes().write_to(writer)
    }

    fn serialized_size(&self) -> usize {
        self.as_bytes().serialized_size()
    }
}
