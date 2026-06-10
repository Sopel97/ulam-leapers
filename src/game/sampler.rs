use crate::collections::array2d::Array2D;
use crate::game::chunk::{BoundedChunk, Chunk, ChunkOrigin};
use crate::game::grid::FrozenGrid;
use crate::math::coords::GridPoint;
use crate::math::pow2;
use crate::math::pow2::Pow2;
use crate::math::rect::GridRect;
use crate::util::blit::{blit_array2d_unchecked, Blit2D};
use crate::util::cache::{CacheEnabled, LockStepCache};
use crate::util::cancel::{Canceled, CancellationToken};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{mpsc, Arc};

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

        if grid.chunker().minimum_chunk_alignment() < minification.into() {
            panic!("Minification factor is larger than minimum chunk alignment.");
        }

        if grid.chunker().minimum_chunk_extent() < minification.into() {
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
