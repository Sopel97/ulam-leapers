use std::fmt;
use std::io::{Read, Write};
use zstd::{Decoder, Encoder};

pub enum CompressionKind {
    Zstd,
    None,
}

pub struct CompressedBlob {
    kind: CompressionKind,
    data: Box<[u8]>,
}

#[derive(Debug, Clone)]
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
        Ok(())
    }
}

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

impl From<ZstdCompression> for AnyCompression {
    fn from(c: ZstdCompression) -> Self {
        AnyCompression::Zstd(c)
    }
}

impl From<NoneCompression> for AnyCompression {
    fn from(c: NoneCompression) -> Self {
        AnyCompression::None(c)
    }
}

impl CompressedBlob {
    /// Succeeds only if the whole `input` was consumed and there was
    /// enough space in the `output` to write the result.
    pub fn decompress(&mut self, output: &mut impl Write) -> std::io::Result<()> {
        match self.kind {
            CompressionKind::Zstd => ZstdCompression::new().decompress(self.data.as_ref(), output),
            CompressionKind::None => NoneCompression::new().decompress(self.data.as_ref(), output),
        }
    }

    /// Succeeds only if the whole `input` was consumed and there was enough
    /// memory available to form the result.
    /// On success returns the decompressed bytes as a `Vec<u8>`.
    /// The capacity of the returned `Vec<u8>` may be higher than necessary.
    pub fn decompress_to_vec(&mut self) -> std::io::Result<Vec<u8>> {
        match self.kind {
            CompressionKind::Zstd => ZstdCompression::new().decompress_to_vec(self.data.as_ref()),
            CompressionKind::None => NoneCompression::new().decompress_to_vec(self.data.as_ref()),
        }
    }

    /// On success the number of bytes written to `output` is returned.
    pub fn decompress_to_buffer(&self, output: &mut [u8]) -> std::io::Result<usize> {
        match self.kind {
            CompressionKind::Zstd => {
                ZstdCompression::new().decompress_to_buffer(self.data.as_ref(), output)
            }
            CompressionKind::None => {
                NoneCompression::new().decompress_to_buffer(self.data.as_ref(), output)
            }
        }
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

    pub fn bytes(&self) -> &[u8] {
        self.data.as_ref()
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
