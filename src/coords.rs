use crate::grid::{GridPoint, GridVector};
use crate::io::{ReadFrom, WriteTo};
use crate::util::pow2;
use crate::util::pow2::Pow2;
use std::cmp;
use std::io::{ErrorKind, Read, Write};
use std::ops::*;

#[derive(Hash, Eq, PartialEq, Ord, PartialOrd, Debug, Copy, Clone)]
pub struct Point2D<T> {
    pub x: T,
    pub y: T,
}

#[derive(Hash, Eq, PartialEq, Ord, PartialOrd, Debug, Copy, Clone)]
pub struct Vector2D<T> {
    pub x: T,
    pub y: T,
}

// Effectively forms a 2-dimensional [start, end) range.
#[derive(Hash, Eq, PartialEq, Ord, PartialOrd, Debug, Copy, Clone)]
pub struct Rect2D<T> {
    pub start: Point2D<T>,
    pub end: Point2D<T>,
}

#[derive(Hash, Eq, PartialEq, Ord, PartialOrd, Debug, Copy, Clone)]
pub struct UlamSpiralPoint(u64);

impl UlamSpiralPoint {
    pub fn new(d: u64) -> UlamSpiralPoint {
        UlamSpiralPoint(d)
    }

    pub fn as_usize(self) -> usize {
        self.0 as usize
    }

    pub fn as_isize(self) -> isize {
        self.0 as isize
    }
}

impl Sub for UlamSpiralPoint {
    type Output = i64;

    fn sub(self, rhs: UlamSpiralPoint) -> i64 {
        self.0 as i64 - rhs.0 as i64
    }
}

impl Add<u64> for UlamSpiralPoint {
    type Output = UlamSpiralPoint;

    fn add(self, rhs: u64) -> UlamSpiralPoint {
        UlamSpiralPoint(self.0 + rhs)
    }
}

impl AddAssign<u64> for UlamSpiralPoint {
    fn add_assign(&mut self, rhs: u64) {
        self.0 += rhs;
    }
}

impl<T> Point2D<T> {
    pub fn new(x: T, y: T) -> Point2D<T> {
        Point2D::<T> { x, y }
    }
}

impl<T> Vector2D<T> {
    pub fn new(x: T, y: T) -> Vector2D<T> {
        Vector2D::<T> { x, y }
    }
}

impl<T: Copy> Mul<T> for Vector2D<T>
where
    T: Mul<T, Output = T>,
{
    type Output = Self;

    fn mul(self, rhs: T) -> Self {
        Vector2D::<T> {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}

impl<T: Copy> Add<Vector2D<T>> for Point2D<T>
where
    T: Add<T, Output = T>,
{
    type Output = Self;

    fn add(self, rhs: Vector2D<T>) -> Self {
        Point2D::<T> {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl<T: Copy> AddAssign<Vector2D<T>> for Point2D<T>
where
    T: AddAssign<T>,
{
    fn add_assign(&mut self, rhs: Vector2D<T>) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl<T: Copy> Sub<Vector2D<T>> for Point2D<T>
where
    T: Sub<T, Output = T>,
{
    type Output = Self;

    fn sub(self, rhs: Vector2D<T>) -> Self {
        Point2D::<T> {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl<T: Copy> SubAssign<Vector2D<T>> for Point2D<T>
where
    T: SubAssign<T>,
{
    fn sub_assign(&mut self, rhs: Vector2D<T>) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

impl From<&Point2D<i32>> for UlamSpiralPoint {
    /// Heavily optimized from the simple form via algebraic transformations.
    ///
    /// ```
    /// use std::cmp::max;
    /// fn from(x: i64, y: i64) -> i64 {
    ///     let m = max(x.abs(), y.abs());
    ///
    ///     let base = 4 * m * (m - 1);
    ///     let p = if x == m && y != -m {
    ///         base + m + y
    ///     } else if y == m {
    ///         base + 3 * m - x
    ///     } else if x == -m {
    ///         base + 5 * m - y
    ///     } else if y == -m {
    ///         base + 7 * m + x
    ///     } else {
    ///         unreachable!()
    ///     };
    ///
    ///     p
    /// }
    /// ```
    ///
    /// There exists a version with one less branch, but it requires computing max(ax, ay)
    /// and it ends up being slower, probably due to longer dependency chain and the fact
    /// that the branches are very predictable in our current use of this function.
    fn from(point: &Point2D<i32>) -> Self {
        let x = point.x as i64;
        let y = point.y as i64;

        let ax = x.abs();
        let ay = y.abs();

        let p = if -y >= ax {
            (4 * y - 3) * y + x
        } else if y >= ax {
            (4 * y - 1) * y - x
        } else if x >= ay {
            (4 * x - 3) * x + y
        } else
        /* if -x >= ay */
        {
            (4 * x - 1) * x - y
        };

        UlamSpiralPoint(p as u64)
    }
}

impl Point2D<i32> {
    pub fn chebyshev_distance_from_origin(&self) -> u32 {
        cmp::max(self.x.unsigned_abs(), self.y.unsigned_abs())
    }
}

pub struct UlamSpiralCursor {
    grid_position: GridPoint,
    spiral_position: UlamSpiralPoint,

    current_direction: GridVector,
    current_line: u32,
    steps_in_current_direction_left: u32,
}

impl Default for UlamSpiralCursor {
    fn default() -> Self {
        Self::new()
    }
}

impl UlamSpiralCursor {
    pub fn new() -> UlamSpiralCursor {
        UlamSpiralCursor {
            grid_position: GridPoint::new(0, 0),
            spiral_position: UlamSpiralPoint(0),
            current_direction: GridVector::new(1, 0),
            current_line: 0,
            steps_in_current_direction_left: 1,
        }
    }

    pub fn advance(&mut self) {
        self.grid_position += self.current_direction;
        self.spiral_position += 1;
        self.steps_in_current_direction_left -= 1;

        if self.steps_in_current_direction_left == 0 {
            self.current_line += 1;
            self.steps_in_current_direction_left = self.current_line / 2 + 1;
            // Rotate 90 degrees counter-clockwise
            self.current_direction =
                GridVector::new(-self.current_direction.y, self.current_direction.x);
        }
    }

    pub fn advance_to(&mut self, to: UlamSpiralPoint) {
        // Extract predictable branch
        if self.spiral_position == to {
            return;
        }

        while (to - self.spiral_position) >= self.steps_in_current_direction_left as i64 {
            self.grid_position += self.current_direction * self.steps_in_current_direction_left as i32;
            self.spiral_position += self.steps_in_current_direction_left as u64;
            self.current_line += 1;
            self.steps_in_current_direction_left = self.current_line / 2 + 1;
            self.current_direction =
                GridVector::new(-self.current_direction.y, self.current_direction.x);
        }

        let diff = to - self.spiral_position;
        if diff > 0 {
            self.grid_position += self.current_direction * diff as i32;
            self.spiral_position += diff as u64;
            self.steps_in_current_direction_left -= diff as u32;
        }
    }

    pub fn grid_position(&self) -> GridPoint {
        self.grid_position
    }

    pub fn spiral_position(&self) -> UlamSpiralPoint {
        self.spiral_position
    }
}

impl<T: Add<Output = T> + Clone + Copy> Rect2D<T> {
    pub fn with_size(start: Point2D<T>, width: T, height: T) -> Rect2D<T> {
        let end = Point2D::new(start.x + width, start.y + height);
        Rect2D::<T> { start, end }
    }

    pub fn square_with_size(start: Point2D<T>, size: T) -> Rect2D<T> {
        let end = Point2D::new(start.x + size, start.y + size);
        Rect2D::<T> { start, end }
    }

    pub fn with_start_end(start: Point2D<T>, end: Point2D<T>) -> Rect2D<T> {
        Rect2D::<T> { start, end }
    }
}

impl<
    T: BitAnd<Output = T> + From<u8> + Shl<Output = T> + Sub<Output = T> + PartialEq<T> + Clone + Copy,
> Rect2D<T>
{
    pub fn is_aligned_to_pow2(&self, align: Pow2) -> bool {
        pow2::floor_mod(self.start.x, align) == T::from(0)
            && pow2::floor_mod(self.start.y, align) == T::from(0)
            && pow2::floor_mod(self.end.x, align) == T::from(0)
            && pow2::floor_mod(self.end.y, align) == T::from(0)
    }
}

impl<T: Clone + Copy> Rect2D<T> {
    pub fn start(&self) -> Point2D<T> {
        self.start
    }

    pub fn end(&self) -> Point2D<T> {
        self.end
    }
}

impl<T: Sub<Output = T> + Clone + Copy> Rect2D<T> {
    pub fn width(&self) -> T {
        self.end.x - self.start.x
    }

    pub fn height(&self) -> T {
        self.end.y - self.start.y
    }
}

impl<T: Ord + Add<Output = T> + Copy + Clone> Rect2D<T> {
    pub fn contains_point(&self, point: &Point2D<T>) -> bool {
        point.x >= self.start.x
            && point.y >= self.start.y
            && point.x < self.end.x
            && point.y < self.end.y
    }

    pub fn contains(&self, other: &Rect2D<T>) -> bool {
        self.start.x <= other.start.x
            && self.start.y <= other.start.y
            && self.end.x >= other.end.x
            && self.end.y >= other.end.y
    }

    pub fn intersection(&self, other: &Rect2D<T>) -> Option<Rect2D<T>> {
        let start = Point2D::new(
            cmp::max(self.start.x, other.start.x),
            cmp::max(self.start.y, other.start.y),
        );
        let end = Point2D::new(
            cmp::min(self.end.x, other.end.x),
            cmp::min(self.end.y, other.end.y),
        );
        if start.x < end.x && start.y < end.y {
            Some(Rect2D::with_start_end(start, end))
        } else {
            None
        }
    }
}

impl<T> WriteTo for Point2D<T>
where
    T: WriteTo,
{
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        self.x.write_to(writer)?;
        self.y.write_to(writer)?;
        Ok(())
    }
}

impl<T> ReadFrom for Point2D<T>
where
    T: ReadFrom,
{
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        Ok(Point2D {
            x: T::read_from(reader)?,
            y: T::read_from(reader)?,
        })
    }
}

impl<T> WriteTo for Vector2D<T>
where
    T: WriteTo,
{
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        self.x.write_to(writer)?;
        self.y.write_to(writer)?;
        Ok(())
    }
}

impl<T> ReadFrom for Vector2D<T>
where
    T: ReadFrom,
{
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        Ok(Vector2D {
            x: T::read_from(reader)?,
            y: T::read_from(reader)?,
        })
    }
}

impl<T> WriteTo for Rect2D<T>
where
    T: WriteTo,
{
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        self.start.write_to(writer)?;
        self.end.write_to(writer)?;
        Ok(())
    }
}

impl<T> ReadFrom for Rect2D<T>
where
    T: ReadFrom,
{
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        Ok(Rect2D {
            start: Point2D::<T>::read_from(reader)?,
            end: Point2D::<T>::read_from(reader)?,
        })
    }
}

pub const ULS_MAX_CURSOR_OFFSET: usize = 2_000_000_000;

impl WriteTo for UlamSpiralPoint {
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        self.0.write_to(writer)
    }
}

impl ReadFrom for UlamSpiralPoint {
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        Ok(UlamSpiralPoint(u64::read_from(reader)?))
    }
}

impl WriteTo for UlamSpiralCursor {
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        if self.grid_position.x.unsigned_abs() as usize > ULS_MAX_CURSOR_OFFSET
            || self.grid_position.y.unsigned_abs() as usize > ULS_MAX_CURSOR_OFFSET
        {
            return Err(std::io::Error::new(
                ErrorKind::InvalidData,
                format!("Cursor is farther than {}", ULS_MAX_CURSOR_OFFSET),
            ));
        }

        // The whole structure can be easily restored from just the spiral index,
        // and it also allows us to avoid any consistency issues.
        self.spiral_position.write_to(writer)
    }
}

impl ReadFrom for UlamSpiralCursor {
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        let mut cursor = UlamSpiralCursor::new();
        // TODO: O(1) set instead of advancing by lines.
        cursor.advance_to(UlamSpiralPoint::read_from(reader)?);
        if cursor.grid_position.x.unsigned_abs() as usize > ULS_MAX_CURSOR_OFFSET
            || cursor.grid_position.y.unsigned_abs() as usize > ULS_MAX_CURSOR_OFFSET
        {
            return Err(std::io::Error::new(
                ErrorKind::InvalidData,
                format!("Cursor is farther than {}", ULS_MAX_CURSOR_OFFSET),
            ));
        }

        Ok(cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_add_vector_to_a_point() {
        let p = Point2D::new(1, 2);
        let v = Vector2D::new(3, 4);
        let translated = p + v;
        assert_eq!(translated.x, 4);
        assert_eq!(translated.y, 6);
    }

    #[test]
    fn can_sub_vector_from_a_point() {
        let p = Point2D::new(1, 2);
        let v = Vector2D::new(3, 4);
        let translated = p - v;
        assert_eq!(translated.x, -2);
        assert_eq!(translated.y, -2);
    }

    #[test]
    fn can_convert_point_to_ulam_spiral_point() {
        let p = Point2D::new(0, 0);
        let u = UlamSpiralPoint::from(&p);
        assert_eq!(u.0, 0);

        let p = Point2D::new(1, 0);
        let u = UlamSpiralPoint::from(&p);
        assert_eq!(u.0, 1);

        let p = Point2D::new(1, 1);
        let u = UlamSpiralPoint::from(&p);
        assert_eq!(u.0, 2);

        let p = Point2D::new(0, 1);
        let u = UlamSpiralPoint::from(&p);
        assert_eq!(u.0, 3);

        let p = Point2D::new(-1, 1);
        let u = UlamSpiralPoint::from(&p);
        assert_eq!(u.0, 4);

        let p = Point2D::new(-1, -1);
        let u = UlamSpiralPoint::from(&p);
        assert_eq!(u.0, 6);

        let p = Point2D::new(-2, -2);
        let u = UlamSpiralPoint::from(&p);
        assert_eq!(u.0, 20);
    }

    #[test]
    fn new_cursor_starts_at_origin() {
        let cursor = UlamSpiralCursor::new();

        assert_eq!(cursor.grid_position, GridPoint::new(0, 0));
        assert_eq!(cursor.spiral_position, UlamSpiralPoint(0));
    }

    #[test]
    fn cursor_advances_correctly() {
        let mut cursor = UlamSpiralCursor::new();

        // Test a few turns worth
        cursor.advance();
        assert_eq!(cursor.grid_position, GridPoint::new(1, 0));
        assert_eq!(cursor.spiral_position, UlamSpiralPoint(1));

        cursor.advance();
        assert_eq!(cursor.grid_position, GridPoint::new(1, 1));
        assert_eq!(cursor.spiral_position, UlamSpiralPoint(2));

        cursor.advance();
        assert_eq!(cursor.grid_position, GridPoint::new(0, 1));
        assert_eq!(cursor.spiral_position, UlamSpiralPoint(3));

        cursor.advance();
        assert_eq!(cursor.grid_position, GridPoint::new(-1, 1));
        assert_eq!(cursor.spiral_position, UlamSpiralPoint(4));

        cursor.advance();
        assert_eq!(cursor.grid_position, GridPoint::new(-1, 0));
        assert_eq!(cursor.spiral_position, UlamSpiralPoint(5));

        cursor.advance();
        assert_eq!(cursor.grid_position, GridPoint::new(-1, -1));
        assert_eq!(cursor.spiral_position, UlamSpiralPoint(6));

        cursor.advance();
        assert_eq!(cursor.grid_position, GridPoint::new(0, -1));
        assert_eq!(cursor.spiral_position, UlamSpiralPoint(7));
    }

    fn some_rect() -> Rect2D<i32> {
        Rect2D::with_size(Point2D::new(8, 9), 12, 13)
    }

    #[test]
    fn rect_contains_its_start() {
        let rect = some_rect();
        assert!(rect.contains_point(&rect.start));
    }

    #[test]
    fn rect_does_not_contain_its_end() {
        let rect = some_rect();
        assert!(!rect.contains_point(&rect.end));
    }

    #[test]
    fn rect_contains_itself() {
        let rect = some_rect();
        assert!(rect.contains(&rect));
    }

    #[test]
    fn rect_intersected_with_itself_stays_the_same() {
        let rect = some_rect();
        let intersection = rect.intersection(&rect).unwrap();
        assert_eq!(rect, intersection);
    }

    #[test]
    fn touching_rects_do_not_intersect() {
        // In other words, no degenerate intersections.
        let rect1 = Rect2D::with_size(Point2D::new(0, 0), 10, 10);
        let rect2 = Rect2D::with_size(Point2D::new(10, 0), 10, 10);
        assert!(rect1.intersection(&rect2).is_none());
    }

    #[test]
    fn rect_intersection_is_contained_by_both_rects() {
        let rect1 = Rect2D::with_size(Point2D::new(0, 0), 10, 10);
        let rect2 = Rect2D::with_size(Point2D::new(5, 5), 10, 10);
        let intersection = rect1.intersection(&rect2).unwrap();
        assert!(rect1.contains(&intersection));
        assert!(rect2.contains(&intersection));
    }
}
