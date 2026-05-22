use crate::collections::array2d::Array2D;
use crate::coords::{Point2D, Vector2D};
use std::collections::BTreeMap;
use std::ops::{Index, IndexMut};
use crate::util::align::CACHE_LINE_SIZE;
use crate::util::pow2::Pow2;
use crate::util::pow2;

pub type GridPoint = Point2D<i32>;
pub type GridVector = Vector2D<i32>;

#[derive(Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct ChunkOrigin(GridPoint);

pub struct ChunkBounds {
    origin: ChunkOrigin,
    width: u32,
    height: u32,
}

pub struct Chunk<T> {
    bounds: ChunkBounds,
    cells: Array2D<T>,
}

impl<T> Chunk<T> {
    pub fn memory_usage(&self) -> usize {
        size_of::<T>() * self.cells.width() * self.cells.height()
    }

    pub fn contains_point(&self, point: &GridPoint) -> bool {
        point.x >= self.bounds.origin.0.x
            && point.y >= self.bounds.origin.0.y
            && point.x < self.bounds.origin.0.x + self.bounds.width as i32
            && point.y < self.bounds.origin.0.y + self.bounds.height as i32
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
    chunker: Box<dyn Chunker>,
    chunks: BTreeMap<ChunkOrigin, Chunk<T>>,
}

impl<T> Grid<T> {
    pub fn memory_usage(&self) -> usize {
        self.chunks.values().map(|c| c.memory_usage()).sum()
    }
}

impl<T: Default + Clone + Copy> Grid<T> {
    pub fn new(chunker: Box<dyn Chunker>) -> Self {
        Grid {
            chunker,
            chunks: BTreeMap::new(),
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
        let chunk = self
            .get_chunk_containing(&point)
            .expect("chunk should exist");
        &chunk[point]
    }
}

impl<T: Default + Clone + Copy> IndexMut<GridPoint> for Grid<T> {
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
            .get_chunk_containing(&point(1, 1))
            .expect("chunk should exist");

        assert_eq!(chunk[point(1, 1)], 42);
    }

    #[test]
    fn grid_returns_same_chunk_for_points_in_same_region() {
        let mut grid = make_grid(Pow2::new(4)); // chunk size = 4

        grid[point(1, 1)] = 10;
        grid[point(3, 3)] = 20;

        let chunk_a = grid.get_chunk_containing(&point(1, 1)).unwrap() as *const _;
        let chunk_b = grid.get_chunk_containing(&point(3, 3)).unwrap() as *const _;

        assert_eq!(chunk_a, chunk_b);
    }

    #[test]
    fn grid_creates_different_chunks_for_different_regions() {
        let mut grid = make_grid(Pow2::new(4)); // chunk size = 4

        grid[point(1, 1)] = 10;
        grid[point(5, 5)] = 20;

        let chunk_a = grid.get_chunk_containing(&point(1, 1)).unwrap() as *const _;
        let chunk_b = grid.get_chunk_containing(&point(5, 5)).unwrap() as *const _;

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
}
