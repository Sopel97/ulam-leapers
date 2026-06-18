use std::cmp;
use std::collections::HashSet;
use std::ops::*;

/// Models a point in a 2-dimensional Cartesian coordinate system.
/// Supports both integer and floating point coordinates.
#[derive(Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Copy, Clone)]
pub struct Point2D<T> {
    pub x: T,
    pub y: T,
}

/// Models a vector in a 2-dimensional Cartesian coordinate system.
/// Supports both integer and floating point coordinates.
#[derive(Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Copy, Clone)]
pub struct Vector2D<T> {
    pub x: T,
    pub y: T,
}

pub type GridPoint = Point2D<i32>;
pub type GridVector = Vector2D<i32>;

impl<T> Point2D<T> {
    pub fn new(x: T, y: T) -> Self {
        Self { x, y }
    }
}

impl<T: Copy> Point2D<T> {
    pub fn splat(xy: T) -> Self {
        Self { x: xy, y: xy }
    }
}

impl<T: From<u8>> Point2D<T> {
    pub fn zero() -> Self {
        Self {
            x: T::from(0),
            y: T::from(0),
        }
    }
}

impl<T> Point2D<T> {
    pub fn map_coords<F, U>(self, mut f: F) -> Point2D<U>
    where
        F: FnMut(T) -> U,
    {
        Point2D {
            x: f(self.x),
            y: f(self.y),
        }
    }
}

impl<T> Vector2D<T> {
    pub fn new(x: T, y: T) -> Self {
        Self { x, y }
    }
}

impl<T: Copy> Vector2D<T> {
    pub fn splat(xy: T) -> Self {
        Self { x: xy, y: xy }
    }
}

impl<T: From<u8>> Vector2D<T> {
    pub fn zero() -> Self {
        Self {
            x: T::from(0),
            y: T::from(0),
        }
    }
}

impl<T> Vector2D<T> {
    pub fn map_coords<F, U>(self, mut f: F) -> Vector2D<U>
    where
        F: FnMut(T) -> U,
    {
        Vector2D {
            x: f(self.x),
            y: f(self.y),
        }
    }
}

impl<T> Mul<T> for Vector2D<T>
where
    T: Mul<T, Output = T> + Copy,
{
    type Output = Self;

    fn mul(self, rhs: T) -> Self {
        Vector2D::<T> {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}

impl<T> Add<Vector2D<T>> for Point2D<T>
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

impl<T> AddAssign<Vector2D<T>> for Point2D<T>
where
    T: AddAssign<T>,
{
    fn add_assign(&mut self, rhs: Vector2D<T>) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl<T> Sub<Vector2D<T>> for Point2D<T>
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

impl<T> Sub<Point2D<T>> for Point2D<T>
where
    T: Sub<T, Output = T>,
{
    type Output = Vector2D<T>;

    fn sub(self, rhs: Point2D<T>) -> Self::Output {
        Vector2D::<T> {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl<T> SubAssign<Vector2D<T>> for Point2D<T>
where
    T: SubAssign<T>,
{
    fn sub_assign(&mut self, rhs: Vector2D<T>) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

impl<T> Point2D<T>
where
    T: Copy,
{
    pub fn vector_from_origin(&self) -> Vector2D<T> {
        Vector2D::<T>::new(self.x, self.y)
    }
}

impl<T> Point2D<T>
where
    T: Copy + Neg<Output = T>,
{
    pub fn vector_to_origin(&self) -> Vector2D<T> {
        Vector2D::<T>::new(-self.x, -self.y)
    }
}

impl<T> Point2D<T>
where
    T: Mul<Output = T> + Add<Output = T> + Sub<Output = T> + Copy,
{
    pub fn squared_euclidean_distance(&self, other: &Point2D<T>) -> T {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        dx * dx + dy * dy
    }

    pub fn squared_euclidean_distance_to_origin(&self) -> T {
        self.x * self.x + self.y * self.y
    }
}

impl Point2D<f32> {
    pub fn euclidean_distance(&self, other: &Self) -> f32 {
        self.squared_euclidean_distance(other).sqrt()
    }

    pub fn euclidean_distance_to_origin(&self) -> f32 {
        self.squared_euclidean_distance_to_origin().sqrt()
    }
}

impl Point2D<f64> {
    pub fn euclidean_distance(&self, other: &Self) -> f64 {
        self.squared_euclidean_distance(other).sqrt()
    }
    pub fn euclidean_distance_to_origin(&self) -> f64 {
        self.squared_euclidean_distance_to_origin().sqrt()
    }
}

impl Point2D<i32> {
    pub fn chebyshev_distance_to_origin(&self) -> u32 {
        cmp::max(self.x.unsigned_abs(), self.y.unsigned_abs())
    }

    pub fn chebyshev_distance(&self, other: &Point2D<i32>) -> u32 {
        cmp::max(
            (self.x - other.x).unsigned_abs(),
            (self.y - other.y).unsigned_abs(),
        )
    }
}

impl<T> Vector2D<T>
where
    T: Mul<Output = T> + Add<Output = T> + Copy,
{
    pub fn squared_length(&self) -> T {
        self.x * self.x + self.y * self.y
    }
}

impl Vector2D<f32> {
    pub fn length(&self) -> f32 {
        self.squared_length().sqrt()
    }
}

impl Vector2D<f64> {
    pub fn length(&self) -> f64 {
        self.squared_length().sqrt()
    }
}

pub fn symmetries(v: &GridVector) -> impl Iterator<Item = GridVector> {
    // We could have assembled these via different cases instead of always computing all
    // of them and then deduplicating, but this is simpler and performance does not matter here.
    [
        GridVector::new(v.x, v.y),
        GridVector::new(-v.y, v.x),
        GridVector::new(-v.x, -v.y),
        GridVector::new(v.y, -v.x),
        GridVector::new(-v.x, v.y),
        GridVector::new(v.y, v.x),
        GridVector::new(v.x, -v.y),
        GridVector::new(-v.y, -v.x),
    ]
    .into_iter()
    .collect::<HashSet<GridVector>>()
    .into_iter()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point2d_new() {
        let p = Point2D::new(3, 7);
        assert_eq!(p.x, 3);
        assert_eq!(p.y, 7);
    }

    #[test]
    fn point2d_splat() {
        let p = Point2D::splat(5i32);
        assert_eq!(p.x, 5);
        assert_eq!(p.y, 5);
    }

    #[test]
    fn point2d_zero() {
        let p = Point2D::<i32>::zero();
        assert_eq!(p.x, 0);
        assert_eq!(p.y, 0);
    }

    #[test]
    fn point2d_zero_f64() {
        let p = Point2D::<f64>::zero();
        assert_eq!(p.x, 0.0);
        assert_eq!(p.y, 0.0);
    }

    #[test]
    fn vector2d_new() {
        let v = Vector2D::new(1, -2);
        assert_eq!(v.x, 1);
        assert_eq!(v.y, -2);
    }

    #[test]
    fn vector2d_splat() {
        let v = Vector2D::splat(4i32);
        assert_eq!(v.x, 4);
        assert_eq!(v.y, 4);
    }

    #[test]
    fn vector2d_zero() {
        let v = Vector2D::<i32>::zero();
        assert_eq!(v.x, 0);
        assert_eq!(v.y, 0);
    }

    #[test]
    fn vector2d_mul_positive_scalar() {
        let v = Vector2D::new(3, -4);
        let result = v * 2;
        assert_eq!(result, Vector2D::new(6, -8));
    }

    #[test]
    #[allow(clippy::erasing_op)]
    fn vector2d_mul_zero() {
        let v = Vector2D::new(10, -5);
        assert_eq!(v * 0, Vector2D::new(0, 0));
    }

    #[test]
    fn vector2d_mul_one() {
        let v = Vector2D::new(3, 7);
        assert_eq!(v * 1, v);
    }

    #[test]
    fn vector2d_mul_negative_scalar() {
        let v = Vector2D::new(2, 3);
        assert_eq!(v * -1, Vector2D::new(-2, -3));
    }

    #[test]
    fn vector2d_mul_f64() {
        let v = Vector2D::new(1.0_f64, 2.0_f64);
        let result = v * 2.5;
        assert_eq!(result, Vector2D::new(2.5, 5.0));
    }

    #[test]
    fn point2d_add_vector() {
        let p = Point2D::new(1, 2);
        let v = Vector2D::new(3, 4);
        assert_eq!(p + v, Point2D::new(4, 6));
    }

    #[test]
    fn point2d_add_zero_vector() {
        let p = Point2D::new(5, -3);
        assert_eq!(p + Vector2D::zero(), p);
    }

    #[test]
    fn point2d_add_assign_vector() {
        let mut p = Point2D::new(1, 2);
        p += Vector2D::new(3, 4);
        assert_eq!(p, Point2D::new(4, 6));
    }

    #[test]
    fn point2d_sub_vector() {
        let p = Point2D::new(5, 7);
        let v = Vector2D::new(2, 3);
        assert_eq!(p - v, Point2D::new(3, 4));
    }

    #[test]
    fn point2d_sub_zero_vector() {
        let p = Point2D::new(5, -3);
        assert_eq!(p - Vector2D::zero(), p);
    }

    #[test]
    fn point2d_sub_assign_vector() {
        let mut p = Point2D::new(5, 7);
        p -= Vector2D::new(2, 3);
        assert_eq!(p, Point2D::new(3, 4));
    }

    #[test]
    fn point2d_add_then_sub_roundtrip() {
        let p = Point2D::new(4, -2);
        let v = Vector2D::new(3, 7);
        assert_eq!(p + v - v, p);
    }

    #[test]
    fn vector_from_origin() {
        let p = Point2D::new(3, -5);
        assert_eq!(p.vector_from_origin(), Vector2D::new(3, -5));
    }

    #[test]
    fn vector_to_origin() {
        let p = Point2D::new(3, -5);
        assert_eq!(p.vector_to_origin(), Vector2D::new(-3, 5));
    }

    #[test]
    fn origin_vectors_are_inverses() {
        let p = Point2D::new(7i32, -4i32);
        let from = p.vector_from_origin();
        let to = p.vector_to_origin();
        assert_eq!(from.x + to.x, 0);
        assert_eq!(from.y + to.y, 0);
    }

    #[test]
    #[allow(clippy::identity_op)]
    fn squared_euclidean_distance_i32() {
        let a = Point2D::new(3i32, 4i32);
        let b = Point2D::new(1i32, 2i32);
        assert_eq!(a.squared_euclidean_distance(&b), 4 + 4);
    }

    #[test]
    fn squared_euclidean_distance_to_origin() {
        let p = Point2D::new(3i32, 4i32);
        assert_eq!(p.squared_euclidean_distance_to_origin(), 25);
    }

    #[test]
    fn squared_euclidean_distance_to_origin_f64() {
        let p = Point2D::new(3.0_f64, 4.0_f64);
        assert_eq!(p.squared_euclidean_distance_to_origin(), 25.0);
    }

    #[test]
    fn euclidean_distance_to_origin_f32_345() {
        let p = Point2D::new(3.0_f32, 4.0_f32);
        assert!((p.euclidean_distance_to_origin() - 5.0).abs() < 1e-6);
    }

    #[test]
    fn euclidean_distance_to_origin_f64_345() {
        let p = Point2D::new(3.0_f64, 4.0_f64);
        assert!((p.euclidean_distance_to_origin() - 5.0).abs() < 1e-12);
    }

    #[test]
    fn euclidean_distance_f64_between_two_points() {
        // distance between (0,0) and (3,4) should be 5
        let a = Point2D::<f64>::zero();
        let b = Point2D::new(3.0_f64, 4.0_f64);
        let dist = a.euclidean_distance(&b);
        assert!((dist - 5.0).abs() < 1e-12);
    }

    #[test]
    fn vector_squared_length() {
        let v = Vector2D::new(3, 4);
        assert_eq!(v.squared_length(), 25);
    }

    #[test]
    fn vector_length() {
        let v = Vector2D::<f64>::new(3.0, 4.0);
        assert!((v.length() - 5.0).abs() < 1e-12);
    }

    #[test]
    fn chebyshev_distance_to_origin_positive() {
        let p = Point2D::new(3i32, 4i32);
        assert_eq!(p.chebyshev_distance_to_origin(), 4);
    }

    #[test]
    fn chebyshev_distance_to_origin_negative_coords() {
        let p = Point2D::new(-5i32, 2i32);
        assert_eq!(p.chebyshev_distance_to_origin(), 5);
    }

    #[test]
    fn chebyshev_distance_to_origin_zero() {
        let p = Point2D::<i32>::zero();
        assert_eq!(p.chebyshev_distance_to_origin(), 0);
    }

    #[test]
    fn chebyshev_distance_to_origin_equal_axes() {
        let p = Point2D::new(3i32, 3i32);
        assert_eq!(p.chebyshev_distance_to_origin(), 3);
    }

    #[test]
    fn chebyshev_distance_between_points() {
        let a = Point2D::new(1i32, 1i32);
        let b = Point2D::new(4i32, -2i32);
        assert_eq!(a.chebyshev_distance(&b), 3);
    }

    #[test]
    fn chebyshev_distance_same_point() {
        let p = Point2D::new(7i32, -3i32);
        assert_eq!(p.chebyshev_distance(&p), 0);
    }

    #[test]
    fn point2d_eq() {
        assert_eq!(Point2D::new(1, 2), Point2D::new(1, 2));
        assert_ne!(Point2D::new(1, 2), Point2D::new(2, 1));
    }

    #[test]
    fn point2d_copy() {
        let a = Point2D::new(1i32, 2i32);
        let b = a; // Copy
        assert_eq!(a, b);
    }

    #[test]
    fn point2d_ord() {
        assert!(Point2D::new(1, 2) < Point2D::new(1, 3));
        assert!(Point2D::new(0, 9) < Point2D::new(1, 0));
    }

    #[test]
    fn vector2d_eq() {
        assert_eq!(Vector2D::new(3, 4), Vector2D::new(3, 4));
        assert_ne!(Vector2D::new(3, 4), Vector2D::new(4, 3));
    }

    #[test]
    fn grid_point_and_grid_vector_are_i32() {
        let p: GridPoint = Point2D::new(1, 2);
        let v: GridVector = Vector2D::new(3, 4);
        assert_eq!(p + v, GridPoint::new(4, 6));
    }
}
