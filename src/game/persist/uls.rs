use crate::compression::inspect::zstd::{max_byte_in_zstd_stream, ZstdInspectError};
use crate::compression::CompressionKind;
use crate::game::chunk::{BoundedChunk, CompressedChunk, CompressedChunkTransform};
use crate::game::chunker::{Chunker, StripChunker};
use crate::game::simulation::{FinalizedSimulation, Game, Player, PlayerId};
use crate::io::{
    read_i32_le, read_i8_le, read_u16_le, read_u32_le, read_u64_le, read_u8_le, write_i32_le,
    write_i8_le, write_u16_le, write_u32_le, write_u64_le, write_u8_le,
};
use crate::math::coords::GridPoint;
use crate::util::memory::view_as_bytes_mut;
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::io::{ErrorKind, Read, Write};

// Constraints for the ULS (Ulam Leapers Simulation) persistence format.
pub const ULS_MIN_CHUNK_ALIGNMENT: u64 = 64;
pub const ULS_MAX_CHUNK_SIZE: u64 = 4096 * 4096;
pub const ULS_MAX_CHUNK_EXTENT: u64 = 8192;

pub const ULS_MAX_ATTACK_VECTOR_COUNT: u64 = u8::MAX as u64;
pub const ULS_MAX_ATTACK_VECTOR_COORD: u64 = i8::MAX as u64;

pub const ULS_MAX_CHUNK_COUNT: u64 = u32::MAX as u64;
pub const ULS_MAX_CHUNK_ORIGIN_COORD: u64 = 1 << 30;
pub const ULS_MAX_CHUNK_BLOB_SIZE: u64 = ULS_MAX_CHUNK_SIZE * 2;

pub const ULS_MAX_PLAYER_COUNT: u64 = (u64::BITS - 1) as u64;

pub const ULS_MAX_TURN_COUNT: u64 = 1 << 60;
pub const ULS_MAX_SPIRAL_POSITION: u64 = 1 << 60;

pub const ULS_MAGIC_FORMAT_SIGNATURE: &[u8; 8] = b"ULS_v1.0";

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub struct UlsChunker {
    pub(in crate::game) strip_length: u16,
    pub(in crate::game) strip_thickness: u16,
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum UlsCompressionKind {
    None = 0,
    Zstd = 1,
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum UlsChunkTransform {
    None = 0,
    Transposition = 1,
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Copy, Clone)]
pub struct UlsAttackVector {
    pub(in crate::game) x: i8,
    pub(in crate::game) y: i8,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct UlsPlayer {
    pub(in crate::game) attack_vectors: Vec<UlsAttackVector>,
    pub(in crate::game) enemies_mask: u64,
    pub(in crate::game) spiral_position: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct UlsChunk<'a> {
    pub(in crate::game) origin_x: i32,
    pub(in crate::game) origin_y: i32,
    pub(in crate::game) transform: UlsChunkTransform,
    pub(in crate::game) compression_kind: UlsCompressionKind,
    pub(in crate::game) compressed_data: Cow<'a, [u8]>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct UlsSimulation<'a> {
    pub(in crate::game) chunker: UlsChunker,
    pub(in crate::game) chunks: Vec<UlsChunk<'a>>,
    pub(in crate::game) players: Vec<UlsPlayer>,
    pub(in crate::game) turn_count: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum UlsError {
    ChunkerChunkAlignmentTooLow {
        actual: u64,
    },
    ChunkerChunkSizeTooHigh {
        actual_strip_length: u64,
        actual_strip_thickness: u64,
    },
    ChunkerChunkExtentTooHigh {
        actual_strip_length: u64,
        actual_strip_thickness: u64,
    },
    ChunkerChunkExtentNotPow2 {
        actual_strip_length: u64,
        actual_strip_thickness: u64,
    },
    ChunkerStripThicknessHigherThanLength {
        actual_strip_length: u64,
        actual_strip_thickness: u64,
    },
    TooManyAttackVectors {
        actual: u64,
    },
    AttackVectorTooLarge {
        actual_x: i64,
        actual_y: i64,
    },
    ChunkOriginTooFar {
        actual_x: i64,
        actual_y: i64,
    },
    ChunkBlobTooLarge {
        actual: u64,
    },
    TooManyPlayers {
        actual: u64,
    },
    TooManyTurns {
        actual: u64,
    },
    SpiralPositionTooHigh {
        actual: u64,
    },
    TooManyChunks {
        actual: u64,
    },
    InvalidChunkTransform {
        transform: u8,
    },
    InvalidCompressionKind {
        kind: u8,
    },
    InvalidMagicFormatSignature {
        actual: Box<[u8]>,
    },
    PlayerIdInCellTooHigh {
        actual: PlayerId,
        highest: PlayerId,
    },
    InvalidChunkOrigin {
        actual_x: i32,
        actual_y: i32,
        expected_x: i32,
        expected_y: i32,
    },
    DuplicateAttackVectors {
        duplicate_count: u64,
    },
    DuplicateChunks {
        duplicate_count: u64,
    },
    ZstdInspectError(ZstdInspectError),
}

impl Display for UlsError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            UlsError::ChunkerChunkAlignmentTooLow { actual } => {
                write!(
                    f,
                    "Chunk alignment too low: {actual} < {ULS_MIN_CHUNK_ALIGNMENT}"
                )
            }
            UlsError::ChunkerChunkSizeTooHigh {
                actual_strip_length,
                actual_strip_thickness,
            } => {
                let size = actual_strip_length * actual_strip_thickness;
                write!(
                    f,
                    "Chunk size too high: {actual_strip_length}*{actual_strip_thickness}={size} > {ULS_MAX_CHUNK_SIZE}"
                )
            }
            UlsError::ChunkerChunkExtentTooHigh {
                actual_strip_length,
                actual_strip_thickness,
            } => {
                write!(
                    f,
                    "Chunk extent too high: ({actual_strip_length}, {actual_strip_thickness}) > ({ULS_MAX_CHUNK_EXTENT}, {ULS_MAX_CHUNK_EXTENT})"
                )
            }
            UlsError::ChunkerChunkExtentNotPow2 {
                actual_strip_length,
                actual_strip_thickness,
            } => {
                write!(
                    f,
                    "Chunk extent not a power of two: ({actual_strip_length}, {actual_strip_thickness})"
                )
            }
            UlsError::ChunkerStripThicknessHigherThanLength {
                actual_strip_length,
                actual_strip_thickness,
            } => {
                write!(
                    f,
                    "Chunk strip thickness {actual_strip_thickness} > chunk strip length {actual_strip_length}"
                )
            }
            UlsError::TooManyAttackVectors { actual } => {
                write!(
                    f,
                    "Too many attack vectors: {actual} > {ULS_MAX_ATTACK_VECTOR_COUNT}"
                )
            }
            UlsError::AttackVectorTooLarge { actual_x, actual_y } => {
                write!(
                    f,
                    "Attack vector too large: abs({actual_x}, {actual_y}) > ({ULS_MAX_ATTACK_VECTOR_COORD}, {ULS_MAX_ATTACK_VECTOR_COORD})"
                )
            }
            UlsError::ChunkOriginTooFar { actual_x, actual_y } => {
                write!(
                    f,
                    "Chunk origin too far: abs({actual_x}, {actual_y}) > ({ULS_MAX_CHUNK_ORIGIN_COORD}, {ULS_MAX_CHUNK_ORIGIN_COORD})"
                )
            }
            UlsError::ChunkBlobTooLarge { actual } => {
                write!(
                    f,
                    "Chunk blob too large: {actual} > {ULS_MAX_CHUNK_BLOB_SIZE}"
                )
            }
            UlsError::TooManyPlayers { actual } => {
                write!(f, "Too many players: {actual} > {ULS_MAX_PLAYER_COUNT}")
            }
            UlsError::TooManyTurns { actual } => {
                write!(f, "Too many turns: {actual} > {ULS_MAX_TURN_COUNT}")
            }
            UlsError::SpiralPositionTooHigh { actual } => {
                write!(
                    f,
                    "Spiral position too high: {actual} > {ULS_MAX_SPIRAL_POSITION}"
                )
            }
            UlsError::TooManyChunks { actual } => {
                write!(f, "Too many chunks: {actual} > {ULS_MAX_CHUNK_COUNT}")
            }
            UlsError::InvalidChunkTransform { transform } => {
                write!(f, "Invalid chunk transform: {transform}")
            }
            UlsError::InvalidCompressionKind { kind } => {
                write!(f, "Invalid compression kind: {kind}")
            }
            UlsError::InvalidMagicFormatSignature { actual } => {
                write!(f, "Invalid magic format signature: {actual:?}")
            }
            UlsError::PlayerIdInCellTooHigh { actual, highest } => {
                let actual_index = actual.index();
                let highest_index = highest.index();
                write!(f, "Player id too high: {actual_index} > {highest_index}")
            }
            UlsError::InvalidChunkOrigin {
                actual_x,
                actual_y,
                expected_x,
                expected_y,
            } => {
                write!(
                    f,
                    "Invalid chunk origin: actual ({actual_x}, {actual_y}) != expected ({expected_x}, {expected_y})"
                )
            }
            UlsError::DuplicateAttackVectors { duplicate_count } => {
                write!(f, "{duplicate_count} duplicate attack vectors")
            }
            UlsError::DuplicateChunks { duplicate_count } => {
                write!(f, "{duplicate_count} duplicate chunks")
            }
            UlsError::ZstdInspectError(err) => {
                write!(f, "ZST inspect error: {err}")
            }
        }
    }
}

impl Error for UlsError {}

impl From<UlsError> for std::io::Error {
    fn from(e: UlsError) -> Self {
        std::io::Error::new(ErrorKind::InvalidData, e.to_string())
    }
}

impl From<ZstdInspectError> for UlsError {
    fn from(err: ZstdInspectError) -> Self {
        UlsError::ZstdInspectError(err)
    }
}

impl TryFrom<u8> for UlsChunkTransform {
    type Error = UlsError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(UlsChunkTransform::None),
            1 => Ok(UlsChunkTransform::Transposition),
            _ => Err(UlsError::InvalidChunkTransform { transform: value }),
        }
    }
}

impl TryFrom<u8> for UlsCompressionKind {
    type Error = UlsError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(UlsCompressionKind::None),
            1 => Ok(UlsCompressionKind::Zstd),
            _ => Err(UlsError::InvalidCompressionKind { kind: value }),
        }
    }
}

impl TryFrom<&Player> for UlsPlayer {
    type Error = UlsError;

    fn try_from(player: &Player) -> Result<Self, Self::Error> {
        let spiral_position = player.cursor().spiral_position().as_u64();
        if spiral_position > ULS_MAX_SPIRAL_POSITION {
            return Err(UlsError::SpiralPositionTooHigh {
                actual: spiral_position,
            });
        }

        let attack_vectors = player.attacks().attack_vectors();
        if attack_vectors.len() as u64 > ULS_MAX_ATTACK_VECTOR_COUNT {
            return Err(UlsError::TooManyAttackVectors {
                actual: attack_vectors.len() as u64,
            });
        }

        let uls_attack_vectors = attack_vectors
            .iter()
            .map(|v| {
                if v.x.unsigned_abs() as u64 > ULS_MAX_ATTACK_VECTOR_COORD
                    || v.y.unsigned_abs() as u64 > ULS_MAX_ATTACK_VECTOR_COORD
                {
                    Err(UlsError::AttackVectorTooLarge {
                        actual_x: v.x as i64,
                        actual_y: v.y as i64,
                    })
                } else {
                    Ok(UlsAttackVector {
                        x: v.x as i8,
                        y: v.y as i8,
                    })
                }
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            spiral_position,
            enemies_mask: player.enemies().as_u64_mask(),
            attack_vectors: uls_attack_vectors,
        })
    }
}

impl TryFrom<&StripChunker> for UlsChunker {
    type Error = UlsError;

    fn try_from(chunker: &StripChunker) -> Result<Self, Self::Error> {
        let chunk_strip_length = chunker.strip_length();
        let chunk_strip_thickness = chunker.strip_thickness();
        assert!(
            chunk_strip_thickness <= chunk_strip_length,
            "This is an invariant of the chunker but important enough to check."
        );
        let chunk_size = chunker.chunk_size();

        if chunk_strip_length.as_u64() < ULS_MIN_CHUNK_ALIGNMENT {
            return Err(UlsError::ChunkerChunkAlignmentTooLow {
                actual: chunk_strip_length.as_u64(),
            });
        }
        if chunk_strip_thickness.as_u64() < ULS_MIN_CHUNK_ALIGNMENT {
            return Err(UlsError::ChunkerChunkAlignmentTooLow {
                actual: chunk_strip_thickness.as_u64(),
            });
        }

        if chunk_strip_length.as_u64() > ULS_MAX_CHUNK_EXTENT
            || chunk_strip_thickness.as_u64() > ULS_MAX_CHUNK_EXTENT
        {
            return Err(UlsError::ChunkerChunkExtentTooHigh {
                actual_strip_length: chunk_strip_length.as_u64(),
                actual_strip_thickness: chunk_strip_thickness.as_u64(),
            });
        }

        if chunk_size.as_u64() > ULS_MAX_CHUNK_SIZE {
            return Err(UlsError::ChunkerChunkSizeTooHigh {
                actual_strip_length: chunk_strip_length.as_u64(),
                actual_strip_thickness: chunk_strip_thickness.as_u64(),
            });
        }

        Ok(Self {
            strip_length: chunk_strip_length.as_u64() as u16,
            strip_thickness: chunk_strip_thickness.as_u64() as u16,
        })
    }
}

impl TryFrom<CompressedChunkTransform> for UlsChunkTransform {
    type Error = UlsError;

    fn try_from(transform: CompressedChunkTransform) -> Result<Self, Self::Error> {
        match transform {
            CompressedChunkTransform::None => Ok(UlsChunkTransform::None),
            CompressedChunkTransform::Transposition => Ok(UlsChunkTransform::Transposition),
        }
    }
}

impl TryFrom<CompressionKind> for UlsCompressionKind {
    type Error = UlsError;

    fn try_from(compression: CompressionKind) -> Result<Self, Self::Error> {
        match compression {
            CompressionKind::None => Ok(UlsCompressionKind::None),
            CompressionKind::Zstd => Ok(UlsCompressionKind::Zstd),
        }
    }
}

impl<'a> TryFrom<&'a CompressedChunk<PlayerId>> for UlsChunk<'a> {
    type Error = UlsError;

    fn try_from(chunk: &'a CompressedChunk<PlayerId>) -> Result<Self, Self::Error> {
        let origin = chunk.origin().point();
        if origin.x.unsigned_abs() as u64 > ULS_MAX_CHUNK_ORIGIN_COORD
            || origin.y.unsigned_abs() as u64 > ULS_MAX_CHUNK_ORIGIN_COORD
        {
            return Err(UlsError::ChunkOriginTooFar {
                actual_x: origin.x as i64,
                actual_y: origin.y as i64,
            });
        }

        // NOTE: We do not verify whether it matches the chunker.
        //       It's somewhat expensive to check, and we trust the application
        //       to maintain its invariants properly.

        let uls_transform = UlsChunkTransform::try_from(chunk.transform())?;

        let blob = chunk.blob();
        let uls_compression_kind = UlsCompressionKind::try_from(blob.compression_kind())?;
        let compressed_data = blob.bytes();

        if compressed_data.len() as u64 > ULS_MAX_CHUNK_BLOB_SIZE {
            return Err(UlsError::ChunkBlobTooLarge {
                actual: compressed_data.len() as u64,
            });
        }

        Ok(Self {
            origin_x: origin.x,
            origin_y: origin.y,
            transform: uls_transform,
            compression_kind: uls_compression_kind,
            compressed_data: Cow::Borrowed(compressed_data),
        })
    }
}

impl<'a> TryFrom<&'a FinalizedSimulation> for UlsSimulation<'a> {
    type Error = UlsError;

    fn try_from(simulation: &'a FinalizedSimulation) -> Result<Self, Self::Error> {
        let player_count = simulation.player_count();
        assert_eq!(
            simulation.highest_player_id().index(),
            player_count,
            "This is an invariant of FinalizedSimulation but it's important enough to check."
        );
        if player_count as u64 > ULS_MAX_PLAYER_COUNT {
            return Err(UlsError::TooManyPlayers {
                actual: player_count as u64,
            });
        }

        let turn_count = simulation.complete_turns();
        if turn_count > ULS_MAX_TURN_COUNT {
            return Err(UlsError::TooManyTurns {
                actual: turn_count,
            });
        }

        let players = simulation.players();
        let uls_players = players
            .iter()
            .map(UlsPlayer::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        let grid = simulation.grid_ref();
        let chunker = grid
            .chunker()
            .as_strip_chunker()
            .expect("All current chunkers are compatible with strip chunker");
        let uls_chunker = UlsChunker::try_from(&chunker)?;

        let chunk_count = grid.chunk_count();
        if chunk_count as u64 > ULS_MAX_CHUNK_COUNT {
            return Err(UlsError::TooManyChunks {
                actual: chunk_count as u64,
            });
        }

        let chunks = grid
            .iter_chunks()
            .map(UlsChunk::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            chunks,
            turn_count,
            chunker: uls_chunker,
            players: uls_players,
        })
    }
}

impl UlsChunkTransform {
    pub fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        write_u8_le(writer, *self as u8)
    }

    pub fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        let val = read_u8_le(reader)?;
        Self::try_from(val).map_err(|e| e.into())
    }
}

impl UlsCompressionKind {
    pub fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        write_u8_le(writer, *self as u8)
    }

    pub fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        let val = read_u8_le(reader)?;
        Self::try_from(val).map_err(|e| e.into())
    }
}

impl UlsChunker {
    pub fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        write_u16_le(writer, self.strip_length)?;
        write_u16_le(writer, self.strip_thickness)?;
        Ok(())
    }

    pub fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        let strip_length = read_u16_le(reader)?;
        let strip_thickness = read_u16_le(reader)?;

        if strip_thickness > strip_length {
            return Err(
                UlsError::ChunkerStripThicknessHigherThanLength {
                    actual_strip_length: strip_length as u64,
                    actual_strip_thickness: strip_thickness as u64,
                }
                .into(),
            );
        }

        if !strip_length.is_power_of_two() || !strip_thickness.is_power_of_two() {
            return Err(UlsError::ChunkerChunkExtentNotPow2 {
                actual_strip_length: strip_length as u64,
                actual_strip_thickness: strip_thickness as u64,
            }
            .into());
        }

        if (strip_length as u64) < ULS_MIN_CHUNK_ALIGNMENT {
            return Err(UlsError::ChunkerChunkAlignmentTooLow {
                actual: strip_length as u64,
            }
            .into());
        }
        if (strip_thickness as u64) < ULS_MIN_CHUNK_ALIGNMENT {
            return Err(UlsError::ChunkerChunkAlignmentTooLow {
                actual: strip_thickness as u64,
            }
            .into());
        }

        if (strip_length as u64) > ULS_MAX_CHUNK_EXTENT
            || (strip_thickness as u64) > ULS_MAX_CHUNK_EXTENT
        {
            return Err(UlsError::ChunkerChunkExtentTooHigh {
                actual_strip_length: strip_length as u64,
                actual_strip_thickness: strip_thickness as u64,
            }
            .into());
        }

        let chunk_size = strip_length as u64 * strip_thickness as u64;
        if chunk_size > ULS_MAX_CHUNK_SIZE {
            return Err(UlsError::ChunkerChunkSizeTooHigh {
                actual_strip_length: strip_length as u64,
                actual_strip_thickness: strip_thickness as u64,
            }
            .into());
        }

        Ok(Self {
            strip_length,
            strip_thickness,
        })
    }
}

impl UlsChunk<'_> {
    pub fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        write_i32_le(writer, self.origin_x)?;
        write_i32_le(writer, self.origin_y)?;
        self.transform.write_to(writer)?;
        self.compression_kind.write_to(writer)?;
        write_u32_le(writer, self.compressed_data.len() as u32)?;
        writer.write_all(&self.compressed_data)?;
        Ok(())
    }

    /// # Important note
    ///
    /// The compressed data is NOT validated. In particular, it is possible that it will
    /// cause and error during decompression or decompress to a different number of bytes
    /// than expected. This behavior has been chosen because validating decompression
    /// is deemed too costly, and would largely defeat the point of using compression
    /// in the first place.
    pub fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        let origin_x = read_i32_le(reader)?;
        let origin_y = read_i32_le(reader)?;
        let transform = UlsChunkTransform::read_from(reader)?;

        let compression_kind = UlsCompressionKind::read_from(reader)?;
        let compressed_data_len = read_u32_le(reader)?;
        if compressed_data_len as u64 > ULS_MAX_CHUNK_BLOB_SIZE {
            return Err(UlsError::ChunkBlobTooLarge {
                actual: compressed_data_len as u64,
            }
            .into());
        }

        // NOTE: We do not verify if the data actually decompresses to exactly fill the chunk,
        //       because it would be too costly.
        let mut buf = Box::new_uninit_slice(compressed_data_len as usize);
        // SAFETY: We know the exact size of the buffer..
        let raw_buf = unsafe { view_as_bytes_mut(&mut buf) };
        reader.read_exact(raw_buf)?;
        // SAFETY: We have read exactly `compressed_data_len` bytes.
        let buf = unsafe { buf.assume_init() };
        let compressed_data: Cow<'_, [u8]> = Cow::Owned(buf.into_vec());

        Ok(Self {
            origin_x,
            origin_y,
            transform,
            compression_kind,
            compressed_data,
        })
    }

    pub fn highest_player_id(&self) -> Result<PlayerId, UlsError> {
        match self.compression_kind {
            UlsCompressionKind::None => Ok(PlayerId::new(
                *self.compressed_data.iter().max().unwrap_or(&0u8),
            )),
            UlsCompressionKind::Zstd => match max_byte_in_zstd_stream(&self.compressed_data) {
                Ok(v) => Ok(PlayerId::new(v)),
                Err(err) => Err(UlsError::ZstdInspectError(err)),
            },
        }
    }
}

impl UlsAttackVector {
    pub fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        write_i8_le(writer, self.x)?;
        write_i8_le(writer, self.y)?;
        Ok(())
    }

    pub fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        let x = read_i8_le(reader)?;
        let y = read_i8_le(reader)?;
        if x.unsigned_abs() as u64 > ULS_MAX_ATTACK_VECTOR_COORD
            || y.unsigned_abs() as u64 > ULS_MAX_ATTACK_VECTOR_COORD
        {
            return Err(UlsError::AttackVectorTooLarge {
                actual_x: x as i64,
                actual_y: y as i64,
            }
            .into());
        }

        Ok(Self { x, y })
    }
}

impl UlsPlayer {
    pub fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        write_u64_le(writer, self.spiral_position)?;
        write_u64_le(writer, self.enemies_mask)?;
        write_u8_le(writer, self.attack_vectors.len() as u8)?;
        for attack_vector in &self.attack_vectors {
            attack_vector.write_to(writer)?;
        }
        Ok(())
    }

    pub fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        let spiral_position = read_u64_le(reader)?;
        let enemies_mask = read_u64_le(reader)?;
        let attack_vectors_len = read_u8_le(reader)?;

        if spiral_position > ULS_MAX_SPIRAL_POSITION {
            return Err(UlsError::SpiralPositionTooHigh {
                actual: spiral_position,
            }
            .into());
        }

        assert!(ULS_MAX_ATTACK_VECTOR_COUNT >= u8::MAX as u64);

        let attack_vectors = (0..attack_vectors_len)
            .map(|_| UlsAttackVector::read_from(reader))
            .collect::<Result<Vec<_>, _>>()?;

        let attack_vectors_unique: BTreeSet<_> = attack_vectors.iter().collect();
        if attack_vectors_unique.len() != attack_vectors.len() {
            return Err(UlsError::DuplicateAttackVectors {
                duplicate_count: (attack_vectors.len() - attack_vectors_unique.len()) as u64,
            }
            .into());
        }

        Ok(Self {
            spiral_position,
            enemies_mask,
            attack_vectors,
        })
    }
}

impl UlsSimulation<'_> {
    pub fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        writer.write_all(ULS_MAGIC_FORMAT_SIGNATURE)?;

        write_u64_le(writer, self.turn_count)?;

        write_u8_le(writer, self.players.len() as u8)?;
        for player in &self.players {
            player.write_to(writer)?;
        }

        self.chunker.write_to(writer)?;

        write_u32_le(writer, self.chunks.len() as u32)?;
        for chunk in &self.chunks {
            chunk.write_to(writer)?;
        }

        Ok(())
    }

    fn read_chunk<'a>(
        reader: &mut impl Read,
        chunker: &StripChunker,
        highest_player_id: PlayerId,
    ) -> std::io::Result<UlsChunk<'a>> {
        let uls_chunk = UlsChunk::read_from(reader)?;

        let highest_player_id_in_chunk = uls_chunk
            .highest_player_id()
            .map_err(std::io::Error::from)?;
        if highest_player_id_in_chunk > highest_player_id {
            return Err(UlsError::PlayerIdInCellTooHigh {
                actual: highest_player_id_in_chunk,
                highest: highest_player_id,
            }
            .into());
        }

        let expected_origin = chunker
            .resolve_chunk_origin(&GridPoint::new(uls_chunk.origin_x, uls_chunk.origin_y))
            .point();
        if expected_origin.x != uls_chunk.origin_x || expected_origin.y != uls_chunk.origin_y {
            return Err(UlsError::InvalidChunkOrigin {
                actual_x: uls_chunk.origin_x,
                actual_y: uls_chunk.origin_y,
                expected_x: expected_origin.x,
                expected_y: expected_origin.y,
            }
            .into());
        }

        Ok(uls_chunk)
    }

    /// # Important note
    ///
    /// `UlsChunk` entries are not fully validated, see [UlsChunk::read_from] for details.
    /// However, the validity of the `PlayerId`s in the decompressed chunk stream (provided
    /// it can be decompressed in the first place) IS verified. This is achieved via
    /// compressed bitstream inspection of the literal sections - no data is decompressed.
    /// This validation is performed so that there's fewer costly last-moment checks
    /// required when sampling the grid.
    pub fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        let mut magic = [0u8; ULS_MAGIC_FORMAT_SIGNATURE.len()];
        reader.read_exact(&mut magic)?;
        if &magic != ULS_MAGIC_FORMAT_SIGNATURE {
            return Err(UlsError::InvalidMagicFormatSignature {
                actual: magic.to_vec().into_boxed_slice(),
            }
            .into());
        }

        let turn_count = read_u64_le(reader)?;
        if turn_count > ULS_MAX_TURN_COUNT {
            return Err(UlsError::TooManyTurns { actual: turn_count }.into());
        }

        let player_count = read_u8_le(reader)?;
        if player_count as u64 > ULS_MAX_PLAYER_COUNT {
            return Err(UlsError::TooManyPlayers {
                actual: player_count as u64,
            }
            .into());
        }

        let players = (0..player_count)
            .map(|_| UlsPlayer::read_from(reader))
            .collect::<Result<Vec<_>, _>>()?;

        let chunker = UlsChunker::read_from(reader)?;
        // We need an actual chunker to determine if the origins are correct.
        let actual_chunker = StripChunker::from(chunker);

        let chunk_count = read_u32_le(reader)?;
        assert!(ULS_MAX_CHUNK_COUNT >= u32::MAX as u64);

        let highest_player_id = PlayerId::new(player_count);
        let chunks = (0..chunk_count)
            .map(|_| Self::read_chunk(reader, &actual_chunker, highest_player_id))
            .collect::<Result<Vec<_>, _>>()?;

        let origins: BTreeSet<_> = chunks.iter().map(|chunk| (chunk.origin_x, chunk.origin_y)).collect();
        if origins.len() != chunks.len() {
            return Err(UlsError::DuplicateChunks {
                duplicate_count: (chunks.len() - origins.len()) as u64,
            }.into());
        }

        Ok(Self {
            turn_count,
            players,
            chunker,
            chunks,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::chunker::Chunker;
    use crate::game::piece::LeaperAttacks;
    use crate::game::simulation::{Simulation, SimulationLimits};
    use crate::math::coords::GridVector;
    use crate::math::pow2::Pow2;

    fn make_chunker() -> Box<dyn Chunker> {
        Box::new(StripChunker::with_strip_length_and_thickness(
            Pow2::from_exponent(8),
            Pow2::from_exponent(6),
        ))
    }

    #[test]
    fn uls_simulation_from_empty_simulation() {
        let sim = Simulation::with_chunker(make_chunker());
        let fin_sim = sim.finalize();
        let uls_sim = UlsSimulation::try_from(&fin_sim).unwrap();

        assert_eq!(uls_sim.turn_count, 0);
        assert_eq!(uls_sim.players.len(), 0);
        assert_eq!(uls_sim.chunks.len(), 0);
        assert_eq!(uls_sim.chunker.strip_length, 256);
        assert_eq!(uls_sim.chunker.strip_thickness, 64);
    }

    #[test]
    fn uls_simulation_from_unstarted_simulation() {
        let mut sim = Simulation::with_chunker(make_chunker());
        let _p1 = sim.add_player(LeaperAttacks::from_offsets(
            [GridVector::new(1, 1), GridVector::new(1, 3)]
                .into_iter()
                .collect(),
        ));
        let _p2 = sim.add_player(LeaperAttacks::from_offsets(
            [GridVector::new(1, 2)].into_iter().collect(),
        ));
        let fin_sim = sim.finalize();
        let uls_sim = UlsSimulation::try_from(&fin_sim).unwrap();

        assert_eq!(uls_sim.turn_count, 0);
        assert_eq!(uls_sim.players.len(), 2);
        assert_eq!(uls_sim.players[0].enemies_mask, 0);
        assert_eq!(uls_sim.players[0].spiral_position, 0);
        assert_eq!(uls_sim.players[0].attack_vectors.len(), 2);
        assert!(
            uls_sim.players[0]
                .attack_vectors
                .contains(&UlsAttackVector { x: 1, y: 1 })
        );
        assert!(
            uls_sim.players[0]
                .attack_vectors
                .contains(&UlsAttackVector { x: 1, y: 3 })
        );
        assert_eq!(uls_sim.players[1].enemies_mask, 0);
        assert_eq!(uls_sim.players[1].spiral_position, 0);
        assert_eq!(uls_sim.players[1].attack_vectors.len(), 1);
        assert!(
            uls_sim.players[1]
                .attack_vectors
                .contains(&UlsAttackVector { x: 1, y: 2 })
        );
        assert_eq!(uls_sim.chunks.len(), 0);
        assert_eq!(uls_sim.chunker.strip_length, 256);
        assert_eq!(uls_sim.chunker.strip_thickness, 64);
    }

    #[test]
    fn uls_simulation_from_two_knight_simulation() {
        let mut sim = Simulation::with_chunker(make_chunker());
        let _p1 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
        let _p2 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
        sim.add_all_pairwise_player_enemies();
        sim.simulate(SimulationLimits::new().with_turn_limit(5))
            .unwrap();
        let fin_sim = sim.finalize();
        let uls_sim = UlsSimulation::try_from(&fin_sim).unwrap();

        //    2  2  1  1
        //    1 [1] 2  2
        //    2  _  _  1

        assert_eq!(uls_sim.turn_count, 5);
        assert_eq!(uls_sim.players.len(), 2);
        assert_eq!(uls_sim.players[0].enemies_mask, u64::MAX ^ 0b010);
        assert_eq!(uls_sim.players[0].spiral_position, 12);
        assert_eq!(uls_sim.players[1].enemies_mask, u64::MAX ^ 0b100);
        assert_eq!(uls_sim.players[1].spiral_position, 11);
        assert_eq!(uls_sim.chunks.len(), 4);
    }

    #[test]
    fn uls_simulation_round_trip_read() {
        let mut sim = Simulation::with_chunker(make_chunker());
        let _p1 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
        let _p2 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
        sim.add_all_pairwise_player_enemies();
        sim.simulate(SimulationLimits::new().with_turn_limit(5))
            .unwrap();
        let fin_sim = sim.finalize();
        let uls_sim = UlsSimulation::try_from(&fin_sim).unwrap();

        let mut out_vec = vec![];
        uls_sim.write_to(&mut out_vec).unwrap();
        let uls_sim_read = UlsSimulation::read_from(&mut out_vec.as_slice()).unwrap();

        assert_eq!(uls_sim, uls_sim_read);

        let mut out_vec_rewritten = vec![];
        uls_sim_read.write_to(&mut out_vec_rewritten).unwrap();

        assert_eq!(out_vec, out_vec_rewritten);
    }
}
