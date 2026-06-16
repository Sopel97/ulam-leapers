use std::borrow::Cow;
use crate::algo::transpose::transpose_u8;
use crate::collections::aligned_boxed_slice::AlignedBoxedSlice;
use crate::collections::array2d::Array2D;
use crate::compression::{AnyCompression, CompressedBlob, Compression, CompressionKind};
use crate::math::coords::GridPoint;
use crate::math::rect::GridRect;
use crate::util::align::CACHE_LINE_SIZE;
use crate::util::memory::{view_as_bytes, view_as_bytes_mut, MemSize};
use std::io::{ErrorKind, Read, Write};
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ops::{Index, IndexMut};
use crate::game::chunker::{Chunker, StripChunker};
use crate::game::persist::uls::{UlsChunk, UlsChunkTransform};
use crate::game::simulation::PlayerId;

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd)]
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
#[derive(Debug)]
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
        let offset = index - self.bounds.start;
        &self.cells[(offset.x as usize, offset.y as usize)]
    }
}

impl<T: Default> IndexMut<GridPoint> for Chunk<T> {
    #[inline(always)]
    fn index_mut(&mut self, index: GridPoint) -> &mut Self::Output {
        let offset = index - self.bounds.start;
        &mut self.cells[(offset.x as usize, offset.y as usize)]
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
        let offset = index - self.bounds.start;
        unsafe {
            self.cells
                .get_unchecked(offset.x as usize, offset.y as usize)
        }
    }

    /// # Safety
    ///
    /// Calling this method with an out-of-bounds index is *[undefined behavior]*
    /// even if the resulting reference is not used.
    ///
    /// [undefined behavior]: https://doc.rust-lang.org/reference/behavior-considered-undefined.html
    #[inline(always)]
    pub unsafe fn get_unchecked_mut(&mut self, index: GridPoint) -> &mut T {
        let offset = index - self.bounds.start;
        unsafe {
            self.cells
                .get_unchecked_mut(offset.x as usize, offset.y as usize)
        }
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum CompressedChunkTransform {
    None,
    Transposition,
}

impl From<UlsChunkTransform> for CompressedChunkTransform {
    fn from(uls_transform: UlsChunkTransform) -> Self {
        match uls_transform {
            UlsChunkTransform::None => CompressedChunkTransform::None,
            UlsChunkTransform::Transposition => CompressedChunkTransform::Transposition,
        }
    }
}

// Generic over T because we want to preserve type information of the underlying data.
#[derive(Debug)]
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
                    height * size_of::<T>(),
                    width,
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
    
    pub fn blob(&self) -> &CompressedBlob {
        &self.data
    }
    
    pub fn transform(&self) -> CompressedChunkTransform {
        self.transform
    }
}

impl<PlayerId> CompressedChunk<PlayerId> {
    pub fn from_uls(uls_chunk: UlsChunk, chunker: &StripChunker) -> Self {
        let UlsChunk {
            origin_x,
            origin_y,
            transform: uls_transform,
            compression_kind: uls_compression_kind,
            compressed_data: uls_compressed_data,
        } = uls_chunk;

        let bounds = chunker.resolve_chunk_bounds(
            &GridPoint::new(
                origin_x, origin_y
            )
        );

        let transform = CompressedChunkTransform::from(uls_transform);
        let compression_kind = CompressionKind::from(uls_compression_kind);
        let data = match uls_compressed_data {
            Cow::Owned(compressed_data) => CompressedBlob::from_raw_parts(compression_kind, compressed_data.into_boxed_slice()),
            Cow::Borrowed(compressed_data) => { panic!("Expected owned during deserialization.") }
        };
        
        // TODO: Consider compressing if not compressed already.

        Self {
            transform,
            bounds,
            data,
            _marker: Default::default(),
        }
    }
}