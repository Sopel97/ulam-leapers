use crate::collections::array2d::Array2D;
use crate::coords::{Point2D, Vector2D};
use crate::util::align::CACHE_LINE_SIZE;
use crate::util::pow2;
use crate::util::pow2::Pow2;
use std::collections::BTreeMap;
use std::ops::{Index, IndexMut};
use crate::util::memory::{as_bytes, as_bytes_mut};

pub type GridPoint = Point2D<i32>;
pub type GridVector = Vector2D<i32>;

#[derive(Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct ChunkOrigin(GridPoint);

#[derive(Clone, Copy)]
pub struct ChunkBounds {
    origin: ChunkOrigin,
    width: u32,
    height: u32,
}

trait SquareBoundedChunk {
    fn bounds(&self) -> &ChunkBounds;

    fn contains_point(&self, point: &GridPoint) -> bool {
        let bounds = self.bounds();
        point.x >= bounds.origin.0.x
            && point.y >= bounds.origin.0.y
            && point.x < bounds.origin.0.x + bounds.width as i32
            && point.y < bounds.origin.0.y + bounds.height as i32
    }

    fn is_contained_within(&self, min: &GridPoint, max: &GridPoint) -> bool {
        let bounds = self.bounds();
        bounds.origin.0.x >= min.x
            && bounds.origin.0.y >= min.y
            && bounds.origin.0.x + bounds.width as i32 - 1 <= max.x
            && bounds.origin.0.y + bounds.height as i32 - 1 <= max.y
    }
}

pub struct Chunk<T> {
    bounds: ChunkBounds,
    cells: Array2D<T>,
}

impl<T> SquareBoundedChunk for Chunk<T> {
    fn bounds(&self) -> &ChunkBounds {
        &self.bounds
    }
}

impl<T> Chunk<T> {
    pub fn memory_usage(&self) -> usize {
        size_of::<T>() * self.cells.width() * self.cells.height()
    }
}

impl<T: Default + Clone> Chunk<T> {
    pub fn new(bounds: ChunkBounds) -> Chunk<T> {
        let cells = Array2D::<T>::new_aligned(bounds.width as usize, bounds.height as usize, CACHE_LINE_SIZE);
        Chunk { bounds, cells }
    }
}

impl<T: Default> Index<GridPoint> for Chunk<T> {
    type Output = T;

    fn index(&self, index: GridPoint) -> &Self::Output {
        let xx = index.x - self.bounds.origin.0.x;
        let yy = index.y - self.bounds.origin.0.y;
        &self.cells[(xx as usize, yy as usize)]
    }
}

impl<T: Default> IndexMut<GridPoint> for Chunk<T> {
    fn index_mut(&mut self, index: GridPoint) -> &mut Self::Output {
        let xx = index.x - self.bounds.origin.0.x;
        let yy = index.y - self.bounds.origin.0.y;
        &mut self.cells[(xx as usize, yy as usize)]
    }
}

// Generic over T because we want to preserve type information of the underlying data.
pub struct CompressedChunk<T> {
    bounds: ChunkBounds,
    data: Box<[u8]>,
    _marker: std::marker::PhantomData<T>,
}

impl<T> From<&Chunk<T>> for CompressedChunk<T> {
    fn from(chunk: &Chunk<T>) -> Self {
        // We might try Morton transform and bit-transposition later,
        // but for now zstd can reduce the size to pretty much zero for the test-case.
        // We use default level 3 compression because higher levels don't seem to have an impact.
        let raw_uncompressed = as_bytes(chunk.cells.as_flat_slice());
        let compressed = zstd::encode_all(raw_uncompressed, 3).unwrap().into_boxed_slice();
        CompressedChunk { bounds: chunk.bounds, data: compressed, _marker: std::marker::PhantomData }
    }
}

impl<T: Default + Clone + Copy> From<&CompressedChunk<T>> for Chunk<T> {
    fn from(chunk: &CompressedChunk<T>) -> Self {
        let mut cells: Array2D<T> = Array2D::new_aligned(chunk.bounds.width as usize, chunk.bounds.height as usize, CACHE_LINE_SIZE);
        let raw_cells = as_bytes_mut(cells.as_flat_mut_slice());
        zstd::bulk::decompress_to_buffer(chunk.data.iter().as_slice(), raw_cells).unwrap();
        Chunk { bounds: chunk.bounds, cells }
    }
}

impl<T> SquareBoundedChunk for CompressedChunk<T> {
    fn bounds(&self) -> &ChunkBounds {
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
    fn resolve_chunk_bounds(&self, _: &GridPoint) -> ChunkBounds;
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

    fn resolve_chunk_bounds(&self, bounds: &GridPoint) -> ChunkBounds {
        let origin = self.resolve_chunk_origin(bounds);
        ChunkBounds {
            origin,
            width: self.size.into(),
            height: self.size.into(),
        }
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

    pub fn freeze(&mut self, min: &GridPoint, max: &GridPoint) {
        let to_freeze = self.active_chunks.extract_if(.., |_origin, chunk| {
            chunk.is_contained_within(min, max)
        });
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
        self.freeze(
            &GridPoint::new(i32::MIN, i32::MIN),
            &GridPoint::new(i32::MAX, i32::MAX),
        );
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
}

impl<T> Into<FrozenGrid<T>> for Grid<T> {
    fn into(mut self) -> FrozenGrid<T> {
        self.freeze_all();

        FrozenGrid {
            chunker: self.chunker,
            frozen_chunks: self.frozen_chunks
        }
    }
}

impl<T: Default + Clone + Copy> FrozenGrid<T> {
    pub fn sample_range2d(&self, min: &GridPoint, max: &GridPoint) -> Array2D<T> {
        let mut uncompressed_chunk_cache: BTreeMap<ChunkOrigin, Chunk<T>> = BTreeMap::new();

        let width = max.x - min.x + 1;
        let height = max.y - min.y + 1;
        let mut result: Array2D<T> = Array2D::new(width as usize, height as usize);

        for x in min.x..max.x+1 {
            for y in min.y..max.y+1 {
                let chunk_origin = self.chunker.resolve_chunk_origin(&GridPoint::new(x, y));
                let chunk = if let Some(chunk) = uncompressed_chunk_cache.get(&chunk_origin) {
                    chunk
                } else {
                    let compressed_chunk = self.frozen_chunks.get(&chunk_origin);
                    match compressed_chunk {
                        Some(chunk) => {
                            uncompressed_chunk_cache.entry(chunk_origin).or_insert(Chunk::from(chunk))
                        },
                        // If we don't have a corresponding chunk we just leave the output values unfilled.
                        None => continue,
                    }
                };

                let val = chunk[GridPoint::new(x, y)];

                let dx = (x - min.x) as usize;
                let dy = (y - min.y) as usize;
                result[(dx, dy)] = val;
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::AssertUnwindSafe;

    fn point(x: i32, y: i32) -> GridPoint {
        GridPoint::new(x, y)
    }

    fn make_bounds(origin_x: i32, origin_y: i32, width: u32, height: u32) -> ChunkBounds {
        ChunkBounds {
            origin: ChunkOrigin(point(origin_x, origin_y)),
            width,
            height,
        }
    }

    fn make_grid(chunk_size: Pow2) -> Grid<i32> {
        Grid::new(Box::new(SquareChunker {
            size: chunk_size,
        }))
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
            size: Pow2::new(16)
        };

        let origin = chunker.resolve_chunk_origin(&point(18, 33));

        assert_eq!(origin.0, point(16, 32));
    }

    #[test]
    fn square_chunker_resolves_negative_chunk_origins() {
        let chunker = SquareChunker {
            size: Pow2::new(16)
        };

        let origin = chunker.resolve_chunk_origin(&point(-1, -17));

        // arithmetic right shift should floor toward negative infinity
        assert_eq!(origin.0, point(-16, -32));
    }

    #[test]
    fn square_chunker_resolves_bounds() {
        let chunker = SquareChunker {
            size: Pow2::new(8)
        };

        let bounds = chunker.resolve_chunk_bounds(&point(9, 17));

        assert_eq!(bounds.origin.0, point(8, 16));
        assert_eq!(bounds.width, 8);
        assert_eq!(bounds.height, 8);
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

        grid.freeze(&GridPoint::new(-4, -4), &GridPoint::new(4, 4));

        assert!(grid.is_chunk_containing_frozen(&GridPoint::new(0, 0)));
        assert!(!grid.is_chunk_containing_frozen(&GridPoint::new(-5, 0)));
    }

    #[test]
    fn attempting_to_modify_frozen_chunk_panics() {
        let mut grid = make_grid(Pow2::new(4));

        grid[point(0, 0)] = 123;

        grid.freeze(&GridPoint::new(-40, -40), &GridPoint::new(40, 40));

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

        let res = frozen_grid.sample_range2d(&GridPoint::new(-1, -3), &GridPoint::new(-1, -1));

        assert_eq!(res.as_flat_slice(), [1234i32, 0, 123]);
    }
}
