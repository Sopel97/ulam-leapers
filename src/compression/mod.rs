pub mod inspect;
pub mod zstd;

use crate::compression::zstd::ZstdCompression;
use crate::game::persist::uls::UlsCompressionKind;
use std::fmt;
use std::io::{Read, Write};

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum CompressionKind {
    Zstd,
    None,
}

#[derive(Debug)]
pub struct CompressedBlob {
    kind: CompressionKind,
    data: Box<[u8]>,
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum CompressionError {
    InvalidCompressionLevel,
}

impl fmt::Display for CompressionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CompressionError::InvalidCompressionLevel => {
                write!(f, "invalid compression level")
            }
        }
    }
}

impl From<UlsCompressionKind> for CompressionKind {
    fn from(kind: UlsCompressionKind) -> Self {
        match kind {
            UlsCompressionKind::None => CompressionKind::None,
            UlsCompressionKind::Zstd => CompressionKind::Zstd,
        }
    }
}

pub trait Compression {
    fn kind(&self) -> CompressionKind;

    /// Succeeds only if the whole `input` was consumed and there was
    /// enough space in the `output` to write the result.
    fn compress(&self, input: impl Read, output: &mut impl Write) -> std::io::Result<()>;

    /// Succeeds only if the whole `input` was consumed and there was enough
    /// memory available to form the result.
    /// On success returns the decompressed bytes as a `CompressedBlob` - tagged `Box<[u8]>`
    fn compress_to_blob(&self, input: impl Read) -> std::io::Result<CompressedBlob> {
        let mut buf = Vec::new();
        self.compress(input, &mut buf)?;
        Ok(CompressedBlob {
            kind: self.kind(),
            data: buf.into_boxed_slice(),
        })
    }

    /// On success the number of bytes written to `output` is returned.
    fn compress_to_buffer(&self, input: &[u8], output: &mut [u8]) -> std::io::Result<usize> {
        let mut writer = std::io::Cursor::new(output);
        self.compress(input, &mut writer)?;
        Ok(writer.position() as usize)
    }

    /// Succeeds only if the whole `input` was consumed and there was
    /// enough space in the `output` to write the result.
    fn decompress(&self, input: impl Read, output: &mut impl Write) -> std::io::Result<()>;

    /// Succeeds only if the whole `input` was consumed and there was enough
    /// memory available to form the result.
    /// On success returns the decompressed bytes as a `Vec<u8>`.
    /// The capacity of the returned `Vec<u8>` may be higher than necessary.
    fn decompress_to_vec(&self, input: impl Read) -> std::io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        self.decompress(input, &mut buf)?;
        Ok(buf)
    }

    /// On success the number of bytes written to `output` is returned.
    fn decompress_to_buffer(&self, input: &[u8], output: &mut [u8]) -> std::io::Result<usize> {
        let mut writer = std::io::Cursor::new(output);
        self.decompress(input, &mut writer)?;
        Ok(writer.position() as usize)
    }
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum AnyCompression {
    Zstd(ZstdCompression),
    None(NoneCompression),
}

macro_rules! dispatch_any_compression {
    ($self:expr, $var:ident => $body:expr) => {
        match $self {
            AnyCompression::Zstd($var) => $body,
            AnyCompression::None($var) => $body,
        }
    };
}

impl Compression for AnyCompression {
    fn kind(&self) -> CompressionKind {
        dispatch_any_compression!(self, c => {
            c.kind()
        })
    }

    fn compress(&self, input: impl Read, output: &mut impl Write) -> std::io::Result<()> {
        dispatch_any_compression!(self, c => {
            c.compress(input, output)
        })
    }

    fn decompress(&self, input: impl Read, output: &mut impl Write) -> std::io::Result<()> {
        dispatch_any_compression!(self, c => {
            c.decompress(input, output)
        })
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub struct NoneCompression;

impl Default for NoneCompression {
    fn default() -> Self {
        Self::new()
    }
}

impl NoneCompression {
    pub fn new() -> Self {
        Self {}
    }
}

impl From<NoneCompression> for AnyCompression {
    fn from(c: NoneCompression) -> Self {
        AnyCompression::None(c)
    }
}

impl From<ZstdCompression> for AnyCompression {
    fn from(c: ZstdCompression) -> Self {
        AnyCompression::Zstd(c)
    }
}

macro_rules! dispatch_compression_kind {
    ($self:expr, $method:ident ( $($args:expr),* )) => {
        match $self.kind {
            CompressionKind::Zstd => ZstdCompression::new().$method($($args),*),
            CompressionKind::None => NoneCompression::new().$method($($args),*),
        }
    };
}

impl CompressedBlob {
    /// Succeeds only if the whole `input` was consumed and there was
    /// enough space in the `output` to write the result.
    pub fn decompress(&mut self, output: &mut impl Write) -> std::io::Result<()> {
        dispatch_compression_kind!(self, decompress(self.data.as_ref(), output))
    }

    /// Succeeds only if the whole `input` was consumed and there was enough
    /// memory available to form the result.
    /// On success returns the decompressed bytes as a `Vec<u8>`.
    /// The capacity of the returned `Vec<u8>` may be higher than necessary.
    pub fn decompress_to_vec(&mut self) -> std::io::Result<Vec<u8>> {
        dispatch_compression_kind!(self, decompress_to_vec(self.data.as_ref()))
    }

    /// On success the number of bytes written to `output` is returned.
    pub fn decompress_to_buffer(&self, output: &mut [u8]) -> std::io::Result<usize> {
        dispatch_compression_kind!(self, decompress_to_buffer(self.data.as_ref(), output))
    }

    /// Constructs a new CompressedBlob with given `kind` and `data` bytes.
    /// While this may result in an object that causes an error on decompression
    /// the decompression process is already fallible.
    pub fn from_raw_parts(kind: CompressionKind, data: Box<[u8]>) -> Self {
        Self { kind, data }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn bytes(&self) -> &[u8] {
        self.data.as_ref()
    }

    pub fn compression_kind(&self) -> CompressionKind {
        self.kind
    }
}

impl Compression for NoneCompression {
    fn kind(&self) -> CompressionKind {
        CompressionKind::None
    }

    fn compress(&self, mut input: impl Read, output: &mut impl Write) -> std::io::Result<()> {
        std::io::copy(&mut input, output).map(|_| ())
    }

    fn decompress(&self, mut input: impl Read, output: &mut impl Write) -> std::io::Result<()> {
        std::io::copy(&mut input, output).map(|_| ())
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

    #[test]
    fn none_compress_is_identity() {
        assert_eq!(roundtrip(&NoneCompression::new(), HELLO), HELLO);
    }

    #[test]
    fn none_compress_empty() {
        assert_eq!(roundtrip(&NoneCompression::new(), EMPTY), EMPTY);
    }

    #[test]
    fn none_compress_to_buffer_returns_correct_len() {
        let mut out = vec![0u8; HELLO.len() + 16];
        let n = NoneCompression::new()
            .compress_to_buffer(HELLO, &mut out)
            .expect("compress_to_buffer failed");
        assert_eq!(n, HELLO.len());
        assert_eq!(&out[..n], HELLO);
    }

    #[test]
    fn none_compress_to_buffer_output_too_small_errors() {
        let mut out = vec![0u8; 4]; // smaller than HELLO
        let result = NoneCompression::new().compress_to_buffer(HELLO, &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn none_decompress_to_buffer_returns_correct_len() {
        let mut out = vec![0u8; HELLO.len() + 16];
        let n = NoneCompression::new()
            .decompress_to_buffer(HELLO, &mut out)
            .expect("decompress_to_buffer failed");
        assert_eq!(n, HELLO.len());
        assert_eq!(&out[..n], HELLO);
    }

    #[test]
    fn none_kind() {
        assert!(matches!(
            NoneCompression::new().kind(),
            CompressionKind::None
        ));
    }

    #[test]
    fn none_compress_to_blob_roundtrip() {
        let mut blob = NoneCompression::new()
            .compress_to_blob(Cursor::new(HELLO))
            .expect("compress_to_blob failed");
        assert!(matches!(blob.kind, CompressionKind::None));
        assert_eq!(blob.bytes(), HELLO);
        let decompressed = blob.decompress_to_vec().expect("blob decompress failed");
        assert_eq!(decompressed, HELLO);
    }

    #[test]
    fn blob_len_and_bytes() {
        let blob = NoneCompression::new()
            .compress_to_blob(Cursor::new(HELLO))
            .unwrap();
        assert_eq!(blob.len(), HELLO.len());
        assert_eq!(blob.bytes(), HELLO);
    }

    #[test]
    fn blob_from_raw_parts_roundtrip_none() {
        let blob = CompressedBlob::from_raw_parts(
            CompressionKind::None,
            HELLO.to_vec().into_boxed_slice(),
        );
        assert_eq!(blob.bytes(), HELLO);
        let mut out = Vec::new();
        // NOTE: decompress takes &mut self even though it need not — call it directly.
        let mut blob = blob;
        blob.decompress(&mut out).unwrap();
        assert_eq!(out, HELLO);
    }

    #[test]
    fn blob_from_raw_parts_roundtrip_zstd() {
        let c = ZstdCompression::new();
        let mut compressed = Vec::new();
        c.compress(Cursor::new(HELLO), &mut compressed).unwrap();

        let mut blob =
            CompressedBlob::from_raw_parts(CompressionKind::Zstd, compressed.into_boxed_slice());
        assert_eq!(blob.decompress_to_vec().unwrap(), HELLO);
    }

    #[test]
    fn blob_decompress_to_buffer() {
        let blob = ZstdCompression::new()
            .compress_to_blob(Cursor::new(HELLO))
            .unwrap();
        let mut out = vec![0u8; HELLO.len() + 16];
        let n = blob.decompress_to_buffer(&mut out).unwrap();
        assert_eq!(&out[..n], HELLO);
    }

    #[test]
    fn any_compression_zstd_roundtrip() {
        let any = AnyCompression::from(ZstdCompression::new());
        assert_eq!(roundtrip(&any, HELLO), HELLO);
    }

    #[test]
    fn any_compression_none_roundtrip() {
        let any = AnyCompression::from(NoneCompression::new());
        assert_eq!(roundtrip(&any, HELLO), HELLO);
    }

    #[test]
    fn any_compression_kind_matches_variant() {
        assert!(matches!(
            AnyCompression::from(ZstdCompression::new()).kind(),
            CompressionKind::Zstd
        ));
        assert!(matches!(
            AnyCompression::from(NoneCompression::new()).kind(),
            CompressionKind::None
        ));
    }

    #[test]
    fn compression_error_display() {
        let msg = format!("{}", CompressionError::InvalidCompressionLevel);
        assert_eq!(msg, "invalid compression level");
    }

    #[test]
    fn compression_error_debug() {
        // Just assert it doesn't panic and produces something non-empty.
        let s = format!("{:?}", CompressionError::InvalidCompressionLevel);
        assert!(!s.is_empty());
    }
}
