use crate::collections::array2d::Array2D;
use crate::compression::{AnyCompression, Compression};
use crate::io::{ReadFrom, WriteTo};
use crate::math::coords::{GridPoint, Point2D, Vector2D};
use crate::math::pow2;
use crate::math::pow2::Pow2;
use crate::math::rect::{GridRect, Rect2D};
use crate::util::blit::{blit_array2d_unchecked, Blit2D};
use crate::util::cache::{CacheEnabled, LockStepCache};
use crate::util::cancel::{Canceled, CancellationToken};
use crate::util::memory::MemSize;
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::io::{ErrorKind, Read, Write};
use std::ops::{Index, IndexMut, Range};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{mpsc, Arc};
use crate::game::chunk::{BoundedChunk, Chunk, ChunkOrigin, CompressedChunk};
use crate::game::chunker::{Chunker, StandardChunker};

pub struct Grid<T> {
    chunker: Box<dyn Chunker + Send + Sync>,
    compression: AnyCompression,
    active_chunks: BTreeMap<ChunkOrigin, Chunk<T>>,
    frozen_chunks: BTreeMap<ChunkOrigin, CompressedChunk<T>>,
    frozen_chunks_memory_usage: MemSize, // to reduce the amount of redundant iteration over chunks
}

impl<T: Default + Send + Sync> Grid<T> {
    /// Freezes at most `n` chunks in the given `region`.
    /// Returns the number of chunks frozen.
    pub fn freeze_n(&mut self, region: &GridRect, n: usize) -> usize {
        // While amortized this function won't do much it may be called on many chunks
        // at the time. There is no good way to change that without possibly blowing
        // up memory usage due to the number of uncompressed chunks growing.
        let to_freeze = self
            .active_chunks
            .extract_if(.., |_origin, chunk| region.contains(chunk.bounds()))
            .take(n)
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
        let count = frozen.len();

        // Collecting to a vector is not great but should be fine. Other ways of converting
        // parallel processing to sequential are annoying.
        for (origin, chunk) in frozen {
            self.frozen_chunks_memory_usage += chunk.memory_usage();
            self.frozen_chunks.insert(origin, chunk);
        }

        count
    }

    /// Returns the number of chunks frozen.
    pub fn freeze(&mut self, region: &GridRect) -> usize {
        self.freeze_n(region, usize::MAX)
    }

    /// Returns the number of chunks frozen.
    pub fn freeze_all(&mut self) -> usize {
        // TODO: Remove this hack. We can't represent the full range properly.
        self.freeze(&GridRect::with_start_end(
            GridPoint::new(i32::MIN, i32::MIN),
            GridPoint::new(i32::MAX, i32::MAX),
        ))
    }
}

impl<T> Grid<T> {
    pub fn memory_usage(&self) -> MemSize {
        let s1: MemSize = self.active_chunks.values().map(|c| c.memory_usage()).sum();
        s1 + self.frozen_chunks_memory_usage
    }

    pub fn is_chunk_at_frozen(&self, origin: &ChunkOrigin) -> bool {
        self.frozen_chunks.contains_key(origin)
    }

    pub fn is_chunk_containing_frozen(&self, point: &GridPoint) -> bool {
        let origin = self.chunker.resolve_chunk_origin(point);
        self.is_chunk_at_frozen(&origin)
    }

    pub fn chunker(&self) -> &(dyn Chunker + Send + Sync) {
        self.chunker.as_ref()
    }
}

impl<T: Default + Clone + Copy> Grid<T> {
    pub fn new(chunker: Box<dyn Chunker + Send + Sync>, compression: AnyCompression) -> Self {
        Grid {
            chunker,
            compression,
            active_chunks: BTreeMap::new(),
            frozen_chunks: BTreeMap::new(),
            frozen_chunks_memory_usage: MemSize::ZERO,
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
    memory_usage: MemSize,
}

impl<T: Default + Send + Sync> From<Grid<T>> for FrozenGrid<T> {
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
    pub fn memory_usage(&self) -> MemSize {
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

pub trait SampleCollector: Send + Sync {
    type InputType: Default + Clone + Copy + Send + Sync;
    type AccumulatorType: Send + Sync;
    type OutputType: Default + Clone + Copy + Send + Sync + 'static;

    fn zero(&self) -> Self::AccumulatorType;
    fn push(&self, acc: &mut Self::AccumulatorType, input: Self::InputType);
    fn finalize(&self, acc: Self::AccumulatorType, size: (usize, usize)) -> Self::OutputType;
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
pub struct FrozenGridSampler<'a, T, TCollector>
where
    TCollector: SampleCollector<InputType = T>,
{
    grid: &'a FrozenGrid<T>,
    region: GridRect,
    minification: Pow2,
    default_value: TCollector::OutputType,
    collector: TCollector,
}

impl<'a, T, TCollector> CacheEnabled for FrozenGridSampler<'a, T, TCollector>
where
    TCollector: SampleCollector<InputType = T>,
{
    type CacheType = LockStepCache<Self::KeyType, Self::EntryType>;
    type KeyType = (ChunkOrigin, Pow2);
    type EntryType = Array2D<TCollector::OutputType>;

    fn make_cache(max_memory_cost: usize) -> Self::CacheType {
        Self::CacheType::new(max_memory_cost)
    }
}

impl<'a, T, TCollector> FrozenGridSampler<'a, T, TCollector>
where
    T: Default + Clone + Copy + Send + Sync,
    TCollector: SampleCollector<InputType = T>,
{
    pub fn new(
        grid: &'a FrozenGrid<T>,
        region: GridRect,
        default_value: TCollector::OutputType,
        collector: TCollector,
    ) -> Self {
        Self {
            grid,
            region,
            minification: Pow2::new(1),
            default_value,
            collector,
        }
    }

    pub fn new_with_minification(
        grid: &'a FrozenGrid<T>,
        region: GridRect,
        minification: Pow2,
        default_value: TCollector::OutputType,
        collector: TCollector,
    ) -> Self {
        if !region.is_aligned_to_pow2(minification) {
            panic!("Region is not aligned to the minification factor.");
        }

        if grid.chunker.minimum_chunk_alignment() < minification.into() {
            panic!("Minification factor is larger than minimum chunk alignment.");
        }

        if grid.chunker.minimum_chunk_extent() < minification.into() {
            panic!("Minification factor is smaller than minimum chunk extent.");
        }

        Self {
            grid,
            region,
            minification,
            default_value,
            collector,
        }
    }

    fn assemble_result(
        &self,
        rx: Receiver<(Arc<Array2D<TCollector::OutputType>>, Blit2D)>,
    ) -> Array2D<TCollector::OutputType> {
        let region = self.region;
        let minification = self.minification;
        let default_value = self.default_value;

        let mut result: Array2D<TCollector::OutputType> = Array2D::new(
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
    unsafe fn collect_block(
        &self,
        chunk: &Chunk<T>,
        xs: Range<i32>,
        ys: Range<i32>,
    ) -> TCollector::OutputType {
        let width = xs.len();
        let height = ys.len();

        let mut acc = self.collector.zero();

        // Fill in the input block for the mapping function.
        for y in ys {
            for x in xs.clone() {
                // SAFETY: We are iterating a known existing chunk, as the subregion
                //         was computed based on its bounds.
                let val = unsafe { chunk.get_unchecked(GridPoint::new(x, y)) };

                self.collector.push(&mut acc, *val);
            }
        }

        self.collector.finalize(acc, (width, height))
    }

    pub fn par_sample_with_cache(
        &self,
        cache: &<Self as CacheEnabled>::CacheType,
    ) -> Array2D<TCollector::OutputType> {
        rayon::scope(|s| {
            let (tx, rx) = mpsc::channel::<(Arc<Array2D<TCollector::OutputType>>, Blit2D)>();

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

                                let mut whole_chunk_result: Array2D<TCollector::OutputType> =
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
                                    * size_of::<TCollector::OutputType>();
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
                            Arc::clone(&whole_chunk_result),
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

    pub fn par_sample_cancellable<C>(
        &self,
        cancellation_token: CancellationToken,
        progress_callback: C,
    ) -> Option<Array2D<TCollector::OutputType>>
    where
        C: Fn(usize, usize) + Send + Sync,
    {
        rayon::scope(|s| {
            let (tx, rx) = mpsc::channel::<(Arc<Array2D<TCollector::OutputType>>, Blit2D)>();

            s.spawn(|_| {
                let chunks_to_process = self
                    .grid
                    .chunker
                    .origins_of_intersecting_chunks(&self.region);

                let num_chunks_to_process = chunks_to_process.len();
                let num_finished_chunks = AtomicUsize::new(0);

                let _res = chunks_to_process
                    .into_par_iter()
                    .flat_map(|origin| self.grid.frozen_chunks.get(&origin))
                    .try_for_each(|compressed_chunk| {
                        if cancellation_token.is_canceled() {
                            return Err(Canceled);
                        }

                        let subregion = compressed_chunk
                            .bounds()
                            .intersection(&self.region)
                            .expect("Chunker should have returned only intersecting chunks.");

                        assert!(subregion.is_aligned_to_pow2(self.minification));

                        let chunk = compressed_chunk.decompress();

                        let block_size: i32 = self.minification.into();

                        let mut subregion_result: Array2D<TCollector::OutputType> = Array2D::new(
                            pow2::floor_div(subregion.width(), self.minification) as usize,
                            pow2::floor_div(subregion.height(), self.minification) as usize,
                        );

                        for by in (subregion.start.y..subregion.end.y).step_by(block_size as usize)
                        {
                            for bx in
                                (subregion.start.x..subregion.end.x).step_by(block_size as usize)
                            {
                                // SAFETY: We are iterating within `bounds`,
                                //         which are taken directly from the chunk.
                                let v = unsafe {
                                    self.collect_block(
                                        &chunk,
                                        bx..bx + block_size,
                                        by..by + block_size,
                                    )
                                };

                                // Map the block and store into the actual result.
                                let srx = pow2::floor_div(bx - subregion.start.x, self.minification)
                                    as usize;
                                let sry = pow2::floor_div(by - subregion.start.y, self.minification)
                                    as usize;
                                // SAFETY: Explicitly iterating within the subregion.
                                unsafe {
                                    *subregion_result.get_unchecked_mut(srx, sry) = v;
                                }
                            }
                        }

                        let dst_x = pow2::floor_div(
                            subregion.start.x - self.region.start.x,
                            self.minification,
                        ) as usize;
                        let dst_y = pow2::floor_div(
                            subregion.start.y - self.region.start.y,
                            self.minification,
                        ) as usize;

                        let width = subregion_result.width();
                        let height = subregion_result.height();

                        tx.send((
                            Arc::new(subregion_result),
                            Blit2D {
                                src_x: 0,
                                src_y: 0,
                                dst_x,
                                dst_y,
                                width,
                                height,
                            },
                        ))
                        .unwrap();

                        progress_callback(
                            num_finished_chunks.fetch_add(1, Ordering::Relaxed),
                            num_chunks_to_process,
                        );

                        Ok(())
                    });

                drop(tx);
            });

            let res = self.assemble_result(rx);

            if cancellation_token.is_canceled() {
                None
            } else {
                Some(res)
            }
        })
    }

    pub fn par_sample(&self) -> Array2D<TCollector::OutputType> {
        let cancellation_token = CancellationToken::new();
        let callback = |_: usize, _: usize| {};
        self.par_sample_cancellable(cancellation_token, callback)
            .expect("This job should never be cancelled.")
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
            let chunker_provided_bounds = chunker.resolve_chunk_bounds(&chunk.bounds().start);
            if origin.point() != chunker_provided_bounds.start
                || chunker_provided_bounds != *chunk.bounds()
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
    use crate::compression::zstd::ZstdCompression;
    use std::panic::AssertUnwindSafe;
    use crate::game::chunker::SquareChunker;

    fn point(x: i32, y: i32) -> GridPoint {
        GridPoint::new(x, y)
    }

    fn make_bounds(origin_x: i32, origin_y: i32, width: u32, height: u32) -> GridRect {
        GridRect::with_size(point(origin_x, origin_y), width as i32, height as i32)
    }

    fn make_grid(chunk_size: Pow2) -> Grid<i32> {
        Grid::new(
            Box::new(SquareChunker::new(chunk_size)),
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

    // ----------------------------
    // Grid sampler tests
    // ----------------------------

    // Helper: create a FrozenGrid<u8> populated with a function of (x, y).
    fn make_frozen_grid(chunk_size: Pow2, points: &[(i32, i32, u8)]) -> FrozenGrid<u8> {
        let mut grid: Grid<u8> = Grid::new(
            Box::new(SquareChunker::new(chunk_size)),
            ZstdCompression::new_with_level(1).into(),
        );
        for &(x, y, v) in points {
            grid[point(x, y)] = v;
        }
        FrozenGrid::from(grid)
    }

    struct SumCollector;

    impl SampleCollector for SumCollector {
        type InputType = u8;
        type AccumulatorType = u8;
        type OutputType = u8;

        fn zero(&self) -> Self::AccumulatorType {
            0
        }

        fn push(&self, acc: &mut Self::AccumulatorType, input: Self::InputType) {
            *acc += input;
        }

        fn finalize(&self, acc: Self::AccumulatorType, _size: (usize, usize)) -> Self::OutputType {
            acc
        }
    }

    // Convenience: build a sum-based sampler (minification = 1 just reads each cell).
    #[allow(clippy::type_complexity)]
    fn sum_sampler(
        grid: &'_ FrozenGrid<u8>,
        region: GridRect,
    ) -> FrozenGridSampler<'_, u8, SumCollector> {
        FrozenGridSampler::new(grid, region, 0u8, SumCollector)
    }

    struct AverageCollector;

    impl SampleCollector for AverageCollector {
        type InputType = u8;
        type AccumulatorType = u64;
        type OutputType = u8;

        fn zero(&self) -> Self::AccumulatorType {
            0
        }

        fn push(&self, acc: &mut Self::AccumulatorType, input: Self::InputType) {
            *acc += input as u64
        }

        fn finalize(&self, acc: Self::AccumulatorType, (w, h): (usize, usize)) -> Self::OutputType {
            (acc / (w * h) as u64) as u8
        }
    }

    // Convenience: build an averaging sampler with a given minification factor.
    #[allow(clippy::type_complexity)]
    fn avg_sampler(
        grid: &'_ FrozenGrid<u8>,
        region: GridRect,
        minification: Pow2,
    ) -> FrozenGridSampler<'_, u8, AverageCollector> {
        FrozenGridSampler::new_with_minification(grid, region, minification, 0u8, AverageCollector)
    }

    #[test]
    fn frozen_grid_sampler_reads_single_cell_correctly() {
        let frozen = make_frozen_grid(Pow2::new(64), &[(0, 0, 42)]);
        let region = GridRect::with_size(point(0, 0), 1, 1);
        let result = sum_sampler(&frozen, region).par_sample();
        assert_eq!(result[(0, 0)], 42);
    }

    #[test]
    fn frozen_grid_sampler_default_value_for_missing_chunks() {
        // Region that has no data written into it — sampler should fill with default.
        let frozen = make_frozen_grid(Pow2::new(64), &[]);
        let region = GridRect::with_size(point(0, 0), 64, 64);
        let result = sum_sampler(&frozen, region).par_sample();
        // All cells in result should be the default (0u8).
        assert!(result.as_flat_slice().iter().all(|&v| v == 0));
    }

    #[test]
    fn frozen_grid_sampler_result_dimensions_match_region() {
        let frozen = make_frozen_grid(Pow2::new(64), &[(0, 0, 1)]);
        let region = GridRect::with_size(point(0, 0), 64, 128);
        let result = sum_sampler(&frozen, region).par_sample();
        assert_eq!(result.width(), 64);
        assert_eq!(result.height(), 128);
    }

    #[test]
    fn frozen_grid_sampler_reads_multiple_cells_in_one_chunk() {
        let frozen = make_frozen_grid(
            Pow2::new(64),
            &[(0, 0, 10), (1, 0, 20), (0, 1, 30), (1, 1, 40)],
        );
        let region = GridRect::with_size(point(0, 0), 64, 64);
        let result = sum_sampler(&frozen, region).par_sample();
        assert_eq!(result[(0, 0)], 10);
        assert_eq!(result[(1, 0)], 20);
        assert_eq!(result[(0, 1)], 30);
        assert_eq!(result[(1, 1)], 40);
    }

    #[test]
    fn frozen_grid_sampler_reads_across_multiple_chunks() {
        // chunk size 64: (0,0) and (64,0) are in different chunks.
        let frozen = make_frozen_grid(Pow2::new(64), &[(0, 0, 7), (64, 0, 13)]);
        let region = GridRect::with_size(point(0, 0), 128, 64);
        let result = sum_sampler(&frozen, region).par_sample();
        assert_eq!(result[(0, 0)], 7);
        assert_eq!(result[(64, 0)], 13);
    }

    #[test]
    fn frozen_grid_sampler_works_with_negative_coordinates() {
        let frozen = make_frozen_grid(Pow2::new(64), &[(-1, -1, 55)]);
        let region = GridRect::with_size(point(-64, -64), 64, 64);
        let result = sum_sampler(&frozen, region).par_sample();
        // (-1,-1) maps to offset (63, 63) inside the region starting at (-64,-64).
        assert_eq!(result[(63, 63)], 55);
    }

    #[test]
    fn frozen_grid_sampler_region_smaller_than_chunk() {
        // Write to a 64×64 chunk, but only sample a 4×4 sub-region.
        let mut points = Vec::new();
        for y in 0..64i32 {
            for x in 0..64i32 {
                points.push((x, y, ((x + y) % 256) as u8));
            }
        }
        let frozen = make_frozen_grid(Pow2::new(64), &points);
        let region = GridRect::with_size(point(4, 4), 4, 4);
        let result = sum_sampler(&frozen, region).par_sample();
        assert_eq!(result.width(), 4);
        assert_eq!(result.height(), 4);
        for y in 0..4i32 {
            for x in 0..4i32 {
                let expected = ((x + 4 + y + 4) % 256) as u8;
                assert_eq!(result[(x as usize, y as usize)], expected);
            }
        }
    }

    #[test]
    fn frozen_grid_sampler_2x_minification_output_dimensions() {
        let frozen = make_frozen_grid(Pow2::new(64), &[(0, 0, 1)]);
        let region = GridRect::with_size(point(0, 0), 64, 64);
        let result = avg_sampler(&frozen, region, Pow2::new(2)).par_sample();
        assert_eq!(result.width(), 32);
        assert_eq!(result.height(), 32);
    }

    #[test]
    fn frozen_grid_sampler_2x_minification_averages_2x2_blocks() {
        // Fill every cell in a 64×64 chunk with a known value, then average 2×2 blocks.
        // All values are 100, so all averages should be 100.
        let points: Vec<(i32, i32, u8)> = (0..64)
            .flat_map(|y| (0..64i32).map(move |x| (x, y, 100u8)))
            .collect();
        let frozen = make_frozen_grid(Pow2::new(64), &points);
        let region = GridRect::with_size(point(0, 0), 64, 64);
        let result = avg_sampler(&frozen, region, Pow2::new(2)).par_sample();
        assert!(result.as_flat_slice().iter().all(|&v| v == 100));
    }

    #[test]
    fn frozen_grid_sampler_4x_minification_single_block_sum() {
        // Fill a 4×4 block with value 1. With sum (not average) and 4× minification
        // the result is a 1×1 array whose only cell should be 16 (= 4×4 × 1).
        let points: Vec<(i32, i32, u8)> = (0..4)
            .flat_map(|y| (0..4i32).map(move |x| (x, y, 1u8)))
            .collect();
        let frozen = make_frozen_grid(Pow2::new(64), &points);
        let region = GridRect::with_size(point(0, 0), 64, 64);

        // Sum sampler with 4× minification (sums all cells, does not divide).
        let sampler = FrozenGridSampler::new_with_minification(
            &frozen,
            region,
            Pow2::new(4),
            0u8,
            SumCollector,
        );
        let result = sampler.par_sample();
        // The 4×4 block lives in output cell (0,0); all other blocks are zero.
        assert_eq!(result[(0, 0)], 16);
    }

    struct SaturatingSumCollector;

    impl SampleCollector for SaturatingSumCollector {
        type InputType = u8;
        type AccumulatorType = u64;
        type OutputType = u8;

        fn zero(&self) -> Self::AccumulatorType {
            0
        }

        fn push(&self, acc: &mut Self::AccumulatorType, input: Self::InputType) {
            *acc += input as u64
        }

        fn finalize(&self, acc: Self::AccumulatorType, _size: (usize, usize)) -> Self::OutputType {
            acc.min(255) as u8
        }
    }

    #[test]
    fn frozen_grid_sampler_minification_equals_chunk_size_single_output_cell() {
        // With minification == chunk_size the entire chunk collapses to one output cell.
        let points: Vec<(i32, i32, u8)> = (0..64)
            .flat_map(|y| (0..64i32).map(move |x| (x, y, 1u8)))
            .collect();
        let frozen = make_frozen_grid(Pow2::new(64), &points);
        let region = GridRect::with_size(point(0, 0), 64, 64);
        let sampler = FrozenGridSampler::new_with_minification(
            &frozen,
            region,
            Pow2::new(64),
            0u8,
            SaturatingSumCollector,
        );
        let result = sampler.par_sample();
        assert_eq!(result.width(), 1);
        assert_eq!(result.height(), 1);
        // sum of 64×64 ones = 4096 → saturates to 255
        assert_eq!(result[(0, 0)], 255);
    }

    // ---------------------------------------------------------------------------
    // Caching
    // ---------------------------------------------------------------------------

    #[test]
    fn frozen_grid_sampler_with_cache_produces_same_result_as_without() {
        let points: Vec<(i32, i32, u8)> = (0..64)
            .flat_map(|y| (0..64i32).map(move |x| (x, y, ((x * 3 + y * 7) % 256) as u8)))
            .collect();
        let frozen = make_frozen_grid(Pow2::new(64), &points);
        let region = GridRect::with_size(point(0, 0), 64, 64);

        type S<'a> = FrozenGridSampler<'a, u8, AverageCollector>;

        // Using function pointers so `CacheEnabled` has a concrete type.
        let sampler =
            S::<'_>::new_with_minification(&frozen, region, Pow2::new(2), 0u8, AverageCollector);

        let no_cache_result = sampler.par_sample();

        let cache = S::make_cache(64 * 1024 * 1024);
        let cached_result = sampler.par_sample_with_cache(&cache);
        // Second call should hit the cache.
        let cached_result2 = sampler.par_sample_with_cache(&cache);

        assert_eq!(
            no_cache_result.as_flat_slice(),
            cached_result.as_flat_slice()
        );
        assert_eq!(
            no_cache_result.as_flat_slice(),
            cached_result2.as_flat_slice()
        );
    }

    #[test]
    fn frozen_grid_sampler_panics_when_region_not_aligned_to_minification() {
        let frozen = make_frozen_grid(Pow2::new(64), &[]);
        // Region starts at (1, 0), which is not aligned to minification factor 2.
        let region = GridRect::with_size(point(1, 0), 64, 64);

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            avg_sampler(&frozen, region, Pow2::new(2));
        }));

        assert!(result.is_err());
    }
}
