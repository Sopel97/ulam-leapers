use crate::coords::{Point2D, Vector2D};
use std::collections::HashMap;

type GridPoint = Point2D<i32>;
type GridVector = Vector2D<i32>;

pub struct ChunkBounds {
    origin: GridPoint,
    width: i32,
    height: i32,
}

pub struct Chunk<T> {
    position: ChunkBounds,
    cells: Vec<T>,
}

pub trait Chunker {
    fn resolve_chunk_origin(&self, _: &GridPoint) -> GridPoint;
    fn resolve_chunk_bounds(&self, _: &GridPoint) -> ChunkBounds;
}

pub struct SquareChunker {
    chunk_size_pow2: i32,
}

impl Chunker for SquareChunker {
    fn resolve_chunk_origin(&self, point: &GridPoint) -> GridPoint {
        let x = point.x;
        let y = point.y;

        // arithmetic shift provides floored division by a power of 2
        let cx = x >> self.chunk_size_pow2;
        let cy = y >> self.chunk_size_pow2;

        return GridPoint { x: cx, y: cy };
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
    chunks: HashMap<GridPoint, Chunk<T>>,
}
