use crate::game::chunk::{ChunkOrigin, ULS_MAXIMUM_CHUNK_EXTENT, ULS_MAXIMUM_CHUNK_SIZE, ULS_MINIMUM_CHUNK_ALIGNMENT};
use crate::io::{ReadFrom, WriteTo};
use crate::math::coords::GridPoint;
use crate::math::pow2::{floor_to_multiple, Pow2};
use crate::math::rect::GridRect;
use std::io::{ErrorKind, Read, Write};

/// # NOTE
///
/// To maintain invariants for the ULS format instances of this type
/// should be made via StandardChunker::try_* instead.
/// This is a limitation of Rust that there is no way to validate enum
/// values directly. May change with a more convoluted abstraction in the future.
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum StandardChunker {
    SquareChunker { chunk_size_pow2: u8 },
}

impl StandardChunker {
    pub fn try_new_square_chunker(size: Pow2) -> Option<Self> {
        if size.as_usize() < ULS_MINIMUM_CHUNK_ALIGNMENT
            || size.as_usize() > ULS_MAXIMUM_CHUNK_EXTENT
            || size.as_usize().pow(2) > ULS_MAXIMUM_CHUNK_SIZE
        {
            None
        } else {
            Some(StandardChunker::SquareChunker {
                chunk_size_pow2: size.exponent(),
            })
        }
    }
}

impl TryFrom<&SquareChunker> for StandardChunker {
    type Error = ();

    fn try_from(chunker: &SquareChunker) -> Result<Self, Self::Error> {
        StandardChunker::try_new_square_chunker(chunker.size).ok_or(())
    }
}

impl StandardChunker {
    pub fn into_chunker(self) -> Box<dyn Chunker> {
        match self {
            StandardChunker::SquareChunker { chunk_size_pow2 } => Box::new(SquareChunker::new(
                Pow2::from_exponent(chunk_size_pow2 as usize),
            )),
        }
    }
}

pub trait Chunker: Send + Sync {
    fn resolve_chunk_origin(&self, _: &GridPoint) -> ChunkOrigin;
    fn resolve_chunk_bounds(&self, _: &GridPoint) -> GridRect;
    fn origins_of_intersecting_chunks(&self, region: &GridRect) -> Vec<ChunkOrigin>;
    fn minimum_chunk_alignment(&self) -> usize;
    fn minimum_chunk_extent(&self) -> usize;
    fn minimum_covered_shells(&self) -> usize;
    fn average_cell_count(&self) -> usize;
    fn maximum_cells_created_by_spiral_steps(&self, steps: usize) -> usize;

    fn as_standard_chunker(&self) -> Option<StandardChunker>;
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
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
        let cx = floor_to_multiple(x, self.size);
        let cy = floor_to_multiple(y, self.size);

        ChunkOrigin::new(GridPoint::new(cx, cy))
    }

    fn resolve_chunk_bounds(&self, bounds: &GridPoint) -> GridRect {
        let origin = self.resolve_chunk_origin(bounds);
        GridRect::square_with_size(origin.point(), self.size.into())
    }

    fn origins_of_intersecting_chunks(&self, region: &GridRect) -> Vec<ChunkOrigin> {
        let min_ox = floor_to_multiple(region.start.x, self.size);
        let min_oy = floor_to_multiple(region.start.y, self.size);
        (min_oy..region.end.y)
            .step_by(self.size.into())
            .flat_map(|oy| {
                (min_ox..region.end.x)
                    .step_by(self.size.into())
                    .map(move |ox| ChunkOrigin::new(GridPoint::new(ox, oy)))
            })
            .collect()
    }

    fn minimum_chunk_alignment(&self) -> usize {
        self.size.into()
    }

    fn minimum_chunk_extent(&self) -> usize {
        self.size.into()
    }

    fn minimum_covered_shells(&self) -> usize {
        self.size.into()
    }

    fn average_cell_count(&self) -> usize {
        self.size.as_usize() * self.size.as_usize()
    }

    fn maximum_cells_created_by_spiral_steps(&self, steps: usize) -> usize {
        let chunk_size = self.size.as_usize() * self.size.as_usize();
        // Corners in extreme case
        if steps < 4 {
            steps * chunk_size
        } else {
            (4 + (steps - 4).div_ceil(self.size.as_usize())) * chunk_size
        }
    }

    fn as_standard_chunker(&self) -> Option<StandardChunker> {
        StandardChunker::try_from(self).ok()
    }
}

impl WriteTo for StandardChunker {
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        match self {
            StandardChunker::SquareChunker { chunk_size_pow2 } => {
                "SquareChunker".as_bytes().write_to(writer)?;
                chunk_size_pow2.write_to(writer)
            }
        }
    }
}

impl ReadFrom for StandardChunker {
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        let t = Box::<[u8]>::read_from(reader)?;
        if t.iter().eq("SquareChunker".as_bytes()) {
            let chunk_size_pow2 = u8::read_from(reader)?;
            let size = Pow2::from_exponent(chunk_size_pow2 as usize);
            StandardChunker::try_new_square_chunker(size).ok_or_else(|| {
                std::io::Error::new(
                    ErrorKind::InvalidData,
                    "Invalid chunk size for SquareChunker.",
                )
            })
        } else {
            Err(std::io::Error::new(
                ErrorKind::InvalidData,
                "Invalid chunker type.",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: i32, y: i32) -> GridPoint {
        GridPoint::new(x, y)
    }

    #[test]
    fn square_chunker_resolves_positive_chunk_origins() {
        let chunker = SquareChunker {
            size: Pow2::new(64),
        };

        let origin = chunker.resolve_chunk_origin(&point(128 + 18, 192 + 33));

        assert_eq!(origin.point(), point(128, 192));
    }

    #[test]
    fn square_chunker_resolves_negative_chunk_origins() {
        let chunker = SquareChunker {
            size: Pow2::new(64),
        };

        let origin = chunker.resolve_chunk_origin(&point(-128 + 1, -192 + 17));

        // arithmetic right shift should floor toward negative infinity
        assert_eq!(origin.point(), point(-128, -192));
    }

    #[test]
    fn square_chunker_resolves_bounds() {
        let chunker = SquareChunker {
            size: Pow2::new(64),
        };

        let bounds = chunker.resolve_chunk_bounds(&point(64 + 9, 128 + 17));

        assert_eq!(bounds.start, point(64, 128));
        assert_eq!(bounds.width(), 64);
        assert_eq!(bounds.height(), 64);
    }
}