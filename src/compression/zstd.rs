use crate::compression::{Compression, CompressionError, CompressionKind};
use std::io::{Read, Write};
use zstd::{Decoder, Encoder};

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub struct ZstdCompression {
    level: i32,
}

impl Default for ZstdCompression {
    fn default() -> Self {
        Self::new()
    }
}

impl ZstdCompression {
    pub fn new() -> Self {
        Self { level: 3 }
    }

    pub fn new_with_level(level: i32) -> Self {
        Self { level }
    }

    pub fn set_encoder_level(&mut self, level: i32) -> Result<(), CompressionError> {
        if !(1..=22).contains(&level) {
            return Err(CompressionError::InvalidCompressionLevel);
        }
        self.level = level;
        Ok(())
    }
}

impl Compression for ZstdCompression {
    fn kind(&self) -> CompressionKind {
        CompressionKind::Zstd
    }

    fn compress(&self, mut input: impl Read, output: &mut impl Write) -> std::io::Result<()> {
        // We can't use copy_encode because it doesn't return the number of bytes read.
        let mut encoder = Encoder::new(output, self.level)?;
        std::io::copy(&mut input, &mut encoder)?;
        // IMPORTANT: Zstd requires `finish` to be called.
        encoder.finish()?;
        Ok(())
    }

    fn compress_to_buffer(&self, input: &[u8], output: &mut [u8]) -> std::io::Result<usize> {
        zstd::bulk::compress_to_buffer(input, output, self.level)
    }

    fn decompress(&self, input: impl Read, output: &mut impl Write) -> std::io::Result<()> {
        // We can't use copy_decode because it doesn't return the number of bytes read.
        let mut decoder = Decoder::new(input)?;
        std::io::copy(&mut decoder, output).map(|_| ())
    }

    fn decompress_to_buffer(&self, input: &[u8], output: &mut [u8]) -> std::io::Result<usize> {
        zstd::bulk::decompress_to_buffer(input, output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn roundtrip(c: &impl Compression, input: &[u8]) -> Vec<u8> {
        let mut compressed = Vec::new();
        c.compress(Cursor::new(input), &mut compressed)
            .expect("compress failed");
        c.decompress_to_vec(Cursor::new(&compressed))
            .expect("decompress failed")
    }

    const HELLO: &[u8] = b"helllllo, world";
    const EMPTY: &[u8] = b"";
    const COMPRESSIBLE: &[u8] = &[0xAB; 1024];

    #[test]
    fn zstd_roundtrip_default_level() {
        assert_eq!(roundtrip(&ZstdCompression::new(), HELLO), HELLO);
    }

    #[test]
    fn zstd_roundtrip_empty() {
        assert_eq!(roundtrip(&ZstdCompression::new(), EMPTY), EMPTY);
    }

    #[test]
    fn zstd_roundtrip_compressible_data() {
        let result = roundtrip(&ZstdCompression::new(), COMPRESSIBLE);
        assert_eq!(result, COMPRESSIBLE);
    }

    #[test]
    fn zstd_actually_compresses() {
        let mut compressed = Vec::new();
        ZstdCompression::new()
            .compress(Cursor::new(COMPRESSIBLE), &mut compressed)
            .unwrap();
        assert!(
            compressed.len() < COMPRESSIBLE.len(),
            "expected compression; got {} >= {} bytes",
            compressed.len(),
            COMPRESSIBLE.len()
        );
    }

    #[test]
    fn zstd_all_valid_levels_roundtrip() {
        for level in 1..=22 {
            let c = ZstdCompression::new_with_level(level);
            assert_eq!(roundtrip(&c, HELLO), HELLO, "failed at level {level}");
        }
    }

    #[test]
    fn zstd_compress_to_buffer_roundtrip() {
        let c = ZstdCompression::new();
        let mut compressed = vec![0u8; HELLO.len() + 128];
        let n = c
            .compress_to_buffer(HELLO, &mut compressed)
            .expect("compress_to_buffer failed");

        let mut decompressed = vec![0u8; HELLO.len() + 16];
        let m = c
            .decompress_to_buffer(&compressed[..n], &mut decompressed)
            .expect("decompress_to_buffer failed");

        assert_eq!(&decompressed[..m], HELLO);
    }

    #[test]
    fn zstd_decompress_invalid_data_errors() {
        let junk = b"this is not valid zstd data";
        let result = ZstdCompression::new().decompress_to_vec(Cursor::new(junk));
        assert!(result.is_err());
    }

    #[test]
    fn zstd_kind() {
        assert!(matches!(
            ZstdCompression::new().kind(),
            CompressionKind::Zstd
        ));
    }

    #[test]
    fn set_encoder_level_accepts_valid_range() {
        let mut c = ZstdCompression::new();
        assert!(c.set_encoder_level(1).is_ok());
        assert!(c.set_encoder_level(22).is_ok());
        assert!(c.set_encoder_level(10).is_ok());
    }

    #[test]
    fn set_encoder_level_rejects_zero() {
        let mut c = ZstdCompression::new();
        assert!(matches!(
            c.set_encoder_level(0),
            Err(CompressionError::InvalidCompressionLevel)
        ));
    }

    #[test]
    fn set_encoder_level_rejects_23() {
        let mut c = ZstdCompression::new();
        assert!(matches!(
            c.set_encoder_level(23),
            Err(CompressionError::InvalidCompressionLevel)
        ));
    }

    #[test]
    fn set_encoder_level_rejects_negative() {
        let mut c = ZstdCompression::new();
        assert!(matches!(
            c.set_encoder_level(-1),
            Err(CompressionError::InvalidCompressionLevel)
        ));
    }
}
