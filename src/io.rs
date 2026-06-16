use crate::util::memory::view_as_bytes_mut;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::mem;

pub trait WriteTo {
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()>;
}

pub trait ReadFrom: Sized {
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self>;
}

impl WriteTo for [u8] {
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        self.len().write_to(writer)?;
        writer.write_all(self)
    }
}

impl ReadFrom for Box<[u8]> {
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        let len = usize::read_from(reader)?;
        let mut res = Box::new_uninit_slice(len);
        let raw_res = unsafe { view_as_bytes_mut(&mut res) };
        reader.read_exact(raw_res)?;
        // SAFETY: We have read exactly len bytes (u8s) into the buffer.
        Ok(unsafe { res.assume_init() })
    }
}

impl<T> WriteTo for Vec<T>
where
    T: WriteTo,
{
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        writer.write_all(&self.len().to_le_bytes())?;
        for player in self.iter() {
            player.write_to(writer)?;
        }
        Ok(())
    }
}

impl<T> ReadFrom for Vec<T>
where
    T: ReadFrom,
{
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        let len = usize::read_from(reader)?;
        let mut res = Vec::with_capacity(len);
        for _ in 0..len {
            res.push(T::read_from(reader)?);
        }
        Ok(res)
    }
}

impl<T> WriteTo for Box<T>
where
    T: WriteTo,
{
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        (**self).write_to(writer)
    }
}

impl<K, V> WriteTo for BTreeMap<K, V>
where
    K: WriteTo + Ord,
    V: WriteTo,
{
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        self.len().write_to(writer)?;
        for (k, v) in self.iter() {
            k.write_to(writer)?;
            v.write_to(writer)?;
        }
        Ok(())
    }
}

impl<K, V> ReadFrom for BTreeMap<K, V>
where
    K: ReadFrom + Ord,
    V: ReadFrom,
{
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        let len = usize::read_from(reader)?;
        let mut res = BTreeMap::new();
        for _ in 0..len {
            res.insert(K::read_from(reader)?, V::read_from(reader)?);
        }
        Ok(res)
    }
}

macro_rules! impl_write_to_le_bytes {
    ($t: ident) => {
        impl WriteTo for $t {
            fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
                writer.write_all(&self.to_le_bytes())
            }
        }
    };
}

impl_write_to_le_bytes!(i8);
impl_write_to_le_bytes!(u8);
impl_write_to_le_bytes!(i16);
impl_write_to_le_bytes!(u16);
impl_write_to_le_bytes!(i32);
impl_write_to_le_bytes!(u32);
impl_write_to_le_bytes!(i64);
impl_write_to_le_bytes!(u64);
impl_write_to_le_bytes!(isize);
impl_write_to_le_bytes!(usize);

macro_rules! impl_read_from_le_bytes {
    ($t: ident) => {
        impl ReadFrom for $t {
            fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
                let mut buf = [0u8; mem::size_of::<$t>()];
                reader.read_exact(&mut buf)?;
                Ok($t::from_le_bytes(buf))
            }
        }
    };
}

impl_read_from_le_bytes!(i8);
impl_read_from_le_bytes!(u8);
impl_read_from_le_bytes!(i16);
impl_read_from_le_bytes!(u16);
impl_read_from_le_bytes!(i32);
impl_read_from_le_bytes!(u32);
impl_read_from_le_bytes!(i64);
impl_read_from_le_bytes!(u64);
impl_read_from_le_bytes!(isize);
impl_read_from_le_bytes!(usize);

macro_rules! impl_write_int_le {
    ($t: ident, $name: ident) => {
        pub fn $name(writer: &mut impl Write, val: $t) -> std::io::Result<()> {
            writer.write_all(&val.to_le_bytes())
        }
    }
}

impl_write_int_le!(i8, write_i8_le);
impl_write_int_le!(u8, write_u8_le);
impl_write_int_le!(i16, write_i16_le);
impl_write_int_le!(u16, write_u16_le);
impl_write_int_le!(i32, write_i32_le);
impl_write_int_le!(u32, write_u32_le);
impl_write_int_le!(i64, write_i64_le);
impl_write_int_le!(u64, write_u64_le);
impl_write_int_le!(isize, write_isize_le);
impl_write_int_le!(usize, write_usize_le);

macro_rules! impl_read_int_le {
    ($t: ident, $name: ident) => {
        pub fn $name(reader: &mut impl Read) -> std::io::Result<$t> {
            let mut buf = [0u8; mem::size_of::<$t>()];
            reader.read_exact(&mut buf)?;
            Ok($t::from_le_bytes(buf))
        }
    }
}

impl_read_int_le!(i8, read_i8_le);
impl_read_int_le!(u8, read_u8_le);
impl_read_int_le!(i16, read_i16_le);
impl_read_int_le!(u16, read_u16_le);
impl_read_int_le!(i32, read_i32_le);
impl_read_int_le!(u32, read_u32_le);
impl_read_int_le!(i64, read_i64_le);
impl_read_int_le!(u64, read_u64_le);
impl_read_int_le!(isize, read_isize_le);
impl_read_int_le!(usize, read_usize_le);