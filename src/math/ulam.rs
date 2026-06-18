use crate::math::coords::{GridPoint, GridVector, Point2D};
use std::cmp::Ordering;
use std::ops::{Add, AddAssign, Sub};

#[derive(Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Copy, Clone)]
pub struct UlamSpiralPoint(u64);

impl UlamSpiralPoint {
    pub fn new(d: u64) -> UlamSpiralPoint {
        UlamSpiralPoint(d)
    }

    pub fn as_u64(self) -> u64 {
        self.0
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
    /// and it ends up being slower. This is probably due to longer dependency chain and the fact
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

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct UlamSpiralCursor {
    grid_position: GridPoint,
    spiral_position: UlamSpiralPoint,

    current_direction: GridVector,
    current_line: u32,
    steps_in_current_direction_left: u32,
}

impl PartialOrd<Self> for UlamSpiralCursor {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for UlamSpiralCursor {
    fn cmp(&self, other: &Self) -> Ordering {
        self.spiral_position.cmp(&other.spiral_position)
    }
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
            self.grid_position +=
                self.current_direction * self.steps_in_current_direction_left as i32;
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

#[cfg(test)]
mod tests {
    use super::*;

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
