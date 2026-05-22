use std::collections::VecDeque;
use std::ops::{Index, IndexMut};

/// A contiguous array where the origin can be moved forward,
/// dropping all values below it.
/// The underlying array is assumed to be infinite in size, that is
/// elements that have not been assigned any value will return T::default().
/// In other words, any immutable index is valid.
/// However, trying to index mutably below the origin will cause a panic.
/// Note that this collection is not sparse - there are no holes between valid elements.
pub struct SlidingWindow<T> {
    origin: isize,
    chunks: VecDeque<T>,
    out_of_bounds_value: T,
}

impl<T> SlidingWindow<T> {
    pub fn memory_usage(&self) -> usize {
        self.chunks.len() * size_of::<T>()
    }
}

impl<T: Default> SlidingWindow<T> {
    pub fn with_origin(origin: isize) -> SlidingWindow<T> {
        SlidingWindow::<T> {
            origin,
            chunks: VecDeque::new(),
            out_of_bounds_value: T::default(),
        }
    }

    pub fn set_origin(&mut self, origin: isize) {
        if origin < self.origin {
            panic!("New origin must not be lower than the current origin.");
        }

        let num_elements_to_drop = (origin - self.origin) as usize;
        for _ in 0..num_elements_to_drop {
            self.chunks.pop_front();
        }

        self.origin = origin;
    }

    pub fn get_origin(&self) -> isize {
        self.origin
    }
}

impl<T: Default> Index<isize> for SlidingWindow<T> {
    type Output = T;

    fn index(&self, index: isize) -> &Self::Output {
        if index < self.origin {
            return &self.out_of_bounds_value;
        }

        let actual_idx = (index - self.origin) as usize;
        if actual_idx >= self.chunks.len() {
            return &self.out_of_bounds_value;
        }

        &self.chunks[actual_idx]
    }
}

impl<T: Default + Clone> IndexMut<isize> for SlidingWindow<T> {
    fn index_mut(&mut self, index: isize) -> &mut Self::Output {
        if index < self.origin {
            panic!("Index is before the origin.");
        }

        let actual_idx = (index - self.origin) as usize;
        while self.chunks.len() <= actual_idx {
            self.chunks.push_back(T::default());
        }

        &mut self.chunks[actual_idx]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::AssertUnwindSafe;

    fn make_window() -> SlidingWindow<i32> {
        SlidingWindow::with_origin(0) // chunk size = 4
    }

    #[test]
    fn read_uninitialized_returns_default() {
        let w = make_window();

        assert_eq!(w[0], 0);
        assert_eq!(w[10], 0);
        assert_eq!(w[100], 0);
    }

    #[test]
    fn writes_persist_to_reads() {
        let mut w = make_window();

        w[1] = 10;
        w[2] = 20;
        w[3] = 30;

        assert_eq!(w[1], 10);
        assert_eq!(w[2], 20);
        assert_eq!(w[3], 30);
    }

    #[test]
    fn holes_read_as_zero() {
        let mut w = make_window();

        w[10] = 99;

        assert_eq!(w[5], 0);
        assert_eq!(w[10], 99);
    }

    #[test]
    fn origin_allows_reads_below_as_default() {
        let mut w = SlidingWindow::with_origin(10);

        w[12] = 5;

        assert_eq!(w[9], 0);
        assert_eq!(w[0], 0);
        assert_eq!(w[-9999], 0);
        assert_eq!(w[11], 0);
        assert_eq!(w[12], 5);
    }

    #[test]
    fn write_before_origin_panics() {
        let mut w = SlidingWindow::with_origin(10);

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            w[9] = 1;
        }));

        assert!(result.is_err());
    }

    #[test]
    fn origin_boundary_is_allowed() {
        let mut w = SlidingWindow::with_origin(10);

        w[10] = 42;

        assert_eq!(w[10], 42);
    }

    #[test]
    fn chunk_indexing_is_consistent() {
        let mut w = make_window();

        for i in 0..32 {
            w[i] = i as i32;
        }

        for i in 0..32 {
            assert_eq!(w[i], i as i32);
        }
    }

    #[test]
    fn moving_origin_backwards_panics() {
        let mut w = make_window();

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            w.set_origin(-999);
        }));

        assert!(result.is_err());
    }

    #[test]
    fn can_move_origin_of_empty_window() {
        let mut w = make_window();
        w.set_origin(999999);
    }
}
