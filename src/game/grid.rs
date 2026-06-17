use crate::compression::zstd::ZstdCompression;
use crate::compression::AnyCompression;
use crate::game::chunk::{BoundedChunk, Chunk, ChunkOrigin, CompressedChunk};
use crate::game::chunker::{Chunker, StripChunker};
use crate::game::persist::uls::{UlsChunk, UlsChunker};
use crate::game::simulation::PlayerId;
use crate::math::coords::GridPoint;
use crate::math::rect::GridRect;
use crate::util::memory::MemSize;
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::ops::{Index, IndexMut};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

pub struct Grid<T> {
    chunker: Box<dyn Chunker + Send + Sync>,
    active_chunks: BTreeMap<ChunkOrigin, Chunk<T>>,
    frozen_chunks: BTreeMap<ChunkOrigin, CompressedChunk<T>>,
    frozen_chunks_memory_usage: MemSize, // to reduce the amount of redundant iteration over chunks
}

impl<T: Default + Send + Sync> Grid<T> {
    fn freeze_chunks<F>(&mut self, extract_fn: F, compression: &AnyCompression) -> usize
    where
        F: FnOnce(&mut BTreeMap<ChunkOrigin, Chunk<T>>) -> Vec<(ChunkOrigin, Chunk<T>)>,
    {
        let to_freeze = extract_fn(&mut self.active_chunks);

        let frozen = to_freeze
            .into_par_iter()
            .map(|entry| {
                let origin = entry.0;
                let chunk = entry.1;
                (origin, chunk.compress(compression))
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

    /// Freezes at most `n` chunks in the given `region`.
    /// Returns the number of chunks frozen.
    pub fn freeze_at_most_n_chunks_in_region(
        &mut self,
        region: &GridRect,
        n: usize,
        compression: &AnyCompression,
    ) -> usize {
        // No better way than O(n) scan I'm afraid, but it should be fine in most uses.
        self.freeze_chunks(
            |active_chunks| {
                active_chunks
                    .extract_if(.., |_origin, chunk| region.contains(chunk.bounds()))
                    .take(n)
                    .collect::<Vec<_>>()
            },
            compression,
        )
    }

    /// Returns the number of chunks frozen.
    pub fn freeze_chunks_in_region(
        &mut self,
        region: &GridRect,
        compression: &AnyCompression,
    ) -> usize {
        self.freeze_at_most_n_chunks_in_region(region, usize::MAX, compression)
    }

    /// Returns the number of chunks frozen.
    pub fn freeze_all(&mut self, compression: &AnyCompression) -> usize {
        self.freeze_chunks(
            |active_chunks| std::mem::take(active_chunks).into_iter().collect(),
            compression,
        )
    }

    pub fn to_frozen_grid(mut self, compression: &AnyCompression) -> FrozenGrid<T> {
        self.freeze_all(compression);

        FrozenGrid {
            frozen_chunks: self.frozen_chunks,
            memory_usage: self.frozen_chunks_memory_usage,
            chunker: self.chunker,
        }
    }
}

impl<T: Default + Clone + Copy + Send + Sync> Grid<T> {
    fn unfreeze_chunks<ExtractF, CallbackF>(
        &mut self,
        extract_fn: ExtractF,
        progress_callback: CallbackF,
    ) -> usize
    where
        ExtractF: FnOnce(
            &mut BTreeMap<ChunkOrigin, CompressedChunk<T>>,
        ) -> Vec<(ChunkOrigin, CompressedChunk<T>)>,
        CallbackF: Fn(usize, usize) + Send + Sync,
    {
        let to_unfreeze = extract_fn(&mut self.frozen_chunks);

        for (_, chunk) in &to_unfreeze {
            self.frozen_chunks_memory_usage -= chunk.memory_usage();
        }

        let to_unfreeze_count = to_unfreeze.len();
        let unfrozen_counter = Arc::new(AtomicUsize::new(0));

        let unfrozen = to_unfreeze
            .into_par_iter()
            .map(|entry| {
                let origin = entry.0;
                let chunk = entry.1;
                let res = (origin, chunk.decompress());
                let i = unfrozen_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                progress_callback(i, to_unfreeze_count);
                res
            })
            .collect::<Vec<_>>();
        let count = unfrozen.len();

        // Collecting to a vector is not great but should be fine. Other ways of converting
        // parallel processing to sequential are annoying.
        self.active_chunks.extend(unfrozen);

        count
    }

    pub fn unfreeze_chunks_not_in_region<F>(
        &mut self,
        region: &GridRect,
        progress_callback: F,
    ) -> usize
    where
        F: Fn(usize, usize) + Send + Sync,
    {
        self.unfreeze_chunks(
            |frozen_chunks| {
                frozen_chunks
                    .extract_if(.., |_origin, chunk| !region.contains(chunk.bounds()))
                    .collect::<Vec<_>>()
            },
            progress_callback,
        )
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

    pub fn chunker(&self) -> &dyn Chunker {
        self.chunker.as_ref()
    }

    pub fn active_chunks(&self) -> &BTreeMap<ChunkOrigin, Chunk<T>> {
        &self.active_chunks
    }
}

impl<T: Default + Clone + Copy> Grid<T> {
    pub fn new(chunker: Box<dyn Chunker + Send + Sync>) -> Self {
        Grid {
            chunker,
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
        if let Some(chunk) = self.get_frozen_chunk_containing(&point) {
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

    pub fn iter_chunks(&self) -> impl Iterator<Item = &CompressedChunk<T>> {
        self.frozen_chunks.values()
    }

    pub fn get_chunk_at(&self, origin: &ChunkOrigin) -> Option<&CompressedChunk<T>> {
        self.frozen_chunks.get(origin)
    }

    pub fn get_chunk_containing(&self, point: &GridPoint) -> Option<&CompressedChunk<T>> {
        let origin = self.chunker.resolve_chunk_origin(point);
        self.get_chunk_at(&origin)
    }

    pub fn chunker(&self) -> &dyn Chunker {
        self.chunker.as_ref()
    }
}

impl<T> From<FrozenGrid<T>> for Grid<T> {
    fn from(grid: FrozenGrid<T>) -> Self {
        Self {
            frozen_chunks: grid.frozen_chunks,
            frozen_chunks_memory_usage: grid.memory_usage,
            chunker: grid.chunker,
            active_chunks: BTreeMap::new(),
        }
    }
}

impl FrozenGrid<PlayerId> {
    pub fn from_uls(uls_chunker: UlsChunker, uls_chunks: Vec<UlsChunk>) -> Self {
        let chunker = StripChunker::from(uls_chunker);
        let frozen_chunks: BTreeMap<_, _> = uls_chunks
            .into_iter()
            .map(|chunk| {
                let origin = ChunkOrigin::new(GridPoint::new(chunk.origin_x, chunk.origin_y));
                (
                    origin,
                    CompressedChunk::<PlayerId>::from_uls(chunk, &chunker),
                )
            })
            .collect();

        let memory_usage = frozen_chunks
            .values()
            .map(|chunk| chunk.memory_usage())
            .sum();

        Self {
            chunker: Box::new(chunker),
            frozen_chunks,
            memory_usage,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compression::zstd::ZstdCompression;
    use crate::game::chunker::SquareChunker;
    use crate::math::pow2::Pow2;
    use std::panic::AssertUnwindSafe;

    fn point(x: i32, y: i32) -> GridPoint {
        GridPoint::new(x, y)
    }

    fn make_bounds(origin_x: i32, origin_y: i32, width: u32, height: u32) -> GridRect {
        GridRect::with_size(point(origin_x, origin_y), width as i32, height as i32)
    }

    fn make_compression() -> AnyCompression {
        ZstdCompression::new_with_level(1).into()
    }

    fn make_grid(chunk_size: Pow2) -> Grid<i32> {
        Grid::new(Box::new(SquareChunker::new(chunk_size)))
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
        let mut grid = make_grid(Pow2::try_from(64).unwrap());

        grid[point(1, 1)] = 42;

        let chunk = grid
            .get_active_chunk_containing(&point(1, 1))
            .expect("chunk should exist");

        assert_eq!(chunk[point(1, 1)], 42);
    }

    #[test]
    fn grid_returns_same_chunk_for_points_in_same_region() {
        let mut grid = make_grid(Pow2::try_from(64).unwrap());

        grid[point(1, 1)] = 10;
        grid[point(3, 3)] = 20;

        let chunk_a = grid.get_active_chunk_containing(&point(1, 1)).unwrap() as *const _;
        let chunk_b = grid.get_active_chunk_containing(&point(3, 3)).unwrap() as *const _;

        assert_eq!(chunk_a, chunk_b);
    }

    #[test]
    fn grid_creates_different_chunks_for_different_regions() {
        let mut grid = make_grid(Pow2::try_from(64).unwrap()); // chunk size = 4

        grid[point(1, 1)] = 10;
        grid[point(64 + 5, 5)] = 20;

        let chunk_a = grid.get_active_chunk_containing(&point(1, 1)).unwrap() as *const _;
        let chunk_b = grid.get_active_chunk_containing(&point(64 + 5, 5)).unwrap() as *const _;

        assert_ne!(chunk_a, chunk_b);
    }

    #[test]
    fn grid_index_panics_when_chunk_does_not_exist() {
        let grid = make_grid(Pow2::try_from(64).unwrap());

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = grid[point(0, 0)];
        }));

        assert!(result.is_err());
    }

    #[test]
    fn grid_supports_negative_coordinates() {
        let mut grid = make_grid(Pow2::try_from(64).unwrap());

        grid[point(-1, -1)] = 123;

        assert_eq!(grid[point(-1, -1)], 123);
    }

    #[test]
    fn correct_chunks_get_frozen() {
        let mut grid = make_grid(Pow2::try_from(64).unwrap());

        grid[point(0, 0)] = 123;
        grid[point(-64 + 5, 0)] = 123;

        grid.freeze_chunks_in_region(
            &GridRect::with_size(GridPoint::new(-4, -4), 70, 70),
            &make_compression(),
        );

        assert!(grid.is_chunk_containing_frozen(&GridPoint::new(0, 0)));
        assert!(!grid.is_chunk_containing_frozen(&GridPoint::new(-64 + 5, 0)));
    }

    #[test]
    fn attempting_to_modify_frozen_chunk_panics() {
        let mut grid = make_grid(Pow2::try_from(64).unwrap());

        grid[point(0, 0)] = 123;

        grid.freeze_chunks_in_region(
            &GridRect::with_size(GridPoint::new(-400, -400), 810, 810),
            &make_compression(),
        );

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            grid[point(0, 0)] = 123;
        }));

        assert!(result.is_err());
    }
}
