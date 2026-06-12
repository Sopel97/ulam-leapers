#[derive(Debug, Eq, PartialEq)]
pub enum ByteReaderError {
    UnexpectedEndOfStream,
}

#[derive(Debug)]
pub struct ByteReader<'a> {
    data: &'a [u8],
    cursor: usize,
}

macro_rules! impl_try_read_le {
    ($t: ident, $name: ident) => {
        #[inline(always)]
        pub fn $name(&mut self) -> Result<$t, ByteReaderError> {
            const BYTES: usize = size_of::<$t>();
            if self.cursor + BYTES > self.data.len() {
                return Err(ByteReaderError::UnexpectedEndOfStream);
            }

            let mut arr = [0u8; BYTES];
            arr.copy_from_slice(&self.data[self.cursor..self.cursor + BYTES]);
            self.cursor += BYTES;

            Ok($t::from_le_bytes(arr))
        }
    };
}

impl<'a> ByteReader<'a> {
    pub fn new(data: &'a [u8]) -> ByteReader<'a> {
        ByteReader { data, cursor: 0 }
    }

    #[inline(always)]
    pub fn try_read_u8(&mut self) -> Result<u8, ByteReaderError> {
        if self.cursor >= self.data.len() {
            return Err(ByteReaderError::UnexpectedEndOfStream);
        }

        let res = self.data[self.cursor];
        self.cursor += 1;

        Ok(res)
    }

    impl_try_read_le!(u16, try_read_u16);
    impl_try_read_le!(u32, try_read_u32);
    impl_try_read_le!(u64, try_read_u64);
    impl_try_read_le!(i16, try_read_i16);
    impl_try_read_le!(i32, try_read_i32);
    impl_try_read_le!(i64, try_read_i64);

    // Panics if n > 8.
    pub fn try_read_bytes_as_u64(&mut self, n: usize) -> Result<u64, ByteReaderError> {
        assert!(n <= 8);

        if self.cursor + n > self.data.len() {
            return Err(ByteReaderError::UnexpectedEndOfStream);
        }

        let mut res = 0u64;
        for i in 0..n {
            res |= (self.data[self.cursor + i] as u64) << (8 * i as u64);
        }

        self.cursor += n;

        Ok(res)
    }

    pub fn try_read_slice(&mut self, n: usize) -> Result<&'a [u8], ByteReaderError> {
        if self.cursor + n > self.data.len() {
            return Err(ByteReaderError::UnexpectedEndOfStream);
        }

        let res = &self.data[self.cursor..self.cursor + n];
        self.cursor += n;
        Ok(res)
    }

    /// Skips `cursor` by `n` bytes iff the positions after skip would be valid.
    /// All cursor positions within the `data` slice are valid, as well
    /// as the one past the end position.
    pub fn try_skip(&mut self, n: usize) -> Result<(), ByteReaderError> {
        if self.cursor + n > self.data.len() {
            return Err(ByteReaderError::UnexpectedEndOfStream);
        }

        self.cursor += n;
        Ok(())
    }

    pub fn is_eof(&self) -> bool {
        self.cursor >= self.data.len()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_empty_slice_is_eof() {
        let r = ByteReader::new(&[]);
        assert!(r.is_eof());
    }

    #[test]
    fn new_nonempty_slice_is_not_eof() {
        let r = ByteReader::new(&[0x00]);
        assert!(!r.is_eof());
    }

    #[test]
    fn read_u8_single_byte() {
        let mut r = ByteReader::new(&[0xAB]);
        assert_eq!(r.try_read_u8(), Ok(0xAB));
        assert!(r.is_eof());
    }

    #[test]
    fn read_u8_advances_cursor() {
        let mut r = ByteReader::new(&[0x01, 0x02, 0x03]);
        assert_eq!(r.try_read_u8(), Ok(0x01));
        assert_eq!(r.try_read_u8(), Ok(0x02));
        assert_eq!(r.try_read_u8(), Ok(0x03));
        assert_eq!(r.try_read_u8(), Err(ByteReaderError::UnexpectedEndOfStream));
    }

    #[test]
    fn read_u8_empty_returns_none() {
        let mut r = ByteReader::new(&[]);
        assert_eq!(r.try_read_u8(), Err(ByteReaderError::UnexpectedEndOfStream));
    }

    #[test]
    fn read_u8_at_eof_returns_none() {
        let mut r = ByteReader::new(&[0xFF]);
        r.try_read_u8().unwrap();
        assert_eq!(r.try_read_u8(), Err(ByteReaderError::UnexpectedEndOfStream));
    }

    #[test]
    fn read_u16_little_endian() {
        let mut r = ByteReader::new(&[0x01, 0x00]);
        assert_eq!(r.try_read_u16(), Ok(1u16));
    }

    #[test]
    fn read_u16_byte_order() {
        // 0x0102 in LE is stored as [0x02, 0x01]
        let mut r = ByteReader::new(&[0x02, 0x01]);
        assert_eq!(r.try_read_u16(), Ok(0x0102u16));
    }

    #[test]
    fn read_u16_max() {
        let mut r = ByteReader::new(&[0xFF, 0xFF]);
        assert_eq!(r.try_read_u16(), Ok(u16::MAX));
    }

    #[test]
    fn read_u16_insufficient_bytes_returns_none() {
        let mut r = ByteReader::new(&[0x01]); // only 1 byte, need 2
        assert_eq!(
            r.try_read_u16(),
            Err(ByteReaderError::UnexpectedEndOfStream)
        );
    }

    #[test]
    fn read_u16_does_not_advance_on_failure() {
        let mut r = ByteReader::new(&[0x42]);
        assert_eq!(
            r.try_read_u16(),
            Err(ByteReaderError::UnexpectedEndOfStream)
        );
        // cursor should be unchanged; u8 read should still work
        assert_eq!(r.try_read_u8(), Ok(0x42));
    }

    #[test]
    fn read_u32_little_endian() {
        let mut r = ByteReader::new(&[0x78, 0x56, 0x34, 0x12]);
        assert_eq!(r.try_read_u32(), Ok(0x12345678u32));
    }

    #[test]
    fn read_u32_max() {
        let mut r = ByteReader::new(&[0xFF; 4]);
        assert_eq!(r.try_read_u32(), Ok(u32::MAX));
    }

    #[test]
    fn read_u32_insufficient_bytes_returns_none() {
        let mut r = ByteReader::new(&[0x01, 0x02, 0x03]); // need 4
        assert_eq!(
            r.try_read_u32(),
            Err(ByteReaderError::UnexpectedEndOfStream)
        );
    }

    #[test]
    fn read_u64_little_endian() {
        let val: u64 = 0x0102030405060708;
        let bytes = val.to_le_bytes();
        let mut r = ByteReader::new(&bytes);
        assert_eq!(r.try_read_u64(), Ok(val));
    }

    #[test]
    fn read_u64_max() {
        let mut r = ByteReader::new(&[0xFF; 8]);
        assert_eq!(r.try_read_u64(), Ok(u64::MAX));
    }

    #[test]
    fn read_u64_insufficient_bytes_returns_none() {
        let mut r = ByteReader::new(&[0x00; 7]); // need 8
        assert_eq!(
            r.try_read_u64(),
            Err(ByteReaderError::UnexpectedEndOfStream)
        );
    }

    #[test]
    fn read_i16_negative() {
        let val: i16 = -1;
        let val_bytes = val.to_le_bytes();
        let mut r = ByteReader::new(&val_bytes);
        assert_eq!(r.try_read_i16(), Ok(val));
    }

    #[test]
    fn read_i16_min_max() {
        let val = i16::MIN;
        let val_bytes = val.to_le_bytes();
        let mut r = ByteReader::new(&val_bytes);
        assert_eq!(r.try_read_i16(), Ok(i16::MIN));

        let val = i16::MAX;
        let val_bytes = val.to_le_bytes();
        let mut r = ByteReader::new(&val_bytes);
        assert_eq!(r.try_read_i16(), Ok(i16::MAX));
    }

    #[test]
    fn read_i32_negative() {
        let val: i32 = -123456;
        let val_bytes = val.to_le_bytes();
        let mut r = ByteReader::new(&val_bytes);
        assert_eq!(r.try_read_i32(), Ok(val));
    }

    #[test]
    fn read_i32_min_max() {
        let val = i32::MIN;
        let val_bytes = val.to_le_bytes();
        let mut r = ByteReader::new(&val_bytes);
        assert_eq!(r.try_read_i32(), Ok(i32::MIN));

        let val = i32::MAX;
        let val_bytes = val.to_le_bytes();
        let mut r = ByteReader::new(&val_bytes);
        assert_eq!(r.try_read_i32(), Ok(i32::MAX));
    }

    #[test]
    fn read_i64_negative() {
        let val: i64 = i64::MIN / 2;
        let val_bytes = val.to_le_bytes();
        let mut r = ByteReader::new(&val_bytes);
        assert_eq!(r.try_read_i64(), Ok(val));
    }

    #[test]
    fn skip_zero_bytes_always_succeeds() {
        let mut r = ByteReader::new(&[]);
        assert_eq!(r.try_skip(0), Ok(()));
    }

    #[test]
    fn skip_advances_cursor() {
        let mut r = ByteReader::new(&[0x01, 0x02, 0x03, 0x04]);
        assert_eq!(r.try_skip(2), Ok(()));
        assert_eq!(r.try_read_u8(), Ok(0x03));
    }

    #[test]
    fn skip_to_exact_end_succeeds() {
        let mut r = ByteReader::new(&[0x01, 0x02]);
        assert_eq!(r.try_skip(2), Ok(()));
        assert!(r.is_eof());
    }

    #[test]
    fn skip_past_end_fails_and_does_not_advance() {
        let mut r = ByteReader::new(&[0x01, 0x02]);
        assert_eq!(r.try_skip(3), Err(ByteReaderError::UnexpectedEndOfStream));
        // cursor unchanged — can still read both bytes
        assert_eq!(r.try_read_u8(), Ok(0x01));
        assert_eq!(r.try_read_u8(), Ok(0x02));
    }

    #[test]
    fn skip_on_empty_slice_fails() {
        let mut r = ByteReader::new(&[]);
        assert_eq!(r.try_skip(1), Err(ByteReaderError::UnexpectedEndOfStream));
    }

    #[test]
    fn is_eof_false_until_all_bytes_consumed() {
        let mut r = ByteReader::new(&[0xDE, 0xAD]);
        assert!(!r.is_eof());
        r.try_read_u8().unwrap();
        assert!(!r.is_eof());
        r.try_read_u8().unwrap();
        assert!(r.is_eof());
    }

    #[test]
    fn mixed_reads_in_sequence() {
        // Simulates reading a small binary record:
        //   u8  tag     = 0x01
        //   u16 length  = 0x0005
        //   u32 payload = 0xDEAD_BEEF
        let data: &[u8] = &[0x01, 0x05, 0x00, 0xEF, 0xBE, 0xAD, 0xDE];
        let mut r = ByteReader::new(data);

        assert_eq!(r.try_read_u8(), Ok(0x01));
        assert_eq!(r.try_read_u16(), Ok(5u16));
        assert_eq!(r.try_read_u32(), Ok(0xDEAD_BEEFu32));
        assert!(r.is_eof());
    }

    #[test]
    fn read_after_skip_returns_correct_data() {
        let data: &[u8] = &[0xFF, 0xFF, 0x42, 0x00];
        let mut r = ByteReader::new(data);
        assert_eq!(r.try_skip(2), Ok(()));
        assert_eq!(r.try_read_u16(), Ok(0x0042u16));
        assert!(r.is_eof());
    }

    #[test]
    fn partial_read_leaves_cursor_intact_on_failure() {
        // 3 bytes available; try to read u32 (needs 4) — should fail without
        // moving the cursor, then fall back to reading three individual bytes.
        let data: &[u8] = &[0xAA, 0xBB, 0xCC];
        let mut r = ByteReader::new(data);
        assert_eq!(
            r.try_read_u32(),
            Err(ByteReaderError::UnexpectedEndOfStream)
        );
        assert_eq!(r.try_read_u8(), Ok(0xAA));
        assert_eq!(r.try_read_u8(), Ok(0xBB));
        assert_eq!(r.try_read_u8(), Ok(0xCC));
        assert!(r.is_eof());
    }
}
