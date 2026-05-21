use crate::collections::aligned_boxed_slice::AlignedBoxedSlice;
use crate::util::align::CACHE_LINE_SIZE;
use crate::util::pow2;
use crate::util::pow2::Pow2;
use std::cmp::min;
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
    chunk_size: Pow2,
    chunk_size_mod_mask: isize, // Using a precomputed mask instead of pow2::floor_mod is measurably faster.
    origin: isize,
    origin_chunk: isize,
    chunks: VecDeque<AlignedBoxedSlice<T>>,
    out_of_bounds_value: T,
}

impl<T> SlidingWindow<T> {
    pub fn memory_usage(&self) -> usize {
        let chunk_size: usize = self.chunk_size.into();
        chunk_size * self.chunks.len() * size_of::<T>()
    }
}

impl<T: Default> SlidingWindow<T> {
    pub fn with_chunk_size_and_origin(chunk_size: Pow2, origin: isize) -> SlidingWindow<T> {
        SlidingWindow::<T> {
            chunk_size,
            chunk_size_mod_mask: chunk_size.floor_mod_mask() as isize,
            origin,
            origin_chunk: pow2::floor_div(origin, chunk_size),
            chunks: VecDeque::new(),
            out_of_bounds_value: T::default(),
        }
    }

    pub fn with_chunk_size(chunk_size: Pow2) -> SlidingWindow<T> {
        Self::with_chunk_size_and_origin(chunk_size, 0)
    }

    pub fn set_origin(&mut self, origin: isize) {
        if origin < self.origin {
            panic!("New origin must not be lower than the current origin.");
        }

        let new_origin_chunk = pow2::floor_div(origin, self.chunk_size);
        let num_chunks_to_drop = min(
            (new_origin_chunk - self.origin_chunk) as usize,
            self.chunks.len(),
        );
        for _ in 0..num_chunks_to_drop {
            self.chunks.pop_front();
        }

        self.origin = origin;
        self.origin_chunk = new_origin_chunk;
    }

    pub fn get_active_chunk_count(&self) -> usize {
        self.chunks.len()
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

        let chunk_index = (pow2::floor_div(index, self.chunk_size) - self.origin_chunk) as usize;
        if chunk_index >= self.chunks.len() {
            return &self.out_of_bounds_value;
        }

        let index_within_chunk = (index & self.chunk_size_mod_mask) as usize;
        &self.chunks[chunk_index][index_within_chunk]
    }
}

impl<T: Default + Clone> IndexMut<isize> for SlidingWindow<T> {
    fn index_mut(&mut self, index: isize) -> &mut Self::Output {
        if index < self.origin {
            panic!("Index is before the origin.");
        }

        let chunk_index = (pow2::floor_div(index, self.chunk_size) - self.origin_chunk) as usize;
        if chunk_index >= self.chunks.len() {
            self.chunks.resize_with(chunk_index + 1, || {
                AlignedBoxedSlice::<T>::new(self.chunk_size.into(), CACHE_LINE_SIZE)
            });
        }

        let index_within_chunk = (index & self.chunk_size_mod_mask) as usize;
        &mut self.chunks[chunk_index][index_within_chunk]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::AssertUnwindSafe;

    fn make_window() -> SlidingWindow<i32> {
        SlidingWindow::with_chunk_size_and_origin(Pow2::new(4), 0) // chunk size = 4
    }

    #[test]
    fn empty_window_has_no_active_chunks() {
        let w = make_window();
        assert_eq!(w.get_active_chunk_count(), 0);
    }

    #[test]
    fn read_uninitialized_returns_default() {
        let w = make_window();

        assert_eq!(w[0], 0);
        assert_eq!(w[10], 0);
        assert_eq!(w[100], 0);
    }

    #[test]
    fn write_and_read_within_same_chunk() {
        let mut w = SlidingWindow::with_chunk_size_and_origin(Pow2::new(4), 0);

        w[1] = 10;
        w[2] = 20;
        w[3] = 30;

        assert_eq!(w[1], 10);
        assert_eq!(w[2], 20);
        assert_eq!(w[3], 30);

        assert_eq!(w.get_active_chunk_count(), 1);
    }

    #[test]
    fn write_and_read_across_chunk_boundary() {
        let mut w = SlidingWindow::with_chunk_size_and_origin(Pow2::new(4), 0); // chunk size = 4

        w[3] = 1;
        w[4] = 2; // next chunk
        w[5] = 3;

        assert_eq!(w[3], 1);
        assert_eq!(w[4], 2);
        assert_eq!(w[5], 3);

        assert_eq!(w.get_active_chunk_count(), 2);
    }

    #[test]
    fn chunks_are_created_without_holes() {
        let mut w = SlidingWindow::with_chunk_size_and_origin(Pow2::new(4), 0);

        assert_eq!(w.get_active_chunk_count(), 0);

        w[10] = 99;

        assert_eq!(w.get_active_chunk_count(), 3);
        assert_eq!(w[10], 99);
    }

    #[test]
    fn multiple_chunks_are_allocated_correctly() {
        let mut w = SlidingWindow::with_chunk_size_and_origin(Pow2::new(4), 0);

        w[0] = 1;
        w[4] = 2;
        w[8] = 3;
        w[12] = 4;

        assert_eq!(w[0], 1);
        assert_eq!(w[4], 2);
        assert_eq!(w[8], 3);
        assert_eq!(w[12], 4);

        assert_eq!(w.chunks.len(), 4);
    }

    #[test]
    fn origin_allows_reads_below_as_default() {
        let mut w = SlidingWindow::with_chunk_size_and_origin(Pow2::new(4), 10);

        w[12] = 5;

        assert_eq!(w[9], 0);
        assert_eq!(w[0], 0);
        assert_eq!(w[-9999], 0);
        assert_eq!(w[11], 0);
        assert_eq!(w[12], 5);
    }

    #[test]
    fn write_before_origin_panics() {
        let mut w = SlidingWindow::with_chunk_size_and_origin(Pow2::new(4), 10);

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            w[9] = 1;
        }));

        assert!(result.is_err());
    }

    #[test]
    fn origin_boundary_is_allowed() {
        let mut w = SlidingWindow::with_chunk_size_and_origin(Pow2::new(4), 10);

        w[10] = 42;

        assert_eq!(w[10], 42);
    }

    #[test]
    fn chunk_indexing_is_consistent() {
        let mut w = SlidingWindow::with_chunk_size_and_origin(Pow2::new(8), 0); // chunk size = 8

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

    #[test]
    fn moving_the_origin_deactivates_chunks() {
        let mut w = SlidingWindow::with_chunk_size_and_origin(Pow2::new(4), 0);

        assert_eq!(w.get_active_chunk_count(), 0);

        w[17] = 1;

        assert_eq!(w.get_active_chunk_count(), 5);

        w.set_origin(11);

        assert_eq!(w.get_active_chunk_count(), 3);
    }
}
