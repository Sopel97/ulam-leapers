use crate::collections::array2d::{Array2D, Slice2D};
use crate::coords::{Point2D, Rect2D, Vector2D};
use crate::util::align::CACHE_LINE_SIZE;
use crate::util::memory::{as_bytes, as_bytes_mut};
use crate::util::pow2;
use crate::util::pow2::{Pow2, floor_to_multiple};
use std::collections::BTreeMap;
use std::ops::{Index, IndexMut};

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
}

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

    fn index(&self, index: GridPoint) -> &Self::Output {
        let xx = index.x - self.bounds.start.x;
        let yy = index.y - self.bounds.start.y;
        &self.cells[(xx as usize, yy as usize)]
    }
}

impl<T: Default> IndexMut<GridPoint> for Chunk<T> {
    fn index_mut(&mut self, index: GridPoint) -> &mut Self::Output {
        let xx = index.x - self.bounds.start.x;
        let yy = index.y - self.bounds.start.y;
        &mut self.cells[(xx as usize, yy as usize)]
    }
}

// Generic over T because we want to preserve type information of the underlying data.
pub struct CompressedChunk<T> {
    bounds: GridRect,
    data: Box<[u8]>,
    _marker: std::marker::PhantomData<T>,
}

impl<T> From<&Chunk<T>> for CompressedChunk<T> {
    fn from(chunk: &Chunk<T>) -> Self {
        // We might try Morton transform and bit-transposition later,
        // but for now zstd can reduce the size to pretty much zero for the test-case.
        // We use default level 3 compression because higher levels don't seem to have an impact.
        let raw_uncompressed = as_bytes(chunk.cells.as_flat_slice());
        let compressed = zstd::encode_all(raw_uncompressed, 3)
            .unwrap()
            .into_boxed_slice();
        CompressedChunk {
            bounds: chunk.bounds,
            data: compressed,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T: Default + Clone + Copy> From<&CompressedChunk<T>> for Chunk<T> {
    fn from(chunk: &CompressedChunk<T>) -> Self {
        let mut cells: Array2D<T> = Array2D::new_aligned(
            chunk.bounds.width() as usize,
            chunk.bounds.height() as usize,
            CACHE_LINE_SIZE,
        );
        let raw_cells = as_bytes_mut(cells.as_flat_mut_slice());
        zstd::bulk::decompress_to_buffer(chunk.data.iter().as_slice(), raw_cells).unwrap();
        Chunk {
            bounds: chunk.bounds,
            cells,
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

pub trait Chunker {
    fn resolve_chunk_origin(&self, _: &GridPoint) -> ChunkOrigin;
    fn resolve_chunk_bounds(&self, _: &GridPoint) -> GridRect;
    fn origins_of_intersecting_chunks(&self, region: &GridRect) -> Vec<ChunkOrigin>;
    fn minimum_chunk_alignment(&self) -> usize;
    fn minimum_chunk_extent(&self) -> usize;
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
        let cx = pow2::floor_to_multiple(x, self.size);
        let cy = pow2::floor_to_multiple(y, self.size);

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
}

pub struct Grid<T> {
    chunker: Box<dyn Chunker + Send>,
    active_chunks: BTreeMap<ChunkOrigin, Chunk<T>>,
    frozen_chunks: BTreeMap<ChunkOrigin, CompressedChunk<T>>,
    frozen_chunks_memory_usage: usize, // to reduce the amount of redundant iteration over chunks
}

impl<T> Grid<T> {
    pub fn memory_usage(&self) -> usize {
        let s1: usize = self.active_chunks.values().map(|c| c.memory_usage()).sum();
        s1 + self.frozen_chunks_memory_usage
    }

    pub fn freeze(&mut self, region: &GridRect) {
        let to_freeze = self
            .active_chunks
            .extract_if(.., |_origin, chunk| region.contains(chunk.bounds()));
        let frozen = to_freeze.map(|entry| {
            let origin = entry.0;
            let chunk = entry.1;
            (origin, CompressedChunk::from(&chunk))
        });
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

    pub fn is_chunk_at_frozen(&self, origin: &ChunkOrigin) -> bool {
        self.frozen_chunks.contains_key(origin)
    }

    pub fn is_chunk_containing_frozen(&self, point: &GridPoint) -> bool {
        let origin = self.chunker.resolve_chunk_origin(point);
        self.is_chunk_at_frozen(&origin)
    }
}

impl<T: Default + Clone + Copy> Grid<T> {
    pub fn new(chunker: Box<dyn Chunker + Send>) -> Self {
        Grid {
            chunker,
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

    pub fn set_multiple(&mut self, indices: &Vec<GridPoint>, value: T) {
        if indices.is_empty() {
            return;
        }

        let mut last_chunk = self.get_or_create_chunk_containing(&indices[0]);
        for index in indices.iter() {
            if last_chunk.contains_point(index) {
                last_chunk[*index] = value;
            } else {
                last_chunk = self.get_or_create_chunk_containing(index);
                last_chunk[*index] = value;
            }
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
    fn index_mut(&mut self, point: GridPoint) -> &mut Self::Output {
        let chunk: &mut Chunk<T> = self.get_or_create_chunk_containing(&point);
        &mut chunk[point]
    }
}

pub struct FrozenGrid<T> {
    chunker: Box<dyn Chunker + Send>,
    frozen_chunks: BTreeMap<ChunkOrigin, CompressedChunk<T>>,
    memory_usage: usize,
}

impl<T> Into<FrozenGrid<T>> for Grid<T> {
    fn into(mut self) -> FrozenGrid<T> {
        self.freeze_all();

        FrozenGrid {
            chunker: self.chunker,
            frozen_chunks: self.frozen_chunks,
            memory_usage: self.frozen_chunks_memory_usage,
        }
    }
}

impl<T> FrozenGrid<T> {
    pub fn memory_usage(&self) -> usize {
        self.memory_usage
    }
}

impl<T: Default + Clone + Copy> FrozenGrid<T> {
    // Intended for small power of 2 factors due to overall complexity. If higher minification 
    // is required consider an approach with pregenerated mip-maps. While not viable for real-time 
    // updates it's still somewhat interactive for minification factors <=8 on typical resolutions. 
    // Region must be aligned to minification factor.
    // Minification factor must be compatible with the chunk grid, otherwise the function panics.
    pub fn sample_range2d_small_zoom_out_map<F, U>(
        &self,
        region: &GridRect,
        minification: Pow2,
        func: F,
    ) -> Array2D<U>
    where
        F: Fn(&Slice2D<T>) -> U,
        U: Default + Clone + Copy,
    {
        if pow2::floor_mod(region.start.x, minification) != 0
            || pow2::floor_mod(region.start.y, minification) != 0
            || pow2::floor_mod(region.end.x, minification) != 0
            || pow2::floor_mod(region.end.y, minification) != 0
        {
            panic!("Region is not aligned to the minification factor.");
        }

        if self.chunker.minimum_chunk_alignment() < minification.into() {
            panic!("Minification factor is larger than minimum chunk alignment.");
        }

        if self.chunker.minimum_chunk_extent() < minification.into() {
            panic!("Minification factor is smaller than minimum chunk extent.");
        }

        let mut result: Array2D<U> = Array2D::new(
            pow2::floor_div(region.width(), minification) as usize,
            pow2::floor_div(region.height(), minification) as usize,
        );

        let mut buffer: Array2D<T> = Array2D::new(
            minification.into(),
            minification.into(),
        );

        self.chunker
            .origins_of_intersecting_chunks(region)
            .into_iter()
            .flat_map(|origin| self.frozen_chunks.get(&origin))
            .for_each(|compressed_chunk| {
                let subregion = compressed_chunk
                    .bounds()
                    .intersection(region)
                    .expect("Chunker should have returned only intersecting chunks.");

                assert_eq!(pow2::floor_mod(subregion.start.x, minification), 0);
                assert_eq!(pow2::floor_mod(subregion.start.y, minification), 0);
                assert_eq!(pow2::floor_mod(subregion.end.x, minification), 0);
                assert_eq!(pow2::floor_mod(subregion.end.y, minification), 0);

                let chunk = Chunk::from(compressed_chunk);

                let block_size: i32 = minification.into();

                for by in (subregion.start.y..subregion.end.y).step_by(block_size as usize) {
                    for bx in (subregion.start.x..subregion.end.x).step_by(block_size as usize) {
                        // Fill in the input block for the mapping function.
                        for y in by..by+block_size {
                            for x in bx..bx+block_size {
                                let val = chunk[GridPoint::new(x, y)];

                                let dx = (x - bx) as usize;
                                let dy = (y - by) as usize;
                                buffer[(dx, dy)] = val;
                            }
                        }

                        // Map the block and store into the actual result.
                        let rx = pow2::floor_div(bx - region.start.x, minification) as usize;
                        let ry = pow2::floor_div(by - region.start.y, minification) as usize;
                        result[(rx, ry)] = func(&buffer.as_slice2d());
                    }
                }
            });

        result
    }

    pub fn sample_range2d_map<F, U>(&self, region: &GridRect, func: F) -> Array2D<U>
    where
        F: Fn(&T) -> U,
        U: Default + Clone + Copy,
    {
        let mut result: Array2D<U> =
            Array2D::new(region.width() as usize, region.height() as usize);

        self.chunker
            .origins_of_intersecting_chunks(region)
            .into_iter()
            .flat_map(|origin| self.frozen_chunks.get(&origin))
            .for_each(|compressed_chunk| {
                let subregion = compressed_chunk
                    .bounds()
                    .intersection(region)
                    .expect("Chunker should have returned only intersecting chunks.");

                let chunk = Chunk::from(compressed_chunk);

                for y in subregion.start.y..subregion.end.y {
                    for x in subregion.start.x..subregion.end.x {
                        let val = chunk[GridPoint::new(x, y)];

                        let dx = (x - region.start.x) as usize;
                        let dy = (y - region.start.y) as usize;
                        result[(dx, dy)] = func(&val);
                    }
                }
            });

        result
    }

    pub fn sample_range2d(&self, region: &GridRect) -> Array2D<T> {
        // This function samples every cell, so we don't have to do any interpolation or translation.
        // This also means that samples don't span multiple chunks, which allows us to make a simple
        // implementation that always keeps at most 1 chunk decompressed while having each chunk
        // decompressed exactly once.
        //
        // TODO NOTES:
        // With larger, zoomed, translated, interpolated samples it will get more problematic and
        // some tradeoffs will have to be made. Might be best to go in stripes with a chunk cache
        // and deallocate past decompressed chunks after each stripe
        // (or 2D sample-chunks at some granularity).
        //
        // For samples spanning a significant portion of the grid it might be worthwhile
        // to implement a precomputed mip-mapped view of every chunk and sample from that.
        // With each mip-map compressed separately it should be manageable and fast.
        //
        // We could also even explore just completely remapping (to RGB) and mip-mapping on load
        // and using that for all samples, or even drawing directly as textures.

        self.sample_range2d_map(region, |x| *x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::AssertUnwindSafe;

    fn point(x: i32, y: i32) -> GridPoint {
        GridPoint::new(x, y)
    }

    fn make_bounds(origin_x: i32, origin_y: i32, width: u32, height: u32) -> GridRect {
        GridRect::with_size(point(origin_x, origin_y), width as i32, height as i32)
    }

    fn make_grid(chunk_size: Pow2) -> Grid<i32> {
        Grid::new(Box::new(SquareChunker { size: chunk_size }))
    }

    #[test]
    fn chunk_new_initializes_with_default_values() {
        let bounds = make_bounds(0, 0, 4, 4);
        let chunk: Chunk<i32> = Chunk::new(bounds);

        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(chunk[point(x, y)], 0);
            }
        }
    }

    #[test]
    fn chunk_index_and_index_mut_work() {
        let bounds = make_bounds(10, 20, 4, 4);
        let mut chunk: Chunk<i32> = Chunk::new(bounds);

        chunk[point(10, 20)] = 1;
        chunk[point(11, 20)] = 2;
        chunk[point(13, 23)] = 99;

        assert_eq!(chunk[point(10, 20)], 1);
        assert_eq!(chunk[point(11, 20)], 2);
        assert_eq!(chunk[point(13, 23)], 99);
    }

    #[test]
    fn square_chunker_resolves_positive_chunk_origins() {
        let chunker = SquareChunker {
            size: Pow2::new(16),
        };

        let origin = chunker.resolve_chunk_origin(&point(18, 33));

        assert_eq!(origin.0, point(16, 32));
    }

    #[test]
    fn square_chunker_resolves_negative_chunk_origins() {
        let chunker = SquareChunker {
            size: Pow2::new(16),
        };

        let origin = chunker.resolve_chunk_origin(&point(-1, -17));

        // arithmetic right shift should floor toward negative infinity
        assert_eq!(origin.0, point(-16, -32));
    }

    #[test]
    fn square_chunker_resolves_bounds() {
        let chunker = SquareChunker { size: Pow2::new(8) };

        let bounds = chunker.resolve_chunk_bounds(&point(9, 17));

        assert_eq!(bounds.start, point(8, 16));
        assert_eq!(bounds.width(), 8);
        assert_eq!(bounds.height(), 8);
    }

    #[test]
    fn grid_creates_chunk_on_mutation() {
        let mut grid = make_grid(Pow2::new(4));

        grid[point(1, 1)] = 42;

        let chunk = grid
            .get_active_chunk_containing(&point(1, 1))
            .expect("chunk should exist");

        assert_eq!(chunk[point(1, 1)], 42);
    }

    #[test]
    fn grid_returns_same_chunk_for_points_in_same_region() {
        let mut grid = make_grid(Pow2::new(4)); // chunk size = 4

        grid[point(1, 1)] = 10;
        grid[point(3, 3)] = 20;

        let chunk_a = grid.get_active_chunk_containing(&point(1, 1)).unwrap() as *const _;
        let chunk_b = grid.get_active_chunk_containing(&point(3, 3)).unwrap() as *const _;

        assert_eq!(chunk_a, chunk_b);
    }

    #[test]
    fn grid_creates_different_chunks_for_different_regions() {
        let mut grid = make_grid(Pow2::new(4)); // chunk size = 4

        grid[point(1, 1)] = 10;
        grid[point(5, 5)] = 20;

        let chunk_a = grid.get_active_chunk_containing(&point(1, 1)).unwrap() as *const _;
        let chunk_b = grid.get_active_chunk_containing(&point(5, 5)).unwrap() as *const _;

        assert_ne!(chunk_a, chunk_b);
    }

    #[test]
    fn grid_index_panics_when_chunk_does_not_exist() {
        let grid = make_grid(Pow2::new(4));

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = grid[point(0, 0)];
        }));

        assert!(result.is_err());
    }

    #[test]
    fn grid_supports_negative_coordinates() {
        let mut grid = make_grid(Pow2::new(4));

        grid[point(-1, -1)] = 123;

        assert_eq!(grid[point(-1, -1)], 123);
    }

    #[test]
    fn correct_chunks_get_frozen() {
        let mut grid = make_grid(Pow2::new(4));

        grid[point(0, 0)] = 123;
        grid[point(-5, 0)] = 123;

        grid.freeze(&GridRect::with_size(GridPoint::new(-4, -4), 9, 9));

        assert!(grid.is_chunk_containing_frozen(&GridPoint::new(0, 0)));
        assert!(!grid.is_chunk_containing_frozen(&GridPoint::new(-5, 0)));
    }

    #[test]
    fn attempting_to_modify_frozen_chunk_panics() {
        let mut grid = make_grid(Pow2::new(4));

        grid[point(0, 0)] = 123;

        grid.freeze(&GridRect::with_size(GridPoint::new(-40, -40), 81, 81));

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            grid[point(0, 0)] = 123;
        }));

        assert!(result.is_err());
    }

    #[test]
    fn frozen_grid_sample_range2d() {
        let mut grid = make_grid(Pow2::new(4));
        grid[point(-1, -3)] = 1234;
        grid[point(-1, -1)] = 123;

        let frozen_grid: FrozenGrid<_> = grid.into();

        let res = frozen_grid.sample_range2d(&GridRect::with_start_end(
            GridPoint::new(-1, -3),
            GridPoint::new(0, 0),
        ));

        assert_eq!(res.as_flat_slice(), [1234i32, 0, 123]);
    }
}
