use std::cmp::max;
use std::ops::*;
use crate::grid::{GridPoint, GridVector};

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

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone)]
pub struct UlamSpiralPoint(i64);

impl UlamSpiralPoint {
    pub fn index(self) -> isize {
        self.0 as isize
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

pub struct UlamSpiralCursor {
    grid_position: GridPoint,
    spiral_position: UlamSpiralPoint,

    current_direction: GridVector,
    current_line: usize,
    steps_in_current_direction_left: usize,
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
        self.grid_position = self.grid_position + self.current_direction;
        self.spiral_position.0 += 1;
        self.steps_in_current_direction_left -= 1;

        if self.steps_in_current_direction_left == 0 {
            self.current_line += 1;
            self.steps_in_current_direction_left = self.current_line / 2 + 1;
            // Rotate 90 degrees counter-clockwise
            self.current_direction = GridVector::new(-self.current_direction.y, self.current_direction.x);
        }
    }

    pub fn grid_position(&self) -> GridPoint {
        self.grid_position
    }

    pub fn spiral_position(&self) -> UlamSpiralPoint {
        self.spiral_position
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
}
