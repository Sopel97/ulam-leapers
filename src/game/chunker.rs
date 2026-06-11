use crate::game::chunk::{
    ChunkOrigin, ULS_MAXIMUM_CHUNK_EXTENT, ULS_MAXIMUM_CHUNK_SIZE, ULS_MINIMUM_CHUNK_ALIGNMENT,
};
use crate::io::{ReadFrom, WriteTo};
use crate::math::coords::GridPoint;
use crate::math::pow2::{Pow2, div_ceil, div_floor, floor_to_multiple, is_multiple_of};
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
    SquareChunker {
        chunk_size_pow2: u8,
    },
    StripChunker {
        strip_length_pow2: u8,
        strip_thickness_pow2: u8,
    },
}

impl StandardChunker {
    pub fn try_new_square_chunker(size: Pow2) -> Option<Self> {
        if size.as_u64() < ULS_MINIMUM_CHUNK_ALIGNMENT
            || size.as_u64() > ULS_MAXIMUM_CHUNK_EXTENT
            || size.as_u64().pow(2) > ULS_MAXIMUM_CHUNK_SIZE
        {
            None
        } else {
            Some(StandardChunker::SquareChunker {
                chunk_size_pow2: size.exponent(),
            })
        }
    }

    pub fn try_new_flat_chunker(strip_length: Pow2, strip_thickness: Pow2) -> Option<Self> {
        if strip_thickness > strip_length {
            return None;
        }

        if strip_thickness.as_u64() < ULS_MINIMUM_CHUNK_ALIGNMENT
            || strip_length.as_u64() > ULS_MAXIMUM_CHUNK_EXTENT
            || (strip_thickness * strip_length).as_u64() > ULS_MAXIMUM_CHUNK_SIZE
        {
            None
        } else {
            Some(StandardChunker::StripChunker {
                strip_length_pow2: strip_length.exponent(),
                strip_thickness_pow2: strip_thickness.exponent(),
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
            StandardChunker::SquareChunker { chunk_size_pow2 } => {
                Box::new(SquareChunker::new(Pow2::from_exponent(chunk_size_pow2)))
            },
            StandardChunker::StripChunker { strip_length_pow2, strip_thickness_pow2 } => {
                Box::new(StripChunker::with_strip_length_and_thickness(Pow2::from_exponent(strip_length_pow2), Pow2::from_exponent(strip_thickness_pow2)))
            }
        }
    }
}

pub trait Chunker: Send + Sync {
    fn resolve_chunk_origin(&self, point: &GridPoint) -> ChunkOrigin;
    fn resolve_chunk_bounds(&self, point: &GridPoint) -> GridRect;
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

    fn resolve_chunk_bounds(&self, point: &GridPoint) -> GridRect {
        let origin = self.resolve_chunk_origin(point);
        GridRect::square_with_size(origin.point(), self.size.as_u64() as i32)
    }

    fn origins_of_intersecting_chunks(&self, region: &GridRect) -> Vec<ChunkOrigin> {
        let min_ox = floor_to_multiple(region.start.x, self.size);
        let min_oy = floor_to_multiple(region.start.y, self.size);
        (min_oy..region.end.y)
            .step_by(self.size.as_u64() as usize)
            .flat_map(|oy| {
                (min_ox..region.end.x)
                    .step_by(self.size.as_u64() as usize)
                    .map(move |ox| ChunkOrigin::new(GridPoint::new(ox, oy)))
            })
            .collect()
    }

    fn minimum_chunk_alignment(&self) -> usize {
        self.size.as_u64() as usize
    }

    fn minimum_chunk_extent(&self) -> usize {
        self.size.as_u64() as usize
    }

    fn minimum_covered_shells(&self) -> usize {
        self.size.as_u64() as usize
    }

    fn average_cell_count(&self) -> usize {
        self.size.as_u64().pow(2) as usize
    }

    fn maximum_cells_created_by_spiral_steps(&self, steps: usize) -> usize {
        let chunk_size = self.size.as_u64().pow(2) as usize;
        // Corners in extreme case
        if steps < 4 {
            steps * chunk_size
        } else {
            (4 + div_ceil(steps - 4, self.size)) * chunk_size
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
            },
            StandardChunker::StripChunker { strip_length_pow2, strip_thickness_pow2 } => {
                "StripChunker".as_bytes().write_to(writer)?;
                strip_length_pow2.write_to(writer)?;
                strip_thickness_pow2.write_to(writer)
            }
        }
    }
}

impl ReadFrom for StandardChunker {
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        let t = Box::<[u8]>::read_from(reader)?;
        if t.iter().eq("SquareChunker".as_bytes()) {
            let chunk_size_pow2 = u8::read_from(reader)?;
            let size = Pow2::from_exponent(chunk_size_pow2);
            StandardChunker::try_new_square_chunker(size).ok_or_else(|| {
                std::io::Error::new(
                    ErrorKind::InvalidData,
                    "Invalid chunk size for SquareChunker.",
                )
            })
        } else if t.iter().eq("StripChunker".as_bytes()) {
            let strip_length_pow2 = u8::read_from(reader)?;
            let strip_thickness_pow2 = u8::read_from(reader)?;
            StandardChunker::try_new_flat_chunker(Pow2::from_exponent(strip_length_pow2), Pow2::from_exponent(strip_thickness_pow2)).ok_or_else(|| {
                std::io::Error::new(
                    ErrorKind::InvalidData,
                    "Invalid chunk sizes for StripChunker.",
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

/// Chunks the grid into stripes within superchunks.
/// Superchunks are squares tiling the plane.
/// Superchunks are `strip_length` by `strip_length` in size.
/// Within every superchunk there is exactly `strip_length / strip_thickness` isomorphic strips.
/// The strip's orientation is determined by the position of its superchunk.
///
/// The spiral starts out at the origin, oscillating between non-negative and
/// negative superchunks, which is why we want the chunk structure to be symmetric
/// about the origin. The idea is to have the spiral be aligned with strip
/// directions as much as possible to maximize usefulness of chunks during
/// generation - narrower in the slow growth direction, less wasted space.
/// The orientation of the strips in superchunks laying on the
/// diagonals is not important, both are equally suboptimal.
///
/// Given `(sx, sy)` - the position of the superchunk - the strip orientation can
/// be computed in the following way:
/// ```
/// pub enum Orientation {
///     Horizontal,
///     Vertical
/// }
///
/// pub fn orient(sx: i32, sy: i32) -> Orientation {
///     let a = sx - sy;
///     let b = sx+1 + sy;
///     if a * b > 0 {
///         Orientation::Vertical
///     } else {
///         Orientation::Horizontal
///     }
/// }
/// ```
///
/// Table showing either `|` for vertical or `-` for horizontal strips
/// for some superchunk coordinates.
///
///  +4  |  |  |  |  |  |  |  |  |  |
///  +3  -  |  |  |  |  |  |  |  |  -
///  +2  -  -  |  |  |  |  |  |  -  -
///  +1  -  -  -  |  |  |  |  -  -  -
///  +0  -  -  -  -  |  |  -  -  -  -
///  -1  -  -  -  -  |  |  -  -  -  -
///  -2  -  -  -  |  |  |  |  -  -  -
///  -3  -  -  |  |  |  |  |  |  -  -
///  -4  -  |  |  |  |  |  |  |  |  -
///  -5  |  |  |  |  |  |  |  |  |  |
///     -5 -4 -3 -2 -1 +0 +1 +2 +3 +4
///
/// # Example
///
/// strip_length = 2
/// strip_thickness = 1
///
/// Numbers on both axes are the superchunk coordinates
/// as computed by `div_floor`. `O` signifies the origin.
///
///    ┌───┬───┬───┬───┬───┬───┬───┬───┐
///  3 ├───┼───┼───┼───┼───┼───┼───┼───┤
///    ├─┬─┼───┼───┼───┼───┼───┼───┼─┬─┤
///  2 │ │ ├───┼───┼───┼───┼───┼───┤ │ │
///    ├─┼─┼─┬─┼───┼───┼───┼───┼─┬─┼─┼─┤
///  1 │ │ │ │ ├───┼───┼───┼───┤ │ │ │ │
///    ├─┼─┼─┼─┼─┬─┼───┼───┼─┬─┼─┼─┼─┼─┤
///  0 │ │ │ │ │ │ ├───┼───┤ │ │ │ │ │ │
///    ├─┼─┼─┼─┼─┼─┼───O───┼─┼─┼─┼─┼─┼─┤
/// -1 │ │ │ │ │ │ ├───┼───┤ │ │ │ │ │ │
///    ├─┼─┼─┼─┼─┴─┼───┼───┼─┴─┼─┼─┼─┼─┤
/// -2 │ │ │ │ ├───┼───┼───┼───┤ │ │ │ │
///    ├─┼─┼─┴─┼───┼───┼───┼───┼─┴─┼─┼─┤
/// -3 │ │ ├───┼───┼───┼───┼───┼───┤ │ │
///    ├─┴─┼───┼───┼───┼───┼───┼───┼─┴─┤
/// -4 ├───┼───┼───┼───┼───┼───┼───┼───┤
///    └───┴───┴───┴───┴───┴───┴───┴───┘
///     -4  -3  -2  -1   0   1   2   3
///
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub struct StripChunker {
    strip_length: Pow2,
    strip_thickness: Pow2,
}

enum SuperchunkOrientation {
    Horizontal,
    Vertical,
}

impl StripChunker {
    pub fn with_strip_length_and_thickness(strip_length: Pow2, strip_thickness: Pow2) -> StripChunker {
        assert!(strip_thickness <= strip_length);
        StripChunker { strip_length, strip_thickness }
    }

    fn superchunk_orientation(sx: i32, sy: i32) -> SuperchunkOrientation{
        let a = sx - sy;
        let b = sx+1 + sy;
        if a * b > 0 {
            SuperchunkOrientation::Vertical
        } else {
            SuperchunkOrientation::Horizontal
        }
    }

    fn num_chunks_in_superchunk(&self) -> usize {
        (self.strip_length / self.strip_thickness).as_u64() as usize
    }
}

impl Chunker for StripChunker {
    fn resolve_chunk_origin(&self, point: &GridPoint) -> ChunkOrigin {
        let superchunk_x = div_floor(point.x, self.strip_length);
        let superchunk_y = div_floor(point.y, self.strip_length);

        let orientation = Self::superchunk_orientation(superchunk_x, superchunk_y);
        // We can reuse superchunk position for one axis,
        // the other needs to be more fine-grained.
        match orientation {
            SuperchunkOrientation::Horizontal => {
                let ox = superchunk_x * self.strip_length.as_u64() as i32;
                let oy = floor_to_multiple(point.y, self.strip_thickness);
                ChunkOrigin::new(GridPoint::new(ox, oy))
            }
            SuperchunkOrientation::Vertical => {
                let ox = floor_to_multiple(point.x, self.strip_thickness);
                let oy = superchunk_y * self.strip_length.as_u64() as i32;
                ChunkOrigin::new(GridPoint::new(ox, oy))
            }
        }
    }

    fn resolve_chunk_bounds(&self, point: &GridPoint) -> GridRect {
        let superchunk_x = div_floor(point.x, self.strip_length);
        let superchunk_y = div_floor(point.y, self.strip_length);

        let orientation = Self::superchunk_orientation(superchunk_x, superchunk_y);
        // We can reuse superchunk position for one axis,
        // the other needs to be more fine-grained.
        match orientation {
            SuperchunkOrientation::Horizontal => {
                let cw = self.strip_length.as_u64() as i32;
                let ch = self.strip_thickness.as_u64() as i32;
                let ox = superchunk_x * cw;
                let oy = floor_to_multiple(point.y, self.strip_thickness);
                GridRect::with_size(GridPoint::new(ox, oy), cw, ch)
            }
            SuperchunkOrientation::Vertical => {
                let cw = self.strip_thickness.as_u64() as i32;
                let ch = self.strip_length.as_u64() as i32;
                let ox = floor_to_multiple(point.x, self.strip_thickness);
                let oy = superchunk_y * self.strip_length.as_u64() as i32;
                GridRect::with_size(GridPoint::new(ox, oy), cw, ch)
            }
        }
    }

    fn origins_of_intersecting_chunks(&self, region: &GridRect) -> Vec<ChunkOrigin> {
        let min_sox = floor_to_multiple(region.start.x, self.strip_length);
        let min_soy = floor_to_multiple(region.start.y, self.strip_length);

        // All superchunks have the same amount of chunks.
        // Not all of them may actually intersect the region, but doesn't hurt
        // to allocate space for the worst case, the difference is small.
        let superchunk_count_x = div_floor(region.end.x - min_sox, self.strip_length) as usize;
        let superchunk_count_y = div_floor(region.end.y - min_soy, self.strip_length) as usize;
        let superchunk_count = superchunk_count_x * superchunk_count_y;
        let chunk_count = superchunk_count * self.num_chunks_in_superchunk();
        let mut origins = Vec::with_capacity(chunk_count);

        let superchunk_size_usize = self.strip_length.as_u64() as usize;
        let superchunk_size_i32 = superchunk_size_usize as i32;
        let strip_thickness_usize = self.strip_thickness.as_u64() as usize;

        for soy in (min_soy..region.end.y).step_by(superchunk_size_usize) {
            for sox in (min_sox..region.end.x).step_by(superchunk_size_usize) {
                // We need to emit strips within this superchunks,
                // so we need to know orientation.
                let sx = div_floor(sox, self.strip_length);
                let sy = div_floor(soy, self.strip_length);
                let orientation = Self::superchunk_orientation(sx, sy);

                // Not all chunks within a superchunk will actually intersect the region,
                // depending on their orientation. Readjust `so{xy}` start and end.
                // Start also requires further realignment.
                match orientation {
                    SuperchunkOrientation::Horizontal => {
                        // Identical `ox = sox`, different `oy` for each chunk.
                        let soy_start = floor_to_multiple(soy.max(region.start.y), self.strip_thickness);
                        let soy_end = (soy+superchunk_size_i32).min(region.end.y);
                        for oy in (soy_start..soy_end).step_by(strip_thickness_usize) {
                           origins.push(ChunkOrigin::new(GridPoint::new(sox, oy)));
                        }
                    },
                    SuperchunkOrientation::Vertical => {
                        // Identical `oy = soy`, different `ox` for each chunk.
                        let sox_start = floor_to_multiple(sox.max(region.start.x), self.strip_thickness);
                        let sox_end = (sox+superchunk_size_i32).min(region.end.x);
                        for ox in (sox_start..sox_end).step_by(strip_thickness_usize) {
                            origins.push(ChunkOrigin::new(GridPoint::new(ox, soy)));
                        }
                    }
                }
            }
        }

        origins
    }

    fn minimum_chunk_alignment(&self) -> usize {
        self.strip_thickness.as_u64() as usize
    }

    fn minimum_chunk_extent(&self) -> usize {
        self.strip_thickness.as_u64() as usize
    }

    fn minimum_covered_shells(&self) -> usize {
        (self.strip_length * self.strip_thickness).as_u64() as usize
    }

    fn average_cell_count(&self) -> usize {
        (self.strip_length * self.strip_thickness).as_u64() as usize
    }

    fn maximum_cells_created_by_spiral_steps(&self, steps: usize) -> usize {
        let chunk_size = (self.strip_length * self.strip_thickness).as_u64() as usize;
        // Corners in extreme case
        if steps < 4 {
            steps * chunk_size
        } else {
            // Strips are aligned with the spiral traversal wherever possible.
            (4 + div_ceil(steps - 4, self.strip_length)) * chunk_size
        }
    }

    fn as_standard_chunker(&self) -> Option<StandardChunker> {
        StandardChunker::try_new_flat_chunker(self.strip_length, self.strip_thickness)
    }
}

#[cfg(test)]
mod tests {
    use crate::math::coords::GridVector;
    use super::*;

    fn point(x: i32, y: i32) -> GridPoint {
        GridPoint::new(x, y)
    }
    fn vector(x: i32, y: i32) -> GridVector {
        GridVector::new(x, y)
    }

    #[test]
    fn square_chunker_resolves_positive_chunk_origins() {
        let chunker = SquareChunker {
            size: Pow2::try_from(64).unwrap(),
        };

        let origin = chunker.resolve_chunk_origin(&point(128 + 18, 192 + 33));

        assert_eq!(origin.point(), point(128, 192));
    }

    #[test]
    fn square_chunker_resolves_negative_chunk_origins() {
        let chunker = SquareChunker {
            size: Pow2::try_from(64).unwrap(),
        };

        let origin = chunker.resolve_chunk_origin(&point(-128 + 1, -192 + 17));

        // arithmetic right shift should floor toward negative infinity
        assert_eq!(origin.point(), point(-128, -192));
    }

    #[test]
    fn square_chunker_resolves_bounds() {
        let chunker = SquareChunker {
            size: Pow2::try_from(64).unwrap(),
        };

        let bounds = chunker.resolve_chunk_bounds(&point(64 + 9, 128 + 17));

        assert_eq!(bounds.start, point(64, 128));
        assert_eq!(bounds.width(), 64);
        assert_eq!(bounds.height(), 64);
    }

    #[test]
    fn strip_chunker_resolves_correct_chunk_origins() {
        let chunker = StripChunker {
            strip_length: Pow2::from_exponent(12),
            strip_thickness: Pow2::from_exponent(8),
        };

        // Horizontal
        assert_eq!(chunker.resolve_chunk_origin(&point(0, 0)).point(), point(0, 0));
        assert_eq!(chunker.resolve_chunk_origin(&point(257, 0)).point(), point(0, 0));
        assert_eq!(chunker.resolve_chunk_origin(&point(-1, 0)).point(), point(-4096, 0));

        // Horizontal
        assert_eq!(chunker.resolve_chunk_origin(&point(0, 0)).point(), point(0, 0));
        assert_eq!(chunker.resolve_chunk_origin(&point(257, 257)).point(), point(0, 256));
        assert_eq!(chunker.resolve_chunk_origin(&point(4095, 257)).point(), point(0, 256));
        assert_eq!(chunker.resolve_chunk_origin(&point(-1, -1)).point(), point(-4096, -256));
        assert_eq!(chunker.resolve_chunk_origin(&point(-1, -257)).point(), point(-4096, -512));

        // Vertical
        assert_eq!(chunker.resolve_chunk_origin(&point(4097, 0)).point(), point(4096, 0));
        assert_eq!(chunker.resolve_chunk_origin(&point(4097, 257)).point(), point(4096, 0));
        assert_eq!(chunker.resolve_chunk_origin(&point(4097+256, 257)).point(), point(4096+256, 0));
        assert_eq!(chunker.resolve_chunk_origin(&point(4097, 257+256)).point(), point(4096, 0));
        assert_eq!(chunker.resolve_chunk_origin(&point(-4097, -1)).point(), point(-4096-256, -4096));
        assert_eq!(chunker.resolve_chunk_origin(&point(-4097-256, -1)).point(), point(-4096-512, -4096));
        assert_eq!(chunker.resolve_chunk_origin(&point(-4097-256, -1-256)).point(), point(-4096-512, -4096));
    }

    #[test]
    fn strip_chunker_resolves_correct_chunk_sizes() {
        let chunker = StripChunker {
            strip_length: Pow2::from_exponent(12),
            strip_thickness: Pow2::from_exponent(8),
        };

        let horizontal_size = vector(4096, 256);
        let vertical_size = vector(256, 4096);

        // Horizontal
        assert_eq!(chunker.resolve_chunk_bounds(&point(0, 0)).extent(), horizontal_size);
        assert_eq!(chunker.resolve_chunk_bounds(&point(257, 0)).extent(), horizontal_size);
        assert_eq!(chunker.resolve_chunk_bounds(&point(-1, 0)).extent(), horizontal_size);
        assert_eq!(chunker.resolve_chunk_bounds(&point(4097, 4097)).extent(), horizontal_size);

        // Horizontal
        assert_eq!(chunker.resolve_chunk_bounds(&point(0, 0)).extent(), horizontal_size);
        assert_eq!(chunker.resolve_chunk_bounds(&point(257, 257)).extent(), horizontal_size);
        assert_eq!(chunker.resolve_chunk_bounds(&point(4095, 257)).extent(), horizontal_size);
        assert_eq!(chunker.resolve_chunk_bounds(&point(-1, -1)).extent(), horizontal_size);
        assert_eq!(chunker.resolve_chunk_bounds(&point(-1, -257)).extent(), horizontal_size);

        // Vertical
        assert_eq!(chunker.resolve_chunk_bounds(&point(4097, 0)).extent(), vertical_size);
        assert_eq!(chunker.resolve_chunk_bounds(&point(4097, 257)).extent(), vertical_size);
        assert_eq!(chunker.resolve_chunk_bounds(&point(4097+256, 257)).extent(), vertical_size);
        assert_eq!(chunker.resolve_chunk_bounds(&point(4097, 257+256)).extent(), vertical_size);
        assert_eq!(chunker.resolve_chunk_bounds(&point(-4097, -1)).extent(), vertical_size);
        assert_eq!(chunker.resolve_chunk_bounds(&point(-4097-256, -1)).extent(), vertical_size);
        assert_eq!(chunker.resolve_chunk_bounds(&point(-4097-256, -1-256)).extent(), vertical_size);
        assert_eq!(chunker.resolve_chunk_bounds(&point(10000, 0)).extent(), vertical_size);
        assert_eq!(chunker.resolve_chunk_bounds(&point(-10000, 0)).extent(), vertical_size);
    }

    #[test]
    fn strip_chunker_resolves_actually_intersecting_chunks_within_bounds() {
        let chunker = StripChunker {
            strip_length: Pow2::from_exponent(12),
            strip_thickness: Pow2::from_exponent(8),
        };

        let test_region = |region: GridRect| {
            let chunks = chunker.origins_of_intersecting_chunks(&region);
            let mut total_intersection_area = 0;
            for chunk_origin in chunks {
                let chunk_bounds = chunker.resolve_chunk_bounds(&chunk_origin.point());
                let intersection = chunk_bounds.intersection(&region);
                assert!(intersection.is_some());
                total_intersection_area += intersection.unwrap().width() * intersection.unwrap().height();
            }
            assert_eq!(total_intersection_area, region.width() * region.height());
        };

        test_region(GridRect::with_start_end(point(0, 0), point(100, 100)));
        test_region(GridRect::with_start_end(point(-10000, -9000), point(10000, 9000)));
        test_region(GridRect::with_start_end(point(-9000, -10000), point(10000, -5000)));
        test_region(GridRect::with_start_end(point(-10000, -9000), point(-5000, 10000)));
        test_region(GridRect::with_start_end(point(5000, -10000), point(10000, 9000)));
        test_region(GridRect::with_start_end(point(-10000, 5000), point(9000, 10000)));
    }

    #[test]
    fn strip_chunker_is_superset_of_square_chunker() {
        let square_chunker = SquareChunker { size: Pow2::from_exponent(6) };
        let strip_chunker = StripChunker {
            strip_length: Pow2::from_exponent(6),
            strip_thickness: Pow2::from_exponent(6),
        };

        for x in (-10000..10000).step_by(777) {
            for y in (-10000..10000).step_by(579) {
                let p = point(x, y);
                let r = GridRect::with_size(p, 2137, 1379);
                assert_eq!(square_chunker.resolve_chunk_origin(&p), strip_chunker.resolve_chunk_origin(&p));
                assert_eq!(square_chunker.resolve_chunk_bounds(&p), strip_chunker.resolve_chunk_bounds(&p));
                assert_eq!(square_chunker.origins_of_intersecting_chunks(&r), strip_chunker.origins_of_intersecting_chunks(&r));
            }
        }
    }
}
