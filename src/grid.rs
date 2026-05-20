use crate::coords::{Point2D, Vector2D};
use std::collections::HashMap;
use std::ops::{Index, IndexMut};

pub type GridPoint = Point2D<i32>;
pub type GridVector = Vector2D<i32>;

#[derive(Hash, Eq, PartialEq)]
pub struct ChunkOrigin(GridPoint);

pub struct ChunkBounds {
    origin: ChunkOrigin,
    width: u32,
    height: u32,
}

pub struct Chunk<T> {
    bounds: ChunkBounds,
    cells: Vec<T>, // TODO: 64 byte alignment via overallocation
}

impl<T> Chunk<T> {
    pub fn memory_usage(&self) -> usize {
        size_of::<T>() * self.cells.len()
    }
}

impl<T: Default> Chunk<T> {
    pub fn new(bounds: ChunkBounds) -> Chunk<T> {
        let mut cells = Vec::new();
        cells.resize_with((bounds.width * bounds.height) as usize, Default::default);
        Chunk { bounds, cells }
    }
}

impl<T: Default> Index<GridPoint> for Chunk<T> {
    type Output = T;

    fn index(&self, index: GridPoint) -> &Self::Output {
        let xx = index.x - self.bounds.origin.0.x;
        let yy = index.y - self.bounds.origin.0.y;
        &self.cells[yy as usize * self.bounds.width as usize + xx as usize]
    }
}

impl<T: Default> IndexMut<GridPoint> for Chunk<T> {
    fn index_mut(&mut self, index: GridPoint) -> &mut Self::Output {
        let xx = index.x - self.bounds.origin.0.x;
        let yy = index.y - self.bounds.origin.0.y;
        &mut self.cells[yy as usize * self.bounds.width as usize + xx as usize]
    }
}

pub trait Chunker {
    fn resolve_chunk_origin(&self, _: &GridPoint) -> ChunkOrigin;
    fn resolve_chunk_bounds(&self, _: &GridPoint) -> ChunkBounds;
}

pub struct SquareChunker {
    chunk_size_pow2: u32,
}

impl SquareChunker {
    pub fn new(chunk_size_pow2: u32) -> SquareChunker {
        SquareChunker { chunk_size_pow2 }
    }
}

impl Chunker for SquareChunker {
    fn resolve_chunk_origin(&self, point: &GridPoint) -> ChunkOrigin {
        let x = point.x;
        let y = point.y;

        // arithmetic shift provides floored division by a power of 2
        let cx = x >> self.chunk_size_pow2 << self.chunk_size_pow2;
        let cy = y >> self.chunk_size_pow2 << self.chunk_size_pow2;

        ChunkOrigin(GridPoint::new(cx, cy))
    }

    fn resolve_chunk_bounds(&self, bounds: &GridPoint) -> ChunkBounds {
        let origin = self.resolve_chunk_origin(bounds);
        ChunkBounds {
            origin,
            width: 1 << self.chunk_size_pow2,
            height: 1 << self.chunk_size_pow2,
        }
    }
}

pub struct Grid<T> {
    chunker: Box<dyn Chunker>,
    chunks: HashMap<ChunkOrigin, Chunk<T>>,
}

impl<T> Grid<T> {
    pub fn memory_usage(&self) -> usize {
        self.chunks.values().map(|c| c.memory_usage()).sum()
    }
}

impl<T: Default> Grid<T> {
    pub fn new(chunker: Box<dyn Chunker>) -> Self {
        Grid {
            chunker,
            chunks: HashMap::new(),
        }
    }

    pub fn get_chunk_at(&self, point: &ChunkOrigin) -> Option<&Chunk<T>> {
        self.chunks.get(point)
    }

    pub fn get_chunk_containing(&self, point: &GridPoint) -> Option<&Chunk<T>> {
        let origin = self.chunker.resolve_chunk_origin(point);
        self.get_chunk_at(&origin)
    }

    pub fn get_or_create_chunk_containing(&mut self, point: &GridPoint) -> &mut Chunk<T> {
        let origin = self.chunker.resolve_chunk_origin(point);
        self.chunks.entry(origin).or_insert_with(|| {
            let bounds = self.chunker.resolve_chunk_bounds(point);
            Chunk::new(bounds)
        })
    }
}

impl<T: Default> Index<GridPoint> for Grid<T> {
    type Output = T;

    fn index(&self, point: GridPoint) -> &Self::Output {
        let chunk = self
            .get_chunk_containing(&point)
            .expect("chunk should exist");
        &chunk[point]
    }
}

impl<T: Default> IndexMut<GridPoint> for Grid<T> {
    fn index_mut(&mut self, point: GridPoint) -> &mut Self::Output {
        let chunk: &mut Chunk<T> = self.get_or_create_chunk_containing(&point);
        &mut chunk[point]
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

    fn make_grid(chunk_pow2: u32) -> Grid<i32> {
        Grid::new(Box::new(SquareChunker {
            chunk_size_pow2: chunk_pow2,
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
            chunk_size_pow2: 4, // chunk size = 16
        };

        let origin = chunker.resolve_chunk_origin(&point(18, 33));

        assert_eq!(origin.0, point(16, 32));
    }

    #[test]
    fn square_chunker_resolves_negative_chunk_origins() {
        let chunker = SquareChunker {
            chunk_size_pow2: 4, // chunk size = 16
        };

        let origin = chunker.resolve_chunk_origin(&point(-1, -17));

        // arithmetic right shift should floor toward negative infinity
        assert_eq!(origin.0, point(-16, -32));
    }

    #[test]
    fn square_chunker_resolves_bounds() {
        let chunker = SquareChunker {
            chunk_size_pow2: 3, // chunk size = 8
        };

        let bounds = chunker.resolve_chunk_bounds(&point(9, 17));

        assert_eq!(bounds.origin.0, point(8, 16));
        assert_eq!(bounds.width, 8);
        assert_eq!(bounds.height, 8);
    }

    #[test]
    fn grid_creates_chunk_on_mutation() {
        let mut grid = make_grid(2);

        grid[point(1, 1)] = 42;

        let chunk = grid
            .get_chunk_containing(&point(1, 1))
            .expect("chunk should exist");

        assert_eq!(chunk[point(1, 1)], 42);
    }

    #[test]
    fn grid_returns_same_chunk_for_points_in_same_region() {
        let mut grid = make_grid(2); // chunk size = 4

        grid[point(1, 1)] = 10;
        grid[point(3, 3)] = 20;

        let chunk_a = grid.get_chunk_containing(&point(1, 1)).unwrap() as *const _;
        let chunk_b = grid.get_chunk_containing(&point(3, 3)).unwrap() as *const _;

        assert_eq!(chunk_a, chunk_b);
    }

    #[test]
    fn grid_creates_different_chunks_for_different_regions() {
        let mut grid = make_grid(2); // chunk size = 4

        grid[point(1, 1)] = 10;
        grid[point(5, 5)] = 20;

        let chunk_a = grid.get_chunk_containing(&point(1, 1)).unwrap() as *const _;
        let chunk_b = grid.get_chunk_containing(&point(5, 5)).unwrap() as *const _;

        assert_ne!(chunk_a, chunk_b);
    }

    #[test]
    fn grid_index_panics_when_chunk_does_not_exist() {
        let grid = make_grid(2);

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = grid[point(0, 0)];
        }));

        assert!(result.is_err());
    }

    #[test]
    fn grid_supports_negative_coordinates() {
        let mut grid = make_grid(2);

        grid[point(-1, -1)] = 123;

        assert_eq!(grid[point(-1, -1)], 123);
    }
}
