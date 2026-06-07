use crate::algo::transpose::transpose_u8;
use crate::collections::aligned_boxed_slice::AlignedBoxedSlice;
use crate::collections::array2d::{Array2D, Slice2D};
use crate::compression::{AnyCompression, CompressedBlob, Compression, CompressionKind};
use crate::coords::{Point2D, Rect2D, Vector2D};
use crate::io::{ReadFrom, WriteTo};
use crate::util::align::CACHE_LINE_SIZE;
use crate::util::blit::{Blit2D, blit_array2d_unchecked};
use crate::util::cache::{CacheEnabled, LockStepCache};
use crate::util::memory::{view_as_bytes, view_as_bytes_mut};
use crate::util::pow2;
use crate::util::pow2::{Pow2, floor_to_multiple};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::io::{ErrorKind, Read, Write};
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ops::{Index, IndexMut, Range};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, mpsc};
use std::thread;
use std::thread::JoinHandle;

pub type GridPoint = Point2D<i32>;
pub type GridVector = Vector2D<i32>;
pub type GridRect = Rect2D<i32>;

#[derive(Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct ChunkOrigin(GridPoint);

trait BoundedChunk {
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
    pub fn memory_usage(&self) -> usize {
        size_of::<T>() * self.cells.width() * self.cells.height()
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

impl<T> Chunk<T> {
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
            let mut transposed_buf = AlignedBoxedSlice::<MaybeUninit<u8>>::new_uninit(
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
    pub fn memory_usage(&self) -> usize {
        size_of::<CompressedChunk<T>>() + self.data.len()
    }
}

// Chunk size and alignment constraints for the ULS (Ulam Leapers Simulation) persistence format.
pub const ULS_MINIMUM_CHUNK_ALIGNMENT: usize = 64;
pub const ULS_MAXIMUM_CHUNK_SIZE: usize = 2048 * 2048;
pub const ULS_MAXIMUM_CHUNK_EXTENT: usize = 8192;

/// # NOTE
///
/// To maintain invariants for the ULS format instances of this type
/// should be made via StandardChunker::try_* instead.
/// This is a limitation of Rust that there is no way to validate enum
/// values directly. May change with a more convoluted abstraction in the future.
pub enum StandardChunker {
    SquareChunker { chunk_size_pow2: u8 },
}

impl StandardChunker {
    pub fn try_new_square_chunker(size: Pow2) -> Option<Self> {
        if size.as_usize() < ULS_MINIMUM_CHUNK_ALIGNMENT
            || size.as_usize() > ULS_MAXIMUM_CHUNK_EXTENT
            || size.as_usize().pow(2) > ULS_MAXIMUM_CHUNK_SIZE
        {
            None
        } else {
            Some(StandardChunker::SquareChunker {
                chunk_size_pow2: size.exponent(),
            })
        }
    }
}

impl TryFrom<&SquareChunker> for StandardChunker {
    type Error = ();

    fn try_from(chunker: &SquareChunker) -> Result<Self, Self::Error> {
        StandardChunker::try_new_square_chunker(chunker.size).ok_or(())
    }
}

impl StandardChunker {
    pub fn into_chunker(self) -> Box<dyn Chunker> {
        match self {
            StandardChunker::SquareChunker { chunk_size_pow2 } => Box::new(SquareChunker::new(
                Pow2::from_exponent(chunk_size_pow2 as usize),
            )),
        }
    }
}

pub trait Chunker: Send + Sync {
    fn resolve_chunk_origin(&self, _: &GridPoint) -> ChunkOrigin;
    fn resolve_chunk_bounds(&self, _: &GridPoint) -> GridRect;
    fn origins_of_intersecting_chunks(&self, region: &GridRect) -> Vec<ChunkOrigin>;
    fn minimum_chunk_alignment(&self) -> usize;
    fn minimum_chunk_extent(&self) -> usize;

    fn as_standard_chunker(&self) -> Option<StandardChunker>;
}

pub struct SquareChunker {
    size: Pow2,
}

impl SquareChunker {
    pub fn new(size: Pow2) -> SquareChunker {
        SquareChunker { size }
    }
}

impl Chunker for SquareChunker {
    fn resolve_chunk_origin(&self, point: &GridPoint) -> ChunkOrigin {
        let x = point.x;
        let y = point.y;

        // arithmetic shift provides floored division by a power of 2
        let cx = floor_to_multiple(x, self.size);
        let cy = floor_to_multiple(y, self.size);

        ChunkOrigin(GridPoint::new(cx, cy))
    }

    fn resolve_chunk_bounds(&self, bounds: &GridPoint) -> GridRect {
        let origin = self.resolve_chunk_origin(bounds);
        GridRect::square_with_size(origin.0, self.size.into())
    }

    fn origins_of_intersecting_chunks(&self, region: &GridRect) -> Vec<ChunkOrigin> {
        let min_ox = floor_to_multiple(region.start.x, self.size);
        let min_oy = floor_to_multiple(region.start.y, self.size);
        (min_oy..region.end.y)
            .step_by(self.size.into())
            .flat_map(|oy| {
                (min_ox..region.end.x)
                    .step_by(self.size.into())
                    .map(move |ox| ChunkOrigin(GridPoint::new(ox, oy)))
            })
            .collect()
    }

    fn minimum_chunk_alignment(&self) -> usize {
        self.size.into()
    }

    fn minimum_chunk_extent(&self) -> usize {
        self.size.into()
    }

    fn as_standard_chunker(&self) -> Option<StandardChunker> {
        StandardChunker::try_from(self).ok()
    }
}

pub struct Grid<T> {
    chunker: Box<dyn Chunker + Send + Sync>,
    compression: AnyCompression,
    active_chunks: BTreeMap<ChunkOrigin, Chunk<T>>,
    frozen_chunks: BTreeMap<ChunkOrigin, CompressedChunk<T>>,
    frozen_chunks_memory_usage: usize, // to reduce the amount of redundant iteration over chunks
}

impl<T: Send + Sync> Grid<T> {
    pub fn freeze(&mut self, region: &GridRect) {
        // While amortized this function won't do much it may be called on many chunks
        // at the time. There is no good way to change that without possibly blowing
        // up memory usage due to the number of uncompressed chunks growing.
        let to_freeze = self
            .active_chunks
            .extract_if(.., |_origin, chunk| region.contains(chunk.bounds()))
            // Not sure if there's a better way,
            // extract_if doesn't produce a par-compatible iterator.
            .collect::<Vec<_>>();
        let frozen = to_freeze
            .into_par_iter()
            .map(|entry| {
                let origin = entry.0;
                let chunk = entry.1;
                (origin, chunk.compress(&self.compression))
            })
            .collect::<Vec<_>>();
        // Collecting to a vector is not great but should be fine. Other ways of converting
        // parallel processing to sequential are annoying.
        for (origin, chunk) in frozen {
            self.frozen_chunks_memory_usage += chunk.memory_usage();
            self.frozen_chunks.insert(origin, chunk);
        }
    }

    pub fn freeze_all(&mut self) {
        // TODO: Remove this hack. We can't represent the full range properly.
        self.freeze(&GridRect::with_start_end(
            GridPoint::new(i32::MIN, i32::MIN),
            GridPoint::new(i32::MAX, i32::MAX),
        ));
    }
}

impl<T> Grid<T> {
    pub fn memory_usage(&self) -> usize {
        let s1: usize = self.active_chunks.values().map(|c| c.memory_usage()).sum();
        s1 + self.frozen_chunks_memory_usage
    }

    pub fn is_chunk_at_frozen(&self, origin: &ChunkOrigin) -> bool {
        self.frozen_chunks.contains_key(origin)
    }

    pub fn is_chunk_containing_frozen(&self, point: &GridPoint) -> bool {
        let origin = self.chunker.resolve_chunk_origin(point);
        self.is_chunk_at_frozen(&origin)
    }
}

impl<T: Default + Clone + Copy> Grid<T> {
    pub fn new(chunker: Box<dyn Chunker + Send + Sync>, compression: AnyCompression) -> Self {
        Grid {
            chunker,
            compression,
            active_chunks: BTreeMap::new(),
            frozen_chunks: BTreeMap::new(),
            frozen_chunks_memory_usage: 0,
        }
    }

    pub fn get_active_chunk_at(&self, point: &ChunkOrigin) -> Option<&Chunk<T>> {
        self.active_chunks.get(point)
    }

    pub fn get_active_chunk_containing(&self, point: &GridPoint) -> Option<&Chunk<T>> {
        let origin = self.chunker.resolve_chunk_origin(point);
        self.get_active_chunk_at(&origin)
    }

    pub fn get_frozen_chunk_at(&self, point: &ChunkOrigin) -> Option<&CompressedChunk<T>> {
        self.frozen_chunks.get(point)
    }

    pub fn get_frozen_chunk_containing(&self, point: &GridPoint) -> Option<&CompressedChunk<T>> {
        let origin = self.chunker.resolve_chunk_origin(point);
        self.get_frozen_chunk_at(&origin)
    }

    #[inline(always)]
    pub fn get_or_create_chunk_containing(&mut self, point: &GridPoint) -> &mut Chunk<T> {
        let origin = self.chunker.resolve_chunk_origin(point);
        if self.frozen_chunks.contains_key(&origin) {
            panic!("Chunk is frozen");
        }

        self.active_chunks.entry(origin).or_insert_with(|| {
            let bounds = self.chunker.resolve_chunk_bounds(point);
            Chunk::new(bounds)
        })
    }

    pub fn set_multiple(&mut self, indices: &[GridPoint], value: T) {
        if indices.is_empty() {
            return;
        }

        let mut last_chunk = self.get_or_create_chunk_containing(&indices[0]);
        for index in indices.iter() {
            if !last_chunk.contains_point(index) {
                last_chunk = self.get_or_create_chunk_containing(index);
            }
            last_chunk[*index] = value;
        }
    }
}

impl<T: Default + Clone + Copy> Index<GridPoint> for Grid<T> {
    type Output = T;

    fn index(&self, point: GridPoint) -> &Self::Output {
        if let Some(_chunk) = self.get_frozen_chunk_containing(&point) {
            // TODO: indexing into a compressed chunk, possibly with some cache
            // &chunk[point]
            panic!("Unimplemented");
        } else if let Some(chunk) = self.get_active_chunk_containing(&point) {
            &chunk[point]
        } else {
            panic!("Point out of bounds");
        }
    }
}

impl<T: Default + Clone + Copy> IndexMut<GridPoint> for Grid<T> {
    #[inline(always)]
    fn index_mut(&mut self, point: GridPoint) -> &mut Self::Output {
        let chunk: &mut Chunk<T> = self.get_or_create_chunk_containing(&point);
        &mut chunk[point]
    }
}

pub struct FrozenGrid<T> {
    chunker: Box<dyn Chunker>,
    frozen_chunks: BTreeMap<ChunkOrigin, CompressedChunk<T>>,
    memory_usage: usize,
}

impl<T: Send + Sync> From<Grid<T>> for FrozenGrid<T> {
    fn from(mut value: Grid<T>) -> FrozenGrid<T> {
        value.freeze_all();

        FrozenGrid {
            chunker: value.chunker,
            frozen_chunks: value.frozen_chunks,
            memory_usage: value.frozen_chunks_memory_usage,
        }
    }
}

impl<T> FrozenGrid<T> {
    pub fn memory_usage(&self) -> usize {
        self.memory_usage
    }

    pub fn chunk_count(&self) -> usize {
        self.frozen_chunks.len()
    }

    pub fn bounds(&self) -> GridRect {
        let mut min = GridPoint::new(0, 0);
        let mut max = GridPoint::new(0, 0);
        for chunk in self.frozen_chunks.values() {
            min.x = chunk.bounds().start().x.min(min.x);
            min.y = chunk.bounds().start().y.min(min.y);
            max.x = chunk.bounds().end().x.max(max.x);
            max.y = chunk.bounds().end().y.max(max.y);
        }
        GridRect::with_start_end(min, max)
    }
}

/// Intended for sampling the grid with small power of 2 minification factors due
/// to overall complexity being linear with the number of cells visited.
/// If higher minification is required consider an approach with pregenerated mip-maps
/// to avoid redundant work. If that's not possible at least consider using the optional
/// cache to avoid redundant computation between calls.
///
/// The sampled region must be aligned to minification factor.
/// Minification factor must be compatible with the chunk grid, otherwise the function panics.
/// Minification factors up to 64 are guaranteed to work, as that's the minimum
/// guaranteed chunk alignment.
/// `par_` functions utilize a rayon thread pool for parallelism.
///
/// DEV NOTE: There is a little bit of unsafe Array2D accesses
///           because it is around 30% faster overall.
pub struct FrozenGridSampler<'a, T, FZero, FReduce, FFinalize, TAcc, U> {
    grid: &'a FrozenGrid<T>,
    region: GridRect,
    minification: Pow2,
    default_value: U,
    fzero: FZero,
    freduce: FReduce,
    ffinalize: FFinalize,
    _marker: PhantomData<(TAcc, U)>,
}

impl<'a, T, FZero, FReduce, FFinalize, TAcc, U> CacheEnabled
    for FrozenGridSampler<'a, T, FZero, FReduce, FFinalize, TAcc, U>
{
    type CacheType = LockStepCache<Self::KeyType, Self::EntryType>;
    type KeyType = (ChunkOrigin, Pow2);
    type EntryType = Array2D<U>;

    fn make_cache(max_memory_cost: usize) -> Self::CacheType {
        Self::CacheType::new(max_memory_cost)
    }
}

impl<'a, T, FZero, FReduce, FFinalize, TAcc, U>
    FrozenGridSampler<'a, T, FZero, FReduce, FFinalize, TAcc, U>
where
    T: Default + Clone + Copy + Send + Sync,
    FZero: Fn() -> TAcc + Send + Sync,
    FReduce: Fn(&mut TAcc, T) + Send + Sync,
    FFinalize: Fn(TAcc, (usize, usize)) -> U + Send + Sync,
    TAcc: Send + Sync,
    U: Default + Clone + Copy + Send + Sync + 'static,
{
    pub fn new(
        grid: &'a FrozenGrid<T>,
        region: GridRect,
        default_value: U,
        fzero: FZero,
        freduce: FReduce,
        ffinalize: FFinalize,
    ) -> FrozenGridSampler<'a, T, FZero, FReduce, FFinalize, TAcc, U> {
        FrozenGridSampler {
            grid,
            region,
            minification: Pow2::new(1),
            default_value,
            fzero,
            freduce,
            ffinalize,
            _marker: Default::default(),
        }
    }

    pub fn new_with_minification(
        grid: &'a FrozenGrid<T>,
        region: GridRect,
        minification: Pow2,
        default_value: U,
        fzero: FZero,
        freduce: FReduce,
        ffinalize: FFinalize,
    ) -> FrozenGridSampler<'a, T, FZero, FReduce, FFinalize, TAcc, U> {
        if !region.is_aligned_to_pow2(minification) {
            panic!("Region is not aligned to the minification factor.");
        }

        if grid.chunker.minimum_chunk_alignment() < minification.into() {
            panic!("Minification factor is larger than minimum chunk alignment.");
        }

        if grid.chunker.minimum_chunk_extent() < minification.into() {
            panic!("Minification factor is smaller than minimum chunk extent.");
        }

        FrozenGridSampler {
            grid,
            region,
            minification,
            default_value,
            fzero,
            freduce,
            ffinalize,
            _marker: Default::default(),
        }
    }

    fn assemble_result(&self, rx: Receiver<(Arc<Array2D<U>>, Blit2D)>) -> Array2D<U> {
        let region = self.region;
        let minification = self.minification;
        let default_value = self.default_value;

        let mut result: Array2D<U> = Array2D::new(
            pow2::floor_div(region.width(), minification) as usize,
            pow2::floor_div(region.height(), minification) as usize,
        );
        result.as_flat_mut_slice().fill(default_value);

        while let Ok((whole_chunk_result, blit)) = rx.recv() {
            // SAFETY: The blit region comes from a trusted producer.
            unsafe {
                blit_array2d_unchecked(&whole_chunk_result, &mut result, &blit);
            }
        }

        result
    }

    /// # Safety
    ///
    /// The index range must be contained within the chunk.
    #[inline]
    unsafe fn collect_block(&self, chunk: &Chunk<T>, xs: Range<i32>, ys: Range<i32>) -> U {
        let width = xs.len();
        let height = ys.len();

        let mut acc = (self.fzero)();

        // Fill in the input block for the mapping function.
        for y in ys {
            for x in xs.clone() {
                // SAFETY: We are iterating a known existing chunk, as the subregion
                //         was computed based on its bounds.
                let val = unsafe { chunk.get_unchecked(GridPoint::new(x, y)) };

                (self.freduce)(&mut acc, *val);
            }
        }

        (self.ffinalize)(acc, (width, height))
    }

    pub fn par_sample_with_cache(&self, cache: &<Self as CacheEnabled>::CacheType) -> Array2D<U> {
        rayon::scope(|s| {
            let (tx, rx) = mpsc::channel::<(Arc<Array2D<U>>, Blit2D)>();

            s.spawn(|_| {
                self.grid
                    .chunker
                    .origins_of_intersecting_chunks(&self.region)
                    .into_par_iter()
                    .flat_map(|origin| self.grid.frozen_chunks.get(&origin))
                    .for_each(|compressed_chunk| {
                        let bounds = compressed_chunk.bounds();

                        // With the cache it's better if we do the whole chunk, because it's
                        // easier to reuse the result in the future.
                        let whole_chunk_result = cache.get_or_compute(
                            (compressed_chunk.origin(), self.minification),
                            || {
                                let chunk = compressed_chunk.decompress();

                                let blocks_x =
                                    pow2::floor_div(bounds.width(), self.minification) as usize;
                                let blocks_y =
                                    pow2::floor_div(bounds.width(), self.minification) as usize;
                                let block_size: i32 = self.minification.into();

                                let mut whole_chunk_result: Array2D<U> =
                                    Array2D::new(blocks_x, blocks_y);

                                for by in 0..blocks_y {
                                    for bx in 0..blocks_x {
                                        let cx = bounds.start.x + bx as i32 * block_size;
                                        let cy = bounds.start.y + by as i32 * block_size;

                                        // SAFETY: We are iterating within `bounds`,
                                        //         which are taken directly from the chunk.
                                        let v = unsafe {
                                            self.collect_block(
                                                &chunk,
                                                cx..cx + block_size,
                                                cy..cy + block_size,
                                            )
                                        };

                                        // SAFETY: Explicitly iterating within the subregion.
                                        unsafe {
                                            *whole_chunk_result.get_unchecked_mut(bx, by) = v;
                                        }
                                    }
                                }

                                let cost = whole_chunk_result.width()
                                    * whole_chunk_result.height()
                                    * size_of::<U>();
                                (whole_chunk_result, cost)
                            },
                        );

                        // Now figure out how much of the whole chunk we actually need and blit that.
                        let subregion = compressed_chunk
                            .bounds()
                            .intersection(&self.region)
                            .expect("Chunker should have returned only intersecting chunks.");

                        assert!(subregion.is_aligned_to_pow2(self.minification));

                        let dst_x = pow2::floor_div(
                            subregion.start.x - self.region.start.x,
                            self.minification,
                        ) as usize;
                        let dst_y = pow2::floor_div(
                            subregion.start.y - self.region.start.y,
                            self.minification,
                        ) as usize;

                        let src_x =
                            pow2::floor_div(subregion.start.x - bounds.start.x, self.minification)
                                as usize;
                        let src_y =
                            pow2::floor_div(subregion.start.y - bounds.start.y, self.minification)
                                as usize;

                        let width = pow2::floor_div(subregion.width(), self.minification) as usize;
                        let height =
                            pow2::floor_div(subregion.height(), self.minification) as usize;

                        tx.send((
                            whole_chunk_result.clone(),
                            Blit2D {
                                src_x,
                                src_y,
                                dst_x,
                                dst_y,
                                width,
                                height,
                            },
                        ))
                        .unwrap();
                    });
                drop(tx);
            });

            self.assemble_result(rx)
        })
    }
    pub fn par_sample(
        &self
    ) -> Array2D<U> {
        rayon::scope(|s| {
            let (tx, rx) = mpsc::channel::<(Arc<Array2D<U>>, Blit2D)>();

            s.spawn(|_| {
        self.grid.chunker
            .origins_of_intersecting_chunks(&self.region)
            .into_par_iter()
            .flat_map(|origin| self.grid.frozen_chunks.get(&origin))
            .for_each(|compressed_chunk| {
                let subregion = compressed_chunk
                    .bounds()
                    .intersection(&self.region)
                    .expect("Chunker should have returned only intersecting chunks.");

                assert!(subregion.is_aligned_to_pow2(self.minification));

                let chunk = compressed_chunk.decompress();

                let block_size: i32 = self.minification.into();

                let mut subregion_result: Array2D<U> = Array2D::new(
                    pow2::floor_div(subregion.width(), self.minification) as usize,
                    pow2::floor_div(subregion.height(), self.minification) as usize,
                );

                for by in (subregion.start.y..subregion.end.y).step_by(block_size as usize) {
                    for bx in (subregion.start.x..subregion.end.x).step_by(block_size as usize) {
                        // SAFETY: We are iterating within `bounds`,
                        //         which are taken directly from the chunk.
                        let v = unsafe {
                            self.collect_block(&chunk, bx..bx + block_size, by..by + block_size)
                        };

                        // Map the block and store into the actual result.
                        let srx = pow2::floor_div(bx - subregion.start.x, self.minification) as usize;
                        let sry = pow2::floor_div(by - subregion.start.y, self.minification) as usize;
                        // SAFETY: Explicitly iterating within the subregion.
                        unsafe {
                            *subregion_result.get_unchecked_mut(srx, sry) = v;
                        }
                    }
                }

                let dst_x = pow2::floor_div(subregion.start.x - self.region.start.x, self.minification) as usize;
                let dst_y = pow2::floor_div(subregion.start.y - self.region.start.y, self.minification) as usize;

                let width = subregion_result.width();
                let height = subregion_result.height();

                tx.send((Arc::new(subregion_result), Blit2D {
                    src_x: 0,
                    src_y: 0,
                    dst_x,
                    dst_y,
                    width,
                    height,
                })).unwrap();
            });
                drop(tx);
            });

            self.assemble_result(rx)
        })
    }
}

impl<T> WriteTo for FrozenGrid<T>
where
    T: WriteTo,
{
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        self.chunker.write_to(writer)?;
        self.frozen_chunks.write_to(writer)?;
        Ok(())
    }
}

impl<T> ReadFrom for FrozenGrid<T>
where
    T: ReadFrom,
{
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        let chunker = Box::<dyn Chunker>::read_from(reader)?;
        let frozen_chunks = BTreeMap::<ChunkOrigin, CompressedChunk<T>>::read_from(reader)?;

        // Validate chunk bounds and origin against the chunker.
        for (origin, chunk) in frozen_chunks.iter() {
            let chunker_provided_bounds = chunker.resolve_chunk_bounds(&chunk.bounds.start);
            if origin != &ChunkOrigin(chunker_provided_bounds.start)
                || chunker_provided_bounds != chunk.bounds
            {
                return Err(std::io::Error::new(
                    ErrorKind::InvalidData,
                    "Chunk bounds mismatch bounds provided by the chunker.",
                ));
            }
        }

        let memory_usage = frozen_chunks.values().map(|v| v.memory_usage()).sum();
        Ok(FrozenGrid {
            chunker,
            frozen_chunks,
            memory_usage,
        })
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

impl WriteTo for StandardChunker {
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        match self {
            StandardChunker::SquareChunker { chunk_size_pow2 } => {
                "SquareChunker".as_bytes().write_to(writer)?;
                chunk_size_pow2.write_to(writer)
            }
        }
    }
}

impl ReadFrom for StandardChunker {
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        let t = Box::<[u8]>::read_from(reader)?;
        if t.iter().eq("SquareChunker".as_bytes()) {
            let chunk_size_pow2 = u8::read_from(reader)?;
            let size = Pow2::from_exponent(chunk_size_pow2 as usize);
            StandardChunker::try_new_square_chunker(size).ok_or_else(|| {
                std::io::Error::new(
                    ErrorKind::InvalidData,
                    "Invalid chunk size for SquareChunker.",
                )
            })
        } else {
            Err(std::io::Error::new(
                ErrorKind::InvalidData,
                "Invalid chunker type.",
            ))
        }
    }
}

impl WriteTo for Box<dyn Chunker> {
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        let standard_chunker = self.as_standard_chunker().ok_or_else(|| {
            std::io::Error::new(
                ErrorKind::InvalidData,
                "Trying to write a non-standard Chunker.",
            )
        })?;
        standard_chunker.write_to(writer)
    }
}

impl ReadFrom for Box<dyn Chunker> {
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        let standard_chunker = StandardChunker::read_from(reader)?;
        Ok(standard_chunker.into_chunker())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compression::ZstdCompression;
    use std::panic::AssertUnwindSafe;

    fn point(x: i32, y: i32) -> GridPoint {
        GridPoint::new(x, y)
    }

    fn make_bounds(origin_x: i32, origin_y: i32, width: u32, height: u32) -> GridRect {
        GridRect::with_size(point(origin_x, origin_y), width as i32, height as i32)
    }

    fn make_grid(chunk_size: Pow2) -> Grid<i32> {
        Grid::new(
            Box::new(SquareChunker { size: chunk_size }),
            ZstdCompression::new_with_level(1).into(),
        )
    }

    #[test]
    fn chunk_new_initializes_with_default_values() {
        let bounds = make_bounds(0, 0, 64, 64);
        let chunk: Chunk<i32> = Chunk::new(bounds);

        for y in 0..64 {
            for x in 0..64 {
                assert_eq!(chunk[point(x, y)], 0);
            }
        }
    }

    #[test]
    fn chunk_index_and_index_mut_work() {
        let bounds = make_bounds(64, 64, 64, 64);
        let mut chunk: Chunk<i32> = Chunk::new(bounds);

        chunk[point(64 + 10, 64 + 20)] = 1;
        chunk[point(64 + 11, 64 + 20)] = 2;
        chunk[point(64 + 13, 64 + 23)] = 99;

        assert_eq!(chunk[point(64 + 10, 64 + 20)], 1);
        assert_eq!(chunk[point(64 + 11, 64 + 20)], 2);
        assert_eq!(chunk[point(64 + 13, 64 + 23)], 99);
    }

    #[test]
    fn square_chunker_resolves_positive_chunk_origins() {
        let chunker = SquareChunker {
            size: Pow2::new(64),
        };

        let origin = chunker.resolve_chunk_origin(&point(128 + 18, 192 + 33));

        assert_eq!(origin.0, point(128, 192));
    }

    #[test]
    fn square_chunker_resolves_negative_chunk_origins() {
        let chunker = SquareChunker {
            size: Pow2::new(64),
        };

        let origin = chunker.resolve_chunk_origin(&point(-128 + 1, -192 + 17));

        // arithmetic right shift should floor toward negative infinity
        assert_eq!(origin.0, point(-128, -192));
    }

    #[test]
    fn square_chunker_resolves_bounds() {
        let chunker = SquareChunker {
            size: Pow2::new(64),
        };

        let bounds = chunker.resolve_chunk_bounds(&point(64 + 9, 128 + 17));

        assert_eq!(bounds.start, point(64, 128));
        assert_eq!(bounds.width(), 64);
        assert_eq!(bounds.height(), 64);
    }

    #[test]
    fn grid_creates_chunk_on_mutation() {
        let mut grid = make_grid(Pow2::new(64));

        grid[point(1, 1)] = 42;

        let chunk = grid
            .get_active_chunk_containing(&point(1, 1))
            .expect("chunk should exist");

        assert_eq!(chunk[point(1, 1)], 42);
    }

    #[test]
    fn grid_returns_same_chunk_for_points_in_same_region() {
        let mut grid = make_grid(Pow2::new(64));

        grid[point(1, 1)] = 10;
        grid[point(3, 3)] = 20;

        let chunk_a = grid.get_active_chunk_containing(&point(1, 1)).unwrap() as *const _;
        let chunk_b = grid.get_active_chunk_containing(&point(3, 3)).unwrap() as *const _;

        assert_eq!(chunk_a, chunk_b);
    }

    #[test]
    fn grid_creates_different_chunks_for_different_regions() {
        let mut grid = make_grid(Pow2::new(64)); // chunk size = 4

        grid[point(1, 1)] = 10;
        grid[point(64 + 5, 5)] = 20;

        let chunk_a = grid.get_active_chunk_containing(&point(1, 1)).unwrap() as *const _;
        let chunk_b = grid.get_active_chunk_containing(&point(64 + 5, 5)).unwrap() as *const _;

        assert_ne!(chunk_a, chunk_b);
    }

    #[test]
    fn grid_index_panics_when_chunk_does_not_exist() {
        let grid = make_grid(Pow2::new(64));

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = grid[point(0, 0)];
        }));

        assert!(result.is_err());
    }

    #[test]
    fn grid_supports_negative_coordinates() {
        let mut grid = make_grid(Pow2::new(64));

        grid[point(-1, -1)] = 123;

        assert_eq!(grid[point(-1, -1)], 123);
    }

    #[test]
    fn correct_chunks_get_frozen() {
        let mut grid = make_grid(Pow2::new(64));

        grid[point(0, 0)] = 123;
        grid[point(-64 + 5, 0)] = 123;

        grid.freeze(&GridRect::with_size(GridPoint::new(-4, -4), 70, 70));

        assert!(grid.is_chunk_containing_frozen(&GridPoint::new(0, 0)));
        assert!(!grid.is_chunk_containing_frozen(&GridPoint::new(-64 + 5, 0)));
    }

    #[test]
    fn attempting_to_modify_frozen_chunk_panics() {
        let mut grid = make_grid(Pow2::new(64));

        grid[point(0, 0)] = 123;

        grid.freeze(&GridRect::with_size(GridPoint::new(-400, -400), 810, 810));

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            grid[point(0, 0)] = 123;
        }));

        assert!(result.is_err());
    }
}
