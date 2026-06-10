use crate::io::{ReadFrom, WriteTo};
use crate::math::coords::Point2D;
use crate::math::pow2;
use crate::math::pow2::Pow2;
use std::cmp;
use std::io::{Read, Write};
use std::ops::{Add, BitAnd, Shl, Sub};

// Effectively forms a 2-dimensional [start, end) range.
#[derive(Hash, Eq, PartialEq, Ord, PartialOrd, Debug, Copy, Clone)]
pub struct Rect2D<T> {
    pub start: Point2D<T>,
    pub end: Point2D<T>,
}

pub type GridRect = Rect2D<i32>;

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