use crate::io::{ReadFrom, WriteTo};
use crate::math::coords::{Point2D, Vector2D};
use crate::math::pow2;
use crate::math::pow2::Pow2;
use std::cmp;
use std::io::{Read, Write};
use std::ops::{Add, AddAssign, Shl, Shr, Sub};

/// Models a 2-dimensional range inclusive at `start` and exclusive at `end`.
/// The rectangle's projection onto the x-axis is `start.x .. end.x`.
/// The rectangle's projection onto the y-axis is `start.y .. end.y`.
/// Supports both floating point and integer coordinate types.
#[derive(Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Copy, Clone)]
pub struct Rect2D<T> {
    pub start: Point2D<T>,
    pub end: Point2D<T>,
}

pub type GridRect = Rect2D<i32>;

impl<T> Rect2D<T> {
    pub fn with_start_end(start: Point2D<T>, end: Point2D<T>) -> Rect2D<T> {
        Rect2D::<T> { start, end }
    }
}

impl<T: From<u8>> Rect2D<T> {
    pub fn zero() -> Self {
        Self::with_start_end(Point2D::<T>::zero(), Point2D::<T>::zero())
    }
}

impl<T> Rect2D<T>
where
    T: Add<Output = T> + Clone + Copy,
{
    pub fn with_size(start: Point2D<T>, width: T, height: T) -> Rect2D<T> {
        let end = Point2D::new(start.x + width, start.y + height);
        Rect2D::<T> { start, end }
    }

    pub fn with_extent(start: Point2D<T>, extent: Vector2D<T>) -> Rect2D<T> {
        Rect2D::<T> {
            start,
            end: start + extent,
        }
    }

    pub fn square_with_size(start: Point2D<T>, size: T) -> Rect2D<T> {
        let end = Point2D::new(start.x + size, start.y + size);
        Rect2D::<T> { start, end }
    }
}

impl<T> Rect2D<T>
where
    T: Shl<Output = T> + Shr<Output = T> + From<u8> + Eq + Clone + Copy,
{
    pub fn is_aligned_to_pow2(&self, align: Pow2) -> bool {
        pow2::is_multiple_of(self.start.x, align)
            && pow2::is_multiple_of(self.start.y, align)
            && pow2::is_multiple_of(self.end.x, align)
            && pow2::is_multiple_of(self.end.y, align)
    }
}

impl<T> Rect2D<T>
where
    T: Clone + Copy,
{
    pub fn start(&self) -> Point2D<T> {
        self.start
    }

    pub fn end(&self) -> Point2D<T> {
        self.end
    }
}

impl<T> Rect2D<T>
where
    T: Sub<Output = T> + Clone + Copy,
{
    pub fn width(&self) -> T {
        self.end.x - self.start.x
    }

    pub fn height(&self) -> T {
        self.end.y - self.start.y
    }

    pub fn extent(&self) -> Vector2D<T> {
        Vector2D::new(self.width(), self.height())
    }
}

impl<T> Rect2D<T>
where
    T: Ord + Add<Output = T> + Copy + Clone,
{
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

    pub fn intersects(&self, other: &Rect2D<T>) -> bool {
        self.intersection(other).is_some()
    }
}

impl<T> Rect2D<T>
where
    T: AddAssign + Copy,
{
    pub fn translate(&mut self, other: Vector2D<T>) {
        self.start += other;
        self.end += other;
    }
}

impl<T> Rect2D<T>
where
    T: Add<Output = T> + Copy,
{
    pub fn translated(&self, offset: Vector2D<T>) -> Self {
        Self {
            start: self.start + offset,
            end: self.end + offset,
        }
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

#[cfg(test)]
mod test {
    use super::*;

    fn p(x: i32, y: i32) -> Point2D<i32> {
        Point2D::new(x, y)
    }

    fn v(x: i32, y: i32) -> Vector2D<i32> {
        Vector2D::new(x, y)
    }

    fn some_rect() -> Rect2D<i32> {
        Rect2D::with_size(p(8, 9), 12, 13)
    }

    #[test]
    fn constructible_from_start_and_end() {
        let r = Rect2D::with_start_end(p(1, 2), p(5, 6));
        assert_eq!(r.start, p(1, 2));
        assert_eq!(r.end, p(5, 6));
    }

    #[test]
    fn zero_rect_has_zero_coordinates() {
        let r: Rect2D<i32> = Rect2D::zero();
        assert_eq!(r.start, p(0, 0));
        assert_eq!(r.end, p(0, 0));
    }

    #[test]
    fn constructible_with_size() {
        let r = Rect2D::with_size(p(2, 3), 4, 5);
        assert_eq!(r.start, p(2, 3));
        assert_eq!(r.end, p(6, 8));
    }

    #[test]
    fn constructible_with_extent() {
        let r = Rect2D::with_extent(p(1, 1), v(3, 4));
        assert_eq!(r.start, p(1, 1));
        assert_eq!(r.end, p(4, 5));
    }

    #[test]
    fn constructible_as_square_with_size() {
        let r = Rect2D::square_with_size(p(0, 0), 3);
        assert_eq!(r.start, p(0, 0));
        assert_eq!(r.end, p(3, 3));
    }

    #[test]
    fn test_width_height() {
        let r = Rect2D::with_start_end(p(2, 3), p(7, 9));
        assert_eq!(r.width(), 5);
        assert_eq!(r.height(), 6);
    }

    #[test]
    fn contains_points_inside() {
        let r = Rect2D::with_start_end(p(0, 0), p(5, 5));

        assert!(r.contains_point(&p(0, 0))); // inclusive start
        assert!(r.contains_point(&p(4, 4))); // inside
    }

    #[test]
    fn does_not_contain_points_on_exclusive_end() {
        let r = Rect2D::with_start_end(p(0, 0), p(5, 5));

        assert!(!r.contains_point(&p(5, 5))); // exclusive end
        assert!(!r.contains_point(&p(5, 0)));
        assert!(!r.contains_point(&p(0, 5)));
    }

    #[test]
    fn contains_smaller_rect_inside() {
        let outer = Rect2D::with_start_end(p(0, 0), p(10, 10));
        let inner = Rect2D::with_start_end(p(2, 2), p(8, 8));

        assert!(outer.contains(&inner));
    }

    #[test]
    fn does_not_contain_partially_overlapping_rect() {
        let outer = Rect2D::with_start_end(p(0, 0), p(10, 10));
        let inner = Rect2D::with_start_end(p(-1, 2), p(8, 8));

        assert!(!outer.contains(&inner));
    }

    #[test]
    fn overlapping_rects_intersect() {
        let a = Rect2D::with_start_end(p(0, 0), p(5, 5));
        let b = Rect2D::with_start_end(p(3, 3), p(7, 7));

        let inter = a.intersection(&b).unwrap();

        assert_eq!(inter.start, p(3, 3));
        assert_eq!(inter.end, p(5, 5));
    }

    #[test]
    fn disjoint_rects_do_not_intersect() {
        let a = Rect2D::with_start_end(p(0, 0), p(2, 2));
        let b = Rect2D::with_start_end(p(3, 3), p(5, 5));

        assert!(a.intersection(&b).is_none());
    }

    #[test]
    fn test_intersection_touching_edges_not_intersecting() {
        let a = Rect2D::with_start_end(p(0, 0), p(2, 2));
        let b = Rect2D::with_start_end(p(2, 0), p(4, 4));

        assert!(a.intersection(&b).is_none());
    }

    #[test]
    fn can_be_translated() {
        let mut r = Rect2D::with_start_end(p(1, 1), p(3, 3));
        r.translate(v(2, 3));

        assert_eq!(r.start, p(3, 4));
        assert_eq!(r.end, p(5, 6));
    }

    #[test]
    fn can_produce_translated_rect() {
        let r = Rect2D::with_start_end(p(1, 1), p(3, 3));
        let moved = r.translated(v(10, 20));

        assert_eq!(r.start, p(1, 1));
        assert_eq!(moved.start, p(11, 21));
    }

    #[test]
    fn line_rect_does_not_contain_points_on_the_line() {
        let r = Rect2D::with_start_end(p(5, 5), p(5, 10));

        assert_eq!(r.width(), 0);
        assert_eq!(r.height(), 5);
        assert!(!r.contains_point(&p(5, 5))); // empty in x
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
