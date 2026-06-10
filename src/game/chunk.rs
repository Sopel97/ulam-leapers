use std::io::{ErrorKind, Read, Write};
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ops::{Index, IndexMut};
use crate::algo::transpose::transpose_u8;
use crate::collections::aligned_boxed_slice::AlignedBoxedSlice;
use crate::collections::array2d::Array2D;
use crate::compression::{AnyCompression, CompressedBlob, Compression, CompressionKind};
use crate::io::{ReadFrom, WriteTo};
use crate::math::coords::GridPoint;
use crate::math::rect::GridRect;
use crate::util::align::CACHE_LINE_SIZE;
use crate::util::memory::{view_as_bytes, view_as_bytes_mut, MemSize};

// Chunk size and alignment constraints for the ULS (Ulam Leapers Simulation) persistence format.
pub const ULS_MINIMUM_CHUNK_ALIGNMENT: usize = 64;
pub const ULS_MAXIMUM_CHUNK_SIZE: usize = 2048 * 2048;
pub const ULS_MAXIMUM_CHUNK_EXTENT: usize = 8192;

#[derive(Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct ChunkOrigin(GridPoint);

impl ChunkOrigin {
    pub fn new(point: GridPoint) -> Self {
        Self(point)
    }
    
    pub fn point(&self) -> GridPoint {
        self.0
    }
}

pub trait BoundedChunk {
    fn bounds(&self) -> &GridRect;

    fn contains_point(&self, point: &GridPoint) -> bool {
        self.bounds().contains_point(point)
    }

    fn origin(&self) -> ChunkOrigin {
        ChunkOrigin(self.bounds().start)
    }
}

// NOTE: T must be accessible as raw bytes.
pub struct Chunk<T> {
    bounds: GridRect,
    cells: Array2D<T>,
}

impl<T> BoundedChunk for Chunk<T> {
    fn bounds(&self) -> &GridRect {
        &self.bounds
    }
}

impl<T> Chunk<T> {
    pub fn memory_usage(&self) -> MemSize {
        MemSize::sizes_of::<T>(self.cells.width() * self.cells.height())
    }
}

impl<T: Default + Clone> Chunk<T> {
    pub fn new(bounds: GridRect) -> Chunk<T> {
        let cells = Array2D::<T>::new_aligned(
            bounds.width() as usize,
            bounds.height() as usize,
            CACHE_LINE_SIZE,
        );
        Chunk { bounds, cells }
    }
}

impl<T: Default> Index<GridPoint> for Chunk<T> {
    type Output = T;

    #[inline(always)]
    fn index(&self, index: GridPoint) -> &Self::Output {
        let xx = index.x - self.bounds.start.x;
        let yy = index.y - self.bounds.start.y;
        &self.cells[(xx as usize, yy as usize)]
    }
}

impl<T: Default> IndexMut<GridPoint> for Chunk<T> {
    #[inline(always)]
    fn index_mut(&mut self, index: GridPoint) -> &mut Self::Output {
        let xx = index.x - self.bounds.start.x;
        let yy = index.y - self.bounds.start.y;
        &mut self.cells[(xx as usize, yy as usize)]
    }
}

impl<T> Chunk<T> {
    /// # Safety
    ///
    /// Calling this method with an out-of-bounds index is *[undefined behavior]*
    /// even if the resulting reference is not used.
    ///
    /// [undefined behavior]: https://doc.rust-lang.org/reference/behavior-considered-undefined.html
    #[inline(always)]
    pub unsafe fn get_unchecked(&self, index: GridPoint) -> &T {
        let xx = index.x - self.bounds.start.x;
        let yy = index.y - self.bounds.start.y;
        unsafe { self.cells.get_unchecked(xx as usize, yy as usize) }
    }

    /// # Safety
    ///
    /// Calling this method with an out-of-bounds index is *[undefined behavior]*
    /// even if the resulting reference is not used.
    ///
    /// [undefined behavior]: https://doc.rust-lang.org/reference/behavior-considered-undefined.html
    #[inline(always)]
    pub unsafe fn get_unchecked_mut(&mut self, index: GridPoint) -> &mut T {
        let xx = index.x - self.bounds.start.x;
        let yy = index.y - self.bounds.start.y;
        unsafe { self.cells.get_unchecked_mut(xx as usize, yy as usize) }
    }
}

pub enum CompressedChunkTransform {
    None,
    Transposition,
}

impl WriteTo for CompressedChunkTransform {
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        match self {
            CompressedChunkTransform::None => b'N'.write_to(writer),
            CompressedChunkTransform::Transposition => b'T'.write_to(writer),
        }
    }
}

impl ReadFrom for CompressedChunkTransform {
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        match u8::read_from(reader)? {
            b'N' => Ok(CompressedChunkTransform::None),
            b'T' => Ok(CompressedChunkTransform::Transposition),
            _ => Err(std::io::Error::new(
                ErrorKind::InvalidData,
                "Invalid chunk transform type.",
            )),
        }
    }
}

// Generic over T because we want to preserve type information of the underlying data.
pub struct CompressedChunk<T> {
    bounds: GridRect,
    transform: CompressedChunkTransform,
    data: CompressedBlob,
    _marker: PhantomData<T>,
}

impl<T: Default> Chunk<T> {
    /// Compresses the chunk using a given compressor.
    ///
    /// # Notes
    ///
    /// Currently, it attempts two compressions, one on the chunk data in row-major order,
    /// and one on te chunk data in column-major order. Whichever ends up smaller is chosen.
    /// While this is inefficient it improves compressions significantly on most grids.
    /// A better heuristic, and/or other transforms, may be used in the future.
    pub fn compress(&self, compression: &AnyCompression) -> CompressedChunk<T> {
        let mut compressed = {
            // SAFETY: We kinda assume that T is accessible as raw bytes.
            let raw_uncompressed = unsafe { view_as_bytes(self.cells.as_flat_slice()) };

            compression.compress_to_blob(raw_uncompressed).unwrap()
        };

        let compressed_transposed = {
            let mut transposed_buf = AlignedBoxedSlice::<MaybeUninit<T>>::new_uninit(
                self.cells.as_flat_slice().len(),
                CACHE_LINE_SIZE,
            );
            // SAFETY: We kinda assume that T is accessible as raw bytes.
            let raw_uncompressed = unsafe { view_as_bytes(self.cells.as_flat_slice()) };

            // SAFETY:
            // - MaybeUninit<u8> has the same layout as u8
            // - transpose_u8 fully overwrites every byte of the destination before
            //   the slice is ever read.
            let raw_uncompressed_transposed: &mut [u8] =
                unsafe { view_as_bytes_mut(transposed_buf.as_mut_slice()) };

            // This transpose completely overwrites the whole raw_uncompressed_transposed
            transpose_u8(
                raw_uncompressed,
                raw_uncompressed_transposed,
                self.cells.width() * size_of::<T>(),
                self.cells.height(),
            );

            // raw_uncompressed_transposed is fully initialized at this point
            compression
                .compress_to_blob(&*raw_uncompressed_transposed)
                .unwrap()
        };

        let transform = if compressed_transposed.len() < compressed.len() {
            compressed = compressed_transposed;
            CompressedChunkTransform::Transposition
        } else {
            CompressedChunkTransform::None
        };

        CompressedChunk {
            bounds: self.bounds,
            data: compressed,
            transform,
            _marker: PhantomData,
        }
    }
}

impl<T: Default + Clone + Copy> CompressedChunk<T> {
    pub fn decompress(&self) -> Chunk<T> {
        let width = self.bounds.width() as usize;
        let height = self.bounds.height() as usize;
        let mut cells: Array2D<MaybeUninit<T>> =
            Array2D::new_uninit_aligned(width, height, CACHE_LINE_SIZE);
        // SAFETY: We kinda assume that T is accessible as raw bytes.
        let raw_cells = unsafe { view_as_bytes_mut(cells.as_flat_mut_slice()) };

        match self.transform {
            CompressedChunkTransform::None => {
                assert_eq!(
                    self.data.decompress_to_buffer(raw_cells).unwrap(),
                    raw_cells.len()
                );
            }
            CompressedChunkTransform::Transposition => {
                let mut transposed_buf = AlignedBoxedSlice::<MaybeUninit<u8>>::new_uninit(
                    raw_cells.len(),
                    CACHE_LINE_SIZE,
                );
                // SAFETY: raw_uncompressed_transposed will have been fully overwritten
                //         by the zstd decompression by the time we call transpose_u8.
                let raw_uncompressed_transposed =
                    unsafe { view_as_bytes_mut(transposed_buf.as_mut_slice()) };
                assert_eq!(
                    self.data
                        .decompress_to_buffer(raw_uncompressed_transposed,)
                        .unwrap(),
                    raw_uncompressed_transposed.len()
                );
                transpose_u8(
                    raw_uncompressed_transposed,
                    raw_cells,
                    width,
                    height * size_of::<T>(),
                );
            }
        }
        Chunk {
            bounds: self.bounds,
            // SAFETY: We have verified that zstd decompressed exactly the whole buffer.
            cells: unsafe { cells.assume_init() },
        }
    }
}

impl<T> BoundedChunk for CompressedChunk<T> {
    fn bounds(&self) -> &GridRect {
        &self.bounds
    }
}

impl<T> CompressedChunk<T> {
    pub fn memory_usage(&self) -> MemSize {
        MemSize::size_of::<CompressedChunk<T>>() + MemSize::b(self.data.len())
    }
}

impl<T> WriteTo for CompressedChunk<T> {
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        self.bounds.write_to(writer)?;
        self.transform.write_to(writer)?;
        self.data.bytes().write_to(writer)?;
        Ok(())
    }
}

impl<T> ReadFrom for CompressedChunk<T> {
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        Ok(CompressedChunk {
            bounds: GridRect::read_from(reader)?,
            transform: CompressedChunkTransform::read_from(reader)?,
            data: CompressedBlob::from_raw_parts(
                CompressionKind::Zstd,
                Box::<[u8]>::read_from(reader)?,
            ),
            _marker: PhantomData,
        })
    }
}

impl WriteTo for ChunkOrigin {
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        self.0.write_to(writer)
    }
}

impl ReadFrom for ChunkOrigin {
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        Ok(ChunkOrigin(GridPoint::read_from(reader)?))
    }
}