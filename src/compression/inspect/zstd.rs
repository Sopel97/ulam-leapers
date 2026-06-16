use std::error::Error;
use std::fmt::Display;
use crate::util::bitstream::LittleEndianBitReader;
use crate::util::bytestream::{ByteReader, ByteReaderError};

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum ZstdInspectError {
    ReservedBlockType,
    InvalidFrameMagic,
    UnexpectedEndOfStream,
    ReservedBitSetInHeaderDescriptor,
    TooManyHuffmanWeights,
    FseAccuracyLogTooHigh,
}

impl Display for ZstdInspectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ZstdInspectError::ReservedBlockType => {
                write!(f, "Reserved block type")
            }
            ZstdInspectError::InvalidFrameMagic => {
                write!(f, "Invalid frame magic")
            }
            ZstdInspectError::UnexpectedEndOfStream => {
                write!(f, "Unexpected end of stream")
            }
            ZstdInspectError::ReservedBitSetInHeaderDescriptor => {
                write!(f, "Reserved bit set in header descriptor")
            }
            ZstdInspectError::TooManyHuffmanWeights => {
                write!(f, "Too many Huffman weights")
            }
            ZstdInspectError::FseAccuracyLogTooHigh => {
                write!(f, "FSE accuracy log too big")
            }
        }
    }
}

impl Error for ZstdInspectError {}

impl From<ByteReaderError> for ZstdInspectError {
    fn from(err: ByteReaderError) -> Self {
        match err {
            ByteReaderError::UnexpectedEndOfStream => ZstdInspectError::UnexpectedEndOfStream,
        }
    }
}

const ZSTD_FRAME_MAGIC: u32 = 0xFD2FB528;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ZstdFrameHeaderDescriptor {
    byte: u8,
}

impl ZstdFrameHeaderDescriptor {
    pub fn from_byte(byte: u8) -> Result<Self, ZstdInspectError> {
        let reserved_bit = (byte >> 3) & 0b1 != 0;
        if reserved_bit {
            return Err(ZstdInspectError::ReservedBitSetInHeaderDescriptor);
        }

        Ok(ZstdFrameHeaderDescriptor { byte })
    }

    pub fn frame_content_size_flag(&self) -> u8 {
        (self.byte >> 6) & 0b11
    }

    pub fn single_segment_flag(&self) -> bool {
        (self.byte >> 5) & 0b1 != 0
    }

    pub fn content_checksum_flag(&self) -> bool {
        (self.byte >> 2) & 0b1 != 0
    }

    pub fn dictionary_id_flag(&self) -> u8 {
        self.byte & 0b11
    }

    pub fn did_field_size(&self) -> usize {
        match self.dictionary_id_flag() {
            0 => 0,
            1 => 1,
            2 => 2,
            3 => 4,
            _ => unreachable!(),
        }
    }

    pub fn fcs_field_size(&self) -> usize {
        match self.frame_content_size_flag() {
            0 => match self.single_segment_flag() {
                false => 0,
                true => 1,
            },
            1 => 2,
            2 => 4,
            3 => 8,
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub struct ZstdBlockHeader {
    last_block: bool,
    block_type: u8,
    block_size: u32,
}

impl ZstdBlockHeader {
    pub fn read_from(reader: &mut ByteReader) -> Result<Self, ZstdInspectError> {
        let data = reader.try_read_bytes_as_u64(3)?;

        let last_block = data & 1 == 1;
        let block_type = ((data >> 1) & 0b11) as u8;
        let block_size = (data >> 3) as u32;

        // TODO: verify against `Block_Maximum_Size`?

        // Check for reserved block type
        if block_type == 3 {
            return Err(ZstdInspectError::ReservedBlockType);
        }

        Ok(Self {
            last_block,
            block_type,
            block_size,
        })
    }

    pub fn read_block<'a>(
        &self,
        reader: &mut ByteReader<'a>,
    ) -> Result<ZstdDataBlock<'a>, ZstdInspectError> {
        Ok(match self.block_type {
            0 => ZstdDataBlock::Raw(reader.try_read_slice(self.block_size as usize)?),
            1 => ZstdDataBlock::Rle(reader.try_read_u8()?, self.block_size as usize),
            2 => ZstdDataBlock::Compressed(reader.try_read_slice(self.block_size as usize)?),
            _ => unreachable!(),
        })
    }
}

#[derive(Debug)]
pub enum ZstdDataBlock<'a> {
    Raw(&'a [u8]),
    Rle(u8, usize),
    Compressed(&'a [u8]),
}

#[derive(Debug)]
pub struct ZstdDataBlockIter<'a> {
    stream: ByteReader<'a>,
    curr_frame_header_descriptor: Option<ZstdFrameHeaderDescriptor>,
}

impl<'a> ZstdDataBlockIter<'a> {
    fn new(stream: &'a [u8]) -> Self {
        Self {
            stream: ByteReader::new(stream),
            curr_frame_header_descriptor: None,
        }
    }
}

impl<'a> Iterator for ZstdDataBlockIter<'a> {
    type Item = Result<ZstdDataBlock<'a>, ZstdInspectError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.stream.is_eof() {
            return if self.stream.cursor() == 0 {
                Some(Err(ZstdInspectError::UnexpectedEndOfStream))
            } else {
                None
            };
        }

        Some(self.next_not_eof())
    }
}

impl<'a> ZstdDataBlockIter<'a> {
    fn parse_frame_header(
        stream: &mut ByteReader<'_>,
    ) -> Result<ZstdFrameHeaderDescriptor, ZstdInspectError> {
        let magic = stream.try_read_u32()?;
        if magic != ZSTD_FRAME_MAGIC {
            return Err(ZstdInspectError::InvalidFrameMagic);
        }

        let frame_header_descriptor = ZstdFrameHeaderDescriptor::from_byte(stream.try_read_u8()?)?;

        if !frame_header_descriptor.single_segment_flag() {
            // Skip the window_descriptor field.
            stream.try_skip(1)?;
        }

        // Skip the dictionary_id field.
        stream.try_skip(frame_header_descriptor.did_field_size())?;

        // Skip the frame_content_size field.
        stream.try_skip(frame_header_descriptor.fcs_field_size())?;

        Ok(frame_header_descriptor)
    }

    // Allows us to use the ? operator for error handling because we can no longer
    // return `None`. Any EOF here is due to a corrupted stream.
    fn next_not_eof(&mut self) -> Result<ZstdDataBlock<'a>, ZstdInspectError> {
        if self.curr_frame_header_descriptor.is_none() {
            self.curr_frame_header_descriptor = Some(Self::parse_frame_header(&mut self.stream)?);
        }

        self.next_block_in_frame()
    }

    fn next_block_in_frame(&mut self) -> Result<ZstdDataBlock<'a>, ZstdInspectError> {
        let frame_header_descriptor = self
            .curr_frame_header_descriptor
            .as_ref()
            .expect("`next_not_eof` should have prepared it if it wasn't present");

        let block_header = ZstdBlockHeader::read_from(&mut self.stream)?;

        let res = block_header.read_block(&mut self.stream);

        // Last block in each frame should be marked.
        if block_header.last_block {
            if frame_header_descriptor.content_checksum_flag() {
                // Skip the 4 byte checksum if present.
                self.stream.try_skip(4)?;
            }
            self.curr_frame_header_descriptor = None;
        }

        res
    }
}

impl<'a> ZstdDataBlock<'a> {
    pub fn max_byte(&self) -> Result<u8, ZstdInspectError> {
        match self {
            ZstdDataBlock::Raw(data) => Ok(*data.iter().max().unwrap_or(&0u8)),
            ZstdDataBlock::Rle(literal, _count) => Ok(*literal),
            ZstdDataBlock::Compressed(data) => Self::max_byte_in_compressed_block(data),
        }
    }

    /// `data` must be `ZstdDataBlock::Compressed`
    fn max_byte_in_compressed_block(data: &[u8]) -> Result<u8, ZstdInspectError> {
        let mut stream = LittleEndianBitReader::new(data);

        let literals_block_type = stream.read_bits_as_u64(2);
        let size_format = stream.read_bits_as_u64(2);

        match literals_block_type {
            0 | 1 => {
                // `Raw_Literals_Block` or `RLE_Literals_Block`
                Self::max_byte_in_simple_literals(stream, literals_block_type, size_format)
            }
            2 => {
                // `Compressed_Literals_Block`
                Self::max_byte_in_compressed_literals(stream, size_format)
            }
            3 => {
                // ``Treeless_Literals_Block`, reuses the previous Huffman table,
                // so we don't need to check anything - it was checked in the previous block.
                Ok(0u8)
            }
            _ => unreachable!(),
        }
    }

    /// Must be either `Raw_Literals_Block` or `RLE_Literals_Block`
    fn max_byte_in_simple_literals(
        mut stream: LittleEndianBitReader,
        literals_block_type: u64,
        size_format: u64,
    ) -> Result<u8, ZstdInspectError> {
        let regenerated_size = match size_format {
            0 | 2 => {
                // `size_format` uses one bit.
                // We need to transfer one bit from `size_format` to `regenerated_size`.
                // Since `size_format` was read earlier it stole one least significant bit
                // of `regenerated size`. `size_format` is at most 2 bits so no need to & with 1.
                let regenerated_size_high = stream.read_bits_as_u64(4);
                (regenerated_size_high << 1) | (size_format >> 1)
            }
            1 => stream.read_bits_as_u64(12),
            3 => stream.read_bits_as_u64(20),
            _ => unreachable!(),
        };

        // Now we convert back to a byte reader to grab the uncompressed u8 literals.
        let mut stream = stream
            .try_into_byte_reader()
            .expect("We should have read 8, 16, or 24 bits");

        match literals_block_type {
            0 => {
                // `Raw_Literals_Block`
                let literals = stream.try_read_slice(regenerated_size as usize)?;
                Ok(*literals.iter().max().unwrap_or(&0u8))
            }
            1 => {
                // `RLE_Literals_Block` - A single literal.
                Ok(stream.try_read_u8()?)
            }
            _ => unreachable!(),
        }
    }

    /// Must be `Compressed_Literals_Block`
    fn max_byte_in_compressed_literals(
        mut stream: LittleEndianBitReader,
        size_format: u64,
    ) -> Result<u8, ZstdInspectError> {
        // We don't actually need `regenerated_size`, nor `compressed_size`, nor `num_streams`.
        // Just read as many bits as it they take to arrive at the weights.
        stream.read_bits_as_u64(match size_format {
            0 | 1 => 10 + 10,
            2 => 14 + 14,
            3 => 18 + 18,
            _ => unreachable!(),
        });

        // Decode the Huffman table.

        let huffman_tree_header = stream.read_bits_as_u64(8);

        if huffman_tree_header >= 128 {
            // Direct representation.
            // The weights are provided for each symbol sequentially, with the last
            // symbol having an implicit non-zero weight (i.e. being guaranteed existing).
            // Because of this we don't have to parse the weights checking them for zero.
            let number_of_weights = huffman_tree_header - 127;
            if number_of_weights + 1 > 256 {
                Err(ZstdInspectError::TooManyHuffmanWeights)
            } else {
                Ok(number_of_weights as u8)
            }
        } else {
            // `huffman_tree_header` contains the compressed length but we don't care.

            assert!(stream.is_byte_aligned());

            const FSE_MAX_ACCURACY_LOG: u64 = 7;

            let accuracy_log = 5 + stream.read_bits_as_u64(4);
            if accuracy_log > FSE_MAX_ACCURACY_LOG {
                return Err(ZstdInspectError::FseAccuracyLogTooHigh);
            }

            let mut remaining = 1i32 << accuracy_log;
            let mut max_byte = 0u8;
            let mut curr_symbol = 0;
            while remaining > 0 && curr_symbol < 256 {
                // `log2sup(N)`, i.e. smallest integer `T` that satisfies `(1 << T) > N`
                // The decoder may read up to `remaining + 1` inclusive.
                let bits = u32::BITS - (remaining as u32 + 1).leading_zeros();
                assert!(bits > 0);

                // Whether we need the lowest bit depends on the value.
                // From the docs:
                // > Value decoded: small values use 1 less bit: example:
                // > Presuming values from 0 to 157 (inclusive) are possible,
                // > 255-157 = 98 values are remaining in an 8-bits field.
                // > They are used this way: first 98 values (hence from 0 to 97)
                // > use only 7 bits, values from 98 to 157 use 8 bits.
                let value_low = stream.read_bits_as_u64((bits - 1) as usize) as u32;

                let threshold = (1 << bits) - 1 - (remaining as u32 + 1);

                let value: u32 = if value_low >= threshold {
                    // Read one more bit and place it at the highest bit position.
                    let highest_bit = stream.read_bits_as_u64(1) as u32;
                    value_low | highest_bit << (bits - 1)
                } else {
                    value_low
                };

                // Maybe negative. Value `-1` has some special meaning during table building.
                let probability = value as i32 - 1;

                // Update the max_byte if the probability is different from 0.
                // In particular, note that the special `-1` probability, signifying
                // a full state reset, is also a valid output symbol.
                if probability != 0 {
                    // We are iterating symbols in ascending order
                    // so no need to check if it's greater.
                    max_byte = curr_symbol as u8;
                }

                remaining -= probability.abs();
                curr_symbol += 1;

                // From the docs:
                // > When a symbol has a probability of zero (decoded from reading a Value 1),
                // > it is followed by a 2-bits repeat flag. This repeat flag tells how many
                // > probabilities of zeroes follow the current one. It provides a number ranging
                // > from 0 to 3. If it is a 3, another 2-bits repeat flag follows, and so on.
                if probability == 0 {
                    // Even if the input is malformed we will exit this loop eventually
                    // because bit reader will read zeros outside the data slice, and 0 < 3.
                    loop {
                        let repeat = stream.read_bits_as_u64(2);

                        curr_symbol += repeat;

                        if repeat < 3 {
                            break;
                        }
                    }
                }
            }

            Ok(max_byte)
        }
    }
}

pub fn max_byte_in_zstd_stream(data: &[u8]) -> Result<u8, ZstdInspectError> {
    let mut max_byte = 0u8;
    for block in ZstdDataBlockIter::new(data) {
        match block {
            Err(err) => return Err(err),
            Ok(block) => max_byte = max_byte.max(block.max_byte()?),
        };
    }
    Ok(max_byte)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: compress bytes with default zstd settings.
    fn zstd_compress(data: &[u8]) -> Vec<u8> {
        zstd::encode_all(data, 0).unwrap()
    }

    // Helper: compress with a specific compression level.
    fn zstd_compress_level(data: &[u8], level: i32) -> Vec<u8> {
        zstd::encode_all(data, level).unwrap()
    }

    #[test]
    fn test_empty_stream() {
        // A valid empty zstd frame (last block is empty raw block).
        let compressed = zstd_compress(b"");
        // Should succeed and return 0 (no bytes present).
        assert_eq!(max_byte_in_zstd_stream(&compressed).unwrap(), 0);
    }

    #[test]
    fn test_single_byte_value() {
        for byte in [0u8, 1, 127, 128, 200, 254, 255] {
            let input = vec![byte; 1];
            let compressed = zstd_compress(&input);
            assert_eq!(
                max_byte_in_zstd_stream(&compressed).unwrap(),
                byte,
                "failed for byte value {byte}"
            );
        }
    }

    #[test]
    fn test_max_is_255() {
        // Ensure 255 is detected when present.
        let mut input = vec![0u8; 1000];
        input.push(255);
        let compressed = zstd_compress(&input);
        assert_eq!(max_byte_in_zstd_stream(&compressed).unwrap(), 255);
    }

    #[test]
    fn test_max_is_not_overstated() {
        // Max byte is 100; 101..=255 must not appear.
        let input: Vec<u8> = (0..=100u8).cycle().take(4096).collect();
        let compressed = zstd_compress(&input);
        assert_eq!(max_byte_in_zstd_stream(&compressed).unwrap(), 100);
    }

    #[test]
    fn test_all_zeros() {
        // All-zero input — likely becomes an RLE block.
        let input = vec![0u8; 65536];
        let compressed = zstd_compress(&input);
        assert_eq!(max_byte_in_zstd_stream(&compressed).unwrap(), 0);
    }

    #[test]
    fn test_all_same_nonzero_byte() {
        // RLE block with a nonzero byte.
        let input = vec![42u8; 65536];
        let compressed = zstd_compress(&input);
        assert_eq!(max_byte_in_zstd_stream(&compressed).unwrap(), 42);
    }

    #[test]
    fn test_raw_block() {
        // Very small inputs or incompressible data should produce a raw block.
        // Use level 1 and short length to encourage it.
        let input = b"\x01\x02\x03";
        let compressed = zstd_compress_level(input, 1);
        assert_eq!(max_byte_in_zstd_stream(&compressed).unwrap(), 3);
    }

    #[test]
    fn test_compressed_literals_huffman() {
        // Large, compressible, diverse input to force Huffman-compressed literals.
        let input: Vec<u8> = (0u8..=127).cycle().take(65536).collect();
        let compressed = zstd_compress_level(&input, 3);
        assert_eq!(max_byte_in_zstd_stream(&compressed).unwrap(), 127);
    }

    #[test]
    fn test_full_byte_range() {
        // All 256 possible byte values present - max must be 255.
        let input: Vec<u8> = (0u8..=255).cycle().take(65536).collect();
        let compressed = zstd_compress(&input);
        assert_eq!(max_byte_in_zstd_stream(&compressed).unwrap(), 255);
    }

    #[test]
    fn test_multi_block_frame() {
        // Force multiple blocks by using a large input.
        // Max byte appears only in the last block.
        let mut input = vec![0u8; 128 * 1024];
        *input.last_mut().unwrap() = 200;
        let compressed = zstd_compress(&input);
        assert_eq!(max_byte_in_zstd_stream(&compressed).unwrap(), 200);
    }

    #[test]
    fn test_multi_frame() {
        // Concatenate two independently compressed frames.
        let frame1 = zstd_compress(&vec![10u8; 1024]);
        let frame2 = zstd_compress(&vec![200u8; 1024]);
        let combined = [frame1, frame2].concat();
        assert_eq!(max_byte_in_zstd_stream(&combined).unwrap(), 200);
    }

    #[test]
    fn test_multi_frame_max_in_first() {
        // Max byte is in the first frame, not the second.
        let frame1 = zstd_compress(&vec![200u8; 1024]);
        let frame2 = zstd_compress(&vec![10u8; 1024]);
        let combined = [frame1, frame2].concat();
        assert_eq!(max_byte_in_zstd_stream(&combined).unwrap(), 200);
    }

    #[test]
    fn test_frame_with_checksum() {
        // Encode with checksum enabled (requires encoder config).
        use std::io::Write;
        let mut encoder = zstd::Encoder::new(Vec::new(), 3).unwrap();
        encoder.include_checksum(true).unwrap();
        encoder.write_all(&vec![77u8; 4096]).unwrap();
        let compressed = encoder.finish().unwrap();
        // If we misparse the checksum, the next frame (or EOF) will fail.
        assert_eq!(max_byte_in_zstd_stream(&compressed).unwrap(), 77);
    }

    #[test]
    fn test_multi_frame_with_checksum() {
        use std::io::Write;
        let make_frame = |byte: u8| {
            let mut enc = zstd::Encoder::new(Vec::new(), 3).unwrap();
            enc.include_checksum(true).unwrap();
            enc.write_all(&vec![byte; 4096]).unwrap();
            enc.finish().unwrap()
        };
        let combined = [make_frame(50), make_frame(150)].concat();
        assert_eq!(max_byte_in_zstd_stream(&combined).unwrap(), 150);
    }

    #[test]
    fn test_treeless_block_doesnt_inflate_max() {
        // A stream with multiple compressed blocks will likely produce treeless
        // blocks after the first. The max should still be correct and not
        // be inflated by the treeless block returning 0.
        let input: Vec<u8> = (0u8..=50).cycle().take(256 * 1024).collect();
        let compressed = zstd_compress(&input);
        assert_eq!(max_byte_in_zstd_stream(&compressed).unwrap(), 50);
    }

    #[test]
    fn test_invalid_magic() {
        let data = b"\x00\x01\x02\x03\x04\x05\x06\x07";
        assert_eq!(
            max_byte_in_zstd_stream(data).unwrap_err(),
            ZstdInspectError::InvalidFrameMagic
        );
    }

    #[test]
    fn test_truncated_stream() {
        let compressed = zstd_compress(&vec![42u8; 1024]);
        // Lop off the last quarter of the stream.
        let truncated = &compressed[..compressed.len() * 3 / 4];
        assert_eq!(
            max_byte_in_zstd_stream(truncated).unwrap_err(),
            ZstdInspectError::UnexpectedEndOfStream
        );
    }

    #[test]
    fn test_empty_input() {
        // Zero bytes — not even a frame header.
        assert_eq!(
            max_byte_in_zstd_stream(b"").unwrap_err(),
            ZstdInspectError::UnexpectedEndOfStream
        );
    }
}
