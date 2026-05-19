use std::ops::*;

pub struct Point2D<T> {
    pub x: T,
    pub y: T,
}

pub struct Vector2D<T> {
    pub x: T,
    pub y: T,
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
