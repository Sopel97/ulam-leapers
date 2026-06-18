use crate::collections::array2d::Array2D;
use crate::game::chunk::{BoundedChunk, Chunk, ChunkOrigin};
use crate::game::chunker::Chunker;
use crate::game::grid::FrozenGrid;
use crate::math::coords::GridPoint;
use crate::math::pow2;
use crate::math::pow2::Pow2;
use crate::math::rect::GridRect;
use crate::util::blit::{blit_array2d_unchecked, Blit2D};
use crate::util::cache::{CacheEnabled, LockStepCache};
use crate::util::cancel::{Canceled, CancellationToken};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{mpsc, Arc, Mutex, MutexGuard};

pub trait SampleCollector: Send + Sync {
    type InputType: Default + Clone + Copy + Send + Sync;
    type AccumulatorType: Send + Sync;
    type OutputType: Default + Clone + Copy + Send + Sync + 'static;

    fn zero(&self) -> Self::AccumulatorType;
    fn push(&self, acc: &mut Self::AccumulatorType, input: Self::InputType);
    fn finalize(&self, acc: Self::AccumulatorType, size: (usize, usize)) -> Self::OutputType;
}

struct FrozenGridCellAccessorCacheEntry<T> {
    chunk: Chunk<T>,
    last_access_gen: u64,
}

/// This structure provides access to individual cells. Up to 4 last accessed chunks are cached.
/// The intended use case is for sparse access for user interaction or other specific probes.
/// While this structure is safe for concurrent access it is NOT advised, as `get` blocks.
pub struct FrozenGridCellAccessor<T> {
    grid: Arc<FrozenGrid<T>>,
    chunk_cache: Mutex<BTreeMap<ChunkOrigin, FrozenGridCellAccessorCacheEntry<T>>>,
    access_gen: AtomicU64,
    max_cached_chunk_count: usize,
}

impl<T> FrozenGridCellAccessor<T>
where
    T: Default + Clone + Copy + Send + Sync,
{
    pub fn new(grid: Arc<FrozenGrid<T>>, max_cached_chunk_count: usize) -> Self {
        Self {
            grid,
            chunk_cache: Mutex::new(BTreeMap::new()),
            access_gen: AtomicU64::new(0),
            max_cached_chunk_count,
        }
    }

    pub fn get(&self, point: GridPoint) -> Option<T> {
        let current_gen = self.access_gen.fetch_add(1, Ordering::Relaxed);

        if let Some(desired_chunk) = self.grid.get_chunk_containing(&point) {
            // We can just hold it for the whole duration of `get` because we don't care about concurrency.
            let mut chunk_cache = self.chunk_cache.lock().unwrap();
            let desired_chunk_origin = desired_chunk.origin();
            if !chunk_cache.contains_key(&desired_chunk_origin) {
                let decompressed_chunk = desired_chunk.decompress();
                if chunk_cache.len() >= self.max_cached_chunk_count {
                    Self::purge_oldest_cache_entry(&mut chunk_cache);
                }

                chunk_cache.insert(
                    desired_chunk_origin,
                    FrozenGridCellAccessorCacheEntry {
                        chunk: decompressed_chunk,
                        last_access_gen: current_gen,
                    },
                );
            }

            Some(chunk_cache.get(&desired_chunk_origin)?.chunk[point])
        } else {
            None
        }
    }

    fn purge_oldest_cache_entry(
        cache: &mut MutexGuard<BTreeMap<ChunkOrigin, FrozenGridCellAccessorCacheEntry<T>>>,
    ) {
        if let Some(oldest_gen) = cache.values().map(|e| e.last_access_gen).min() {
            cache
                .extract_if(.., |_k, v| v.last_access_gen == oldest_gen)
                .take(1)
                .for_each(|_| {});
        }
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
            minification: Pow2::try_from(1).unwrap(),
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

        if grid.chunker().minimum_chunk_alignment() < minification.as_u64() as usize {
            panic!("Minification factor is larger than minimum chunk alignment.");
        }

        if grid.chunker().minimum_chunk_extent() < minification.as_u64() as usize {
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
            pow2::div_floor(region.width(), minification) as usize,
            pow2::div_floor(region.height(), minification) as usize,
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

    /// # Safety
    ///
    /// The subregion must be contained within the chunk and be aligned to the minification factor.
    unsafe fn collect_subregion(
        &self,
        chunk: &Chunk<T>,
        subregion: GridRect,
    ) -> Array2D<TCollector::OutputType> {
        assert!(subregion.is_aligned_to_pow2(self.minification));

        let block_count = subregion
            .extent()
            .map_coords(|c| pow2::div_floor(c, self.minification));
        let block_size: i32 = self.minification.as_u64() as i32;

        let mut subregion_result: Array2D<TCollector::OutputType> =
            Array2D::new(block_count.x as usize, block_count.y as usize);

        for by in 0..block_count.y {
            for bx in 0..block_count.x {
                let cx = subregion.start.x + bx * block_size;
                let cy = subregion.start.y + by * block_size;

                // SAFETY: We are iterating within `bounds`,
                //         which are taken directly from the chunk.
                let v =
                    unsafe { self.collect_block(chunk, cx..cx + block_size, cy..cy + block_size) };

                // SAFETY: Explicitly iterating within the subregion.
                unsafe {
                    *subregion_result.get_unchecked_mut(bx as usize, by as usize) = v;
                }
            }
        }

        subregion_result
    }

    pub fn par_sample_with_cache(
        &self,
        cache: &<Self as CacheEnabled>::CacheType,
    ) -> Array2D<TCollector::OutputType> {
        rayon::scope(|s| {
            let (tx, rx) = mpsc::channel::<(Arc<Array2D<TCollector::OutputType>>, Blit2D)>();

            s.spawn(|_| {
                self.grid
                    .chunker()
                    .origins_of_intersecting_chunks(&self.region)
                    .into_par_iter()
                    .flat_map(|origin| self.grid.get_chunk_at(&origin))
                    .for_each(|compressed_chunk| {
                        let bounds = compressed_chunk.bounds();

                        // With the cache it's better if we do the whole chunk, because it's
                        // easier to reuse the result in the future.
                        let whole_chunk_result = cache.get_or_compute(
                            (compressed_chunk.origin(), self.minification),
                            || {
                                let chunk = compressed_chunk.decompress();

                                // SAFETY: We are explicitly using the chunk's bounds.
                                let whole_chunk_result =
                                    unsafe { self.collect_subregion(&chunk, *bounds) };

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

                        let dst = (subregion.start - self.region.start)
                            .map_coords(|c| pow2::div_floor(c, self.minification));
                        let src = (subregion.start - bounds.start)
                            .map_coords(|c| pow2::div_floor(c, self.minification));
                        let size = subregion
                            .extent()
                            .map_coords(|c| pow2::div_floor(c, self.minification));

                        tx.send((
                            Arc::clone(&whole_chunk_result),
                            Blit2D {
                                src_x: src.x as usize,
                                src_y: src.y as usize,
                                dst_x: dst.x as usize,
                                dst_y: dst.y as usize,
                                width: size.x as usize,
                                height: size.y as usize,
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
                    .chunker()
                    .origins_of_intersecting_chunks(&self.region);

                let num_chunks_to_process = chunks_to_process.len();
                let num_finished_chunks = AtomicUsize::new(0);

                let _res = chunks_to_process
                    .into_par_iter()
                    .flat_map(|origin| self.grid.get_chunk_at(&origin))
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

                        // SAFETY: The subregion is an intersection of chunk bounds with another
                        //         rectangle, so we know that subregion is contained within chunk bounds.
                        let subregion_result = unsafe { self.collect_subregion(&chunk, subregion) };

                        let dst = (subregion.start - self.region.start)
                            .map_coords(|c| pow2::div_floor(c, self.minification));
                        let width = subregion_result.width();
                        let height = subregion_result.height();

                        tx.send((
                            Arc::new(subregion_result),
                            Blit2D {
                                src_x: 0,
                                src_y: 0,
                                dst_x: dst.x as usize,
                                dst_y: dst.y as usize,
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

#[cfg(test)]
mod tests {
    use crate::compression::zstd::ZstdCompression;
    use crate::compression::AnyCompression;
    use crate::game::chunker::StripChunker;
    use crate::game::grid::{FrozenGrid, Grid};
    use crate::game::sampler::{FrozenGridSampler, SampleCollector};
    use crate::math::coords::GridPoint;
    use crate::math::pow2::Pow2;
    use crate::math::rect::GridRect;
    use crate::util::cache::CacheEnabled;
    use std::panic::AssertUnwindSafe;

    fn point(x: i32, y: i32) -> GridPoint {
        GridPoint::new(x, y)
    }

    // Helper: create a FrozenGrid<u8> populated with a function of (x, y).
    fn make_frozen_grid(chunk_size: Pow2, points: &[(i32, i32, u8)]) -> FrozenGrid<u8> {
        let mut grid: Grid<u8> = Grid::new(StripChunker::with_strip_length_and_thickness(
            chunk_size, chunk_size,
        ));
        for &(x, y, v) in points {
            grid[point(x, y)] = v;
        }
        let compression: AnyCompression = ZstdCompression::new_with_level(1).into();
        grid.to_frozen_grid(&compression)
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
        let frozen = make_frozen_grid(Pow2::try_from(64).unwrap(), &[(0, 0, 42)]);
        let region = GridRect::with_size(point(0, 0), 1, 1);
        let result = sum_sampler(&frozen, region).par_sample();
        assert_eq!(result[(0, 0)], 42);
    }

    #[test]
    fn frozen_grid_sampler_default_value_for_missing_chunks() {
        // Region that has no data written into it — sampler should fill with default.
        let frozen = make_frozen_grid(Pow2::try_from(64).unwrap(), &[]);
        let region = GridRect::with_size(point(0, 0), 64, 64);
        let result = sum_sampler(&frozen, region).par_sample();
        // All cells in result should be the default (0u8).
        assert!(result.as_flat_slice().iter().all(|&v| v == 0));
    }

    #[test]
    fn frozen_grid_sampler_result_dimensions_match_region() {
        let frozen = make_frozen_grid(Pow2::try_from(64).unwrap(), &[(0, 0, 1)]);
        let region = GridRect::with_size(point(0, 0), 64, 128);
        let result = sum_sampler(&frozen, region).par_sample();
        assert_eq!(result.width(), 64);
        assert_eq!(result.height(), 128);
    }

    #[test]
    fn frozen_grid_sampler_reads_multiple_cells_in_one_chunk() {
        let frozen = make_frozen_grid(
            Pow2::try_from(64).unwrap(),
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
        let frozen = make_frozen_grid(Pow2::try_from(64).unwrap(), &[(0, 0, 7), (64, 0, 13)]);
        let region = GridRect::with_size(point(0, 0), 128, 64);
        let result = sum_sampler(&frozen, region).par_sample();
        assert_eq!(result[(0, 0)], 7);
        assert_eq!(result[(64, 0)], 13);
    }

    #[test]
    fn frozen_grid_sampler_works_with_negative_coordinates() {
        let frozen = make_frozen_grid(Pow2::try_from(64).unwrap(), &[(-1, -1, 55)]);
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
        let frozen = make_frozen_grid(Pow2::try_from(64).unwrap(), &points);
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
        let frozen = make_frozen_grid(Pow2::try_from(64).unwrap(), &[(0, 0, 1)]);
        let region = GridRect::with_size(point(0, 0), 64, 64);
        let result = avg_sampler(&frozen, region, Pow2::try_from(2).unwrap()).par_sample();
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
        let frozen = make_frozen_grid(Pow2::try_from(64).unwrap(), &points);
        let region = GridRect::with_size(point(0, 0), 64, 64);
        let result = avg_sampler(&frozen, region, Pow2::try_from(2).unwrap()).par_sample();
        assert!(result.as_flat_slice().iter().all(|&v| v == 100));
    }

    #[test]
    fn frozen_grid_sampler_4x_minification_single_block_sum() {
        // Fill a 4×4 block with value 1. With sum (not average) and 4× minification
        // the result is a 1×1 array whose only cell should be 16 (= 4×4 × 1).
        let points: Vec<(i32, i32, u8)> = (0..4)
            .flat_map(|y| (0..4i32).map(move |x| (x, y, 1u8)))
            .collect();
        let frozen = make_frozen_grid(Pow2::try_from(64).unwrap(), &points);
        let region = GridRect::with_size(point(0, 0), 64, 64);

        // Sum sampler with 4× minification (sums all cells, does not divide).
        let sampler = FrozenGridSampler::new_with_minification(
            &frozen,
            region,
            Pow2::try_from(4).unwrap(),
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
        let frozen = make_frozen_grid(Pow2::try_from(64).unwrap(), &points);
        let region = GridRect::with_size(point(0, 0), 64, 64);
        let sampler = FrozenGridSampler::new_with_minification(
            &frozen,
            region,
            Pow2::try_from(64).unwrap(),
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
        let frozen = make_frozen_grid(Pow2::try_from(64).unwrap(), &points);
        let region = GridRect::with_size(point(0, 0), 64, 64);

        type S<'a> = FrozenGridSampler<'a, u8, AverageCollector>;

        // Using function pointers so `CacheEnabled` has a concrete type.
        let sampler = S::<'_>::new_with_minification(
            &frozen,
            region,
            Pow2::try_from(2).unwrap(),
            0u8,
            AverageCollector,
        );

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
        let frozen = make_frozen_grid(Pow2::try_from(64).unwrap(), &[]);
        // Region starts at (1, 0), which is not aligned to minification factor 2.
        let region = GridRect::with_size(point(1, 0), 64, 64);

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            avg_sampler(&frozen, region, Pow2::try_from(2).unwrap());
        }));

        assert!(result.is_err());
    }
}
