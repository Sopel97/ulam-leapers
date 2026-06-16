use std::io::{Read, Write};
use std::mem;

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