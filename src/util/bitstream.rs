use crate::util::bytestream::ByteReader;

// Making this a type parameter of LittleEndianBitReader
// is not feasible due to lack of duck typing.
// This is unfortunate, because due to `read_bits_as_u64` it requires `WORD_BITS >= 64` to compile.
// A generic implementation would allow us to create specializations based on the word size
// to assemble the needed type from smaller parts, but as it stands it's a lot of trait fuckery.
type WordType = u64;
const WORD_BYTES: usize = size_of::<WordType>();
const WORD_BITS: usize = WordType::BITS as usize;

pub struct LittleEndianBitReader<'a> {
    data: &'a [u8],
    offset: usize,
    prefill_low_bits: usize,
    prefill_low: WordType,
    prefill_high: WordType,
}

impl<'a> LittleEndianBitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            offset: 2 * WORD_BYTES,
            prefill_low_bits: WORD_BITS,
            prefill_low: prefill_from_slice(data, 0),
            prefill_high: prefill_from_slice(data, 8),
        }
    }

    /// Read next `bits` bits from little-endian bytes.
    /// Reading past the end is valid, bytes past the end are assumed to be 0.
    ///
    /// Inlining is suggested because intended usage is with a constant number of bits,
    /// in which case some branches can be omitted in most cases.
    #[inline]
    pub fn read_bits_as_u64(&mut self, bits: usize) -> u64 {
        const { assert!(WORD_BITS >= 64) }
        assert!(
            bits <= WORD_BITS,
            "Cannot read more than {WORD_BITS} bits at a time"
        );

        let mut res;
        if bits < self.prefill_low_bits {
            // We know it's `<WORD_BITS` bits so we can apply the mask unconditionally.
            res = self.prefill_low & ((1u64 << bits) - 1);
            self.prefill_low >>= bits;
            self.prefill_low_bits -= bits;
        } else if bits == self.prefill_low_bits {
            // We have to handle some shifts by 64 here.
            res = self.prefill_low;
            self.prefill_low_bits = WORD_BITS;
            self.prefill_low = self.prefill_high;
            self.prefill_high = prefill_from_slice(self.data, self.offset);
            self.offset += WORD_BYTES;
        } else {
            // No shifts by `WORD_BITS` here.
            let bits_from_high = bits - self.prefill_low_bits;
            res = self.prefill_low | (self.prefill_high << self.prefill_low_bits);
            if bits != WORD_BITS {
                res &= (1u64 << bits) - 1;
            }
            self.prefill_low_bits = WORD_BITS - bits_from_high;
            self.prefill_low = self.prefill_high >> bits_from_high;
            self.prefill_high = prefill_from_slice(self.data, self.offset);
            self.offset += WORD_BYTES;
        }

        res
    }
    
    pub fn is_byte_aligned(&self) -> bool {
        self.prefill_low_bits % 8 == 0
    }

    pub fn try_into_byte_reader(self) -> Option<ByteReader<'a>> {
        if self.prefill_low_bits % 8 != 0 {
            None
        } else {
            let actual_offset = self.offset - WORD_BYTES - (self.prefill_low_bits / 8);
            Some(ByteReader::new(&self.data[actual_offset..]))
        }
    }
}

fn prefill_from_slice(slice: &[u8], offset: usize) -> WordType {
    // Carefully written to avoid one redundant branch for a panic.
    // However, there is still one branch that checks of `offset` overflow that
    // we cannot avoid without unsafe code.
    if slice.len() >= offset + WORD_BYTES {
        let mut arr = [0u8; WORD_BYTES];
        arr.copy_from_slice(&slice[offset..offset + WORD_BYTES]);
        WordType::from_le_bytes(arr)
    } else if slice.len() > offset {
        let bytes_left_in_slice = slice.len() - offset;
        let mut res = 0;
        for i in 0..bytes_left_in_slice {
            res |= (slice[offset + i] as WordType) << (WORD_BYTES * i);
        }
        res
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zeropad_empty_slice() {
        assert_eq!(prefill_from_slice(&[], 0), 0);
    }

    #[test]
    fn zeropad_partial_slice() {
        // [0x01, 0x02, 0x03] at offset 0 → 0x00_00_00_00_03_02_01
        let data = [0x01u8, 0x02, 0x03];
        assert_eq!(prefill_from_slice(&data, 0), 0x0000_0000_0003_0201);
    }

    #[test]
    fn zeropad_exact_8_bytes() {
        let data = [0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        assert_eq!(prefill_from_slice(&data, 0), 0x0807_0605_0403_0201);
    }

    #[test]
    fn zeropad_with_nonzero_offset_partial() {
        // bytes_left = 2, should read slice[2] and slice[3]
        let data = [0xFFu8, 0xFF, 0xAB, 0xCD];
        assert_eq!(prefill_from_slice(&data, 2), 0x0000_0000_0000_CDAB);
    }

    #[test]
    fn zeropad_with_nonzero_offset_full_8_bytes() {
        // 16-byte slice; read 8 bytes starting at offset 4
        let data: Vec<u8> = (0u8..16).collect(); // [0,1,2,...,15]
        // Expected: LE u64 of bytes [4,5,6,7,8,9,10,11]
        let expected = u64::from_le_bytes([4, 5, 6, 7, 8, 9, 10, 11]);
        assert_eq!(prefill_from_slice(&data, 4), expected);
    }

    #[test]
    fn zeropad_offset_at_last_byte() {
        let data = [0x00u8, 0xBE];
        assert_eq!(prefill_from_slice(&data, 1), 0xBE);
    }

    #[test]
    fn read_zero_bits() {
        let data = [0xFFu8];
        let mut r = LittleEndianBitReader::new(&data);
        assert_eq!(r.read_bits_as_u64(0), 0);
    }

    #[test]
    fn read_single_bit() {
        // 0b0000_0001 → first bit is 1, second bit is 0
        let data = [0b0000_0001u8];
        let mut r = LittleEndianBitReader::new(&data);
        assert_eq!(r.read_bits_as_u64(1), 1);
        assert_eq!(r.read_bits_as_u64(1), 0);
    }

    #[test]
    fn read_full_byte_at_once() {
        let data = [0xA5u8]; // 0b1010_0101
        let mut r = LittleEndianBitReader::new(&data);
        assert_eq!(r.read_bits_as_u64(8), 0xA5);
    }

    #[test]
    fn read_nibbles() {
        let data = [0xABu8];
        let mut r = LittleEndianBitReader::new(&data);
        assert_eq!(r.read_bits_as_u64(4), 0xB); // low nibble first (little-endian bits)
        assert_eq!(r.read_bits_as_u64(4), 0xA);
    }

    #[test]
    fn read_exactly_64_bits_single_call() {
        let data: Vec<u8> = (1u8..=8).collect();
        let mut r = LittleEndianBitReader::new(&data);
        let expected = u64::from_le_bytes([1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(r.read_bits_as_u64(64), expected);
    }

    #[test]
    fn read_bits_crossing_byte_boundary() {
        // Two bytes: [0b1111_0001, 0b0000_1010]
        // As a 16-bit LE stream: bits 0-7 = 0xF1, bits 8-15 = 0x0A
        // Read 4 then 8 bits:
        //   first 4  → 0b0001 = 1
        //   next  8  → 0b1010_1111 = 0xAF  (spans the byte boundary)
        let data = [0b1111_0001u8, 0b0000_1010];
        let mut r = LittleEndianBitReader::new(&data);
        assert_eq!(r.read_bits_as_u64(4), 0x1);
        assert_eq!(r.read_bits_as_u64(8), 0xAF);
    }

    #[test]
    fn sequential_reads_reconstruct_original() {
        let data = [0x12u8, 0x34, 0x56, 0x78];
        let mut r = LittleEndianBitReader::new(&data);
        // Read byte-by-byte and reassemble
        let reconstructed = r.read_bits_as_u64(8)
            | (r.read_bits_as_u64(8) << 8)
            | (r.read_bits_as_u64(8) << 16)
            | (r.read_bits_as_u64(8) << 24);
        assert_eq!(reconstructed, 0x7856_3412);
    }

    #[test]
    fn read_more_than_64_bits_across_calls() {
        // 16 bytes: two 64-bit words
        let data: Vec<u8> = (0u8..16).collect();
        let mut r = LittleEndianBitReader::new(&data);
        let lo = r.read_bits_as_u64(64);
        let hi = r.read_bits_as_u64(64);
        assert_eq!(lo, u64::from_le_bytes([0, 1, 2, 3, 4, 5, 6, 7]));
        assert_eq!(hi, u64::from_le_bytes([8, 9, 10, 11, 12, 13, 14, 15]));
    }

    #[test]
    fn read_unaligned_64_bits() {
        // Read 1 bit to misalign, then 64 bits spanning two 8-byte windows.
        let data: Vec<u8> = vec![0xFFu8; 16];
        let mut r = LittleEndianBitReader::new(&data);
        r.read_bits_as_u64(1); // consume 1 bit (value = 1)
        let v = r.read_bits_as_u64(64); // all remaining bits are 1
        assert_eq!(v, u64::MAX);
    }

    #[test]
    fn read_unaligned_large() {
        let data: Vec<u8> = (0u8..64).collect();
        let mut r = LittleEndianBitReader::new(&data);
        assert_eq!(
            r.read_bits_as_u64(24),
            u64::from_le_bytes([0, 1, 2, 0, 0, 0, 0, 0])
        );
        assert_eq!(
            r.read_bits_as_u64(56),
            u64::from_le_bytes([3, 4, 5, 6, 7, 8, 9, 0])
        );
        assert_eq!(
            r.read_bits_as_u64(64),
            u64::from_le_bytes([10, 11, 12, 13, 14, 15, 16, 17])
        );
        assert_eq!(
            r.read_bits_as_u64(8),
            u64::from_le_bytes([18, 0, 0, 0, 0, 0, 0, 0])
        );
        assert_eq!(
            r.read_bits_as_u64(40),
            u64::from_le_bytes([19, 20, 21, 22, 23, 0, 0, 0])
        );
    }

    #[test]
    fn result_is_masked_to_requested_width() {
        // 0xFF byte; reading 3 bits should give 0b111 = 7, not 0xFF
        let data = [0xFFu8];
        let mut r = LittleEndianBitReader::new(&data);
        assert_eq!(r.read_bits_as_u64(3), 0b111);
    }

    #[test]
    fn read_past_end_returns_zeros() {
        let data = [0x01u8];
        let mut r = LittleEndianBitReader::new(&data);
        r.read_bits_as_u64(8); // consume the only byte
        assert_eq!(r.read_bits_as_u64(8), 0); // past end → 0
        assert_eq!(r.read_bits_as_u64(64), 0);
    }

    #[test]
    fn read_from_empty_slice() {
        let mut r = LittleEndianBitReader::new(&[]);
        assert_eq!(r.read_bits_as_u64(1), 0);
        assert_eq!(r.read_bits_as_u64(64), 0);
    }

    #[test]
    fn alternating_bits() {
        // 0b0101_0101 = 0x55 repeated
        let data = [0x55u8, 0x55];
        let mut r = LittleEndianBitReader::new(&data);
        for _ in 0..8 {
            assert_eq!(r.read_bits_as_u64(1), 1); // odd bits
            assert_eq!(r.read_bits_as_u64(1), 0); // even bits
        }
    }
}
