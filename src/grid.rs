use std::collections::HashMap;
use crate::coords::{Point2D, Vector2D};

type GridPoint = Point2D<i32>;
type GridVector = Vector2D<i32>;

struct ChunkBounds {
    origin: GridPoint,
    width: i32,
    height: i32,
}

struct Chunk<T> {
    position: ChunkBounds,
    cells: Vec<T>,
}

trait Chunker {
    fn resolve_chunk_origin(&self, _:&GridPoint) -> GridPoint;
    fn resolve_chunk_bounds(&self, _:&GridPoint) -> ChunkBounds;
}

struct Grid<T> {
    chunks: HashMap<GridPoint, Chunk<T>>,
}
