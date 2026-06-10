use crate::io::{ReadFrom, WriteTo};
use std::cmp;
use std::io::{Read, Write};
use std::ops::*;

#[derive(Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Copy, Clone)]
pub struct Point2D<T> {
    pub x: T,
    pub y: T,
}

#[derive(Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Copy, Clone)]
pub struct Vector2D<T> {
    pub x: T,
    pub y: T,
}

pub type GridPoint = Point2D<i32>;
pub type GridVector = Vector2D<i32>;

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

impl Point2D<i32> {
    pub fn chebyshev_distance_from_origin(&self) -> u32 {
        cmp::max(self.x.unsigned_abs(), self.y.unsigned_abs())
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
}
