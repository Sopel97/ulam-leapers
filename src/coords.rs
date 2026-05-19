use std::cmp::max;
use std::ops::*;

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone)]
pub struct Point2D<T> {
    pub x: T,
    pub y: T,
}

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone)]
pub struct Vector2D<T> {
    pub x: T,
    pub y: T,
}

pub struct UlamSpiralPoint(i64);

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

impl From<&Point2D<i32>> for UlamSpiralPoint {
    fn from(point: &Point2D<i32>) -> Self {
        let x = point.x as i64;
        let y = point.y as i64;
        let m = max(x.abs(), y.abs());

        let base = 4 * m * (m - 1);
        let p = if x == m && y != -m {
            base + m + y
        } else if y == m {
            base + 3 * m - x
        } else if x == -m {
            base + 5 * m - y
        } else if y == -m {
            base + 7 * m + x
        } else {
            unreachable!()
        };

        UlamSpiralPoint(p)
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
}
