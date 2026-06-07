use crate::collections::aligned_boxed_slice::AlignedBoxedSlice;
use crate::util::align::MemoryAlignment;
use std::mem::MaybeUninit;
use std::ops::{Index, IndexMut, Range};

// Row-major 2-dimensional array.
#[derive(Debug)]
pub struct Array2D<T> {
    data: AlignedBoxedSlice<T>,
    width: usize,
    height: usize,
}

impl<T> Clone for Array2D<T>
where
    T: Default + Copy,
{
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            width: self.width,
            height: self.height,
        }
    }
}

impl<T> PartialEq for Array2D<T>
where
    T: PartialEq,
{
    fn eq(&self, other: &Array2D<T>) -> bool {
        if self.width != other.width || self.height != other.height {
            return false;
        }

        for y in 0..self.height {
            for x in 0..self.width {
                if self[(x, y)] != other[(x, y)] {
                    return false;
                }
            }
        }

        true
    }
}

impl<T: Default + Clone> Array2D<T> {
    pub fn new(width: usize, height: usize) -> Self {
        Array2D::<T> {
            data: AlignedBoxedSlice::<T>::new(
                width * height,
                MemoryAlignment::new(align_of::<T>()),
            ),
            width,
            height,
        }
    }

    pub fn new_aligned(width: usize, height: usize, align: MemoryAlignment) -> Self {
        Array2D::<T> {
            data: AlignedBoxedSlice::<T>::new(width * height, align),
            width,
            height,
        }
    }
}

impl<T: Default> Array2D<MaybeUninit<T>> {
    pub fn new_uninit_aligned(width: usize, height: usize, align: MemoryAlignment) -> Self {
        Array2D {
            data: AlignedBoxedSlice::<MaybeUninit<T>>::new_uninit(width * height, align),
            width,
            height,
        }
    }

    /// # SAFETY
    ///
    /// As with [`MaybeUninit::assume_init`],
    /// it is up to the caller to guarantee that the values
    /// really are in an initialized state.
    /// Calling this when the content is not yet fully initialized
    /// causes immediate undefined behavior.
    pub unsafe fn assume_init(self) -> Array2D<T> {
        // SAFETY: We have to assume the caller filled every element they have access too.
        //         We filled the padding above.
        let data = unsafe { self.data.assume_init() };
        Array2D {
            data,
            width: self.width,
            height: self.height,
        }
    }
}

impl<T> Index<(usize, usize)> for Array2D<T> {
    type Output = T;

    #[inline(always)]
    fn index(&self, (x, y): (usize, usize)) -> &Self::Output {
        assert!(x < self.width && y < self.height);
        &self.data[y * self.width + x]
    }
}

impl<T> IndexMut<(usize, usize)> for Array2D<T> {
    #[inline(always)]
    fn index_mut(&mut self, (x, y): (usize, usize)) -> &mut Self::Output {
        assert!(x < self.width && y < self.height);
        &mut self.data[y * self.width + x]
    }
}

#[derive(Debug)]
pub struct Slice2DInternal<'a, T, P> {
    data: P,
    width: usize,
    height: usize,
    stride: usize,
    _marker: std::marker::PhantomData<&'a T>,
}

impl<'a, T, P> Slice2DInternal<'a, T, P> {
    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }
}

pub type Slice2D<'a, T> = Slice2DInternal<'a, T, &'a [T]>;
pub type MutSlice2D<'a, T> = Slice2DInternal<'a, T, &'a mut [T]>;

impl<T> Index<(usize, usize)> for Slice2D<'_, T> {
    type Output = T;

    #[inline(always)]
    fn index(&self, (x, y): (usize, usize)) -> &Self::Output {
        assert!(x < self.width && y < self.height);
        &self.data[y * self.stride + x]
    }
}

impl<T> Slice2D<'_, T> {
    /// # Safety
    ///
    /// Calling this method with an out-of-bounds index is *[undefined behavior]*
    /// even if the resulting reference is not used.
    ///
    /// [undefined behavior]: https://doc.rust-lang.org/reference/behavior-considered-undefined.html
    #[inline(always)]
    pub unsafe fn get_unchecked(&self, x: usize, y: usize) -> &T {
        unsafe { self.data.get_unchecked(y * self.stride + x) }
    }
}

impl<T> Index<(usize, usize)> for MutSlice2D<'_, T> {
    type Output = T;

    #[inline(always)]
    fn index(&self, (x, y): (usize, usize)) -> &Self::Output {
        assert!(x < self.width && y < self.height);
        &self.data[y * self.stride + x]
    }
}

impl<T> IndexMut<(usize, usize)> for MutSlice2D<'_, T> {
    #[inline(always)]
    fn index_mut(&mut self, (x, y): (usize, usize)) -> &mut Self::Output {
        assert!(x < self.width && y < self.height);
        &mut self.data[y * self.stride + x]
    }
}

impl<T> MutSlice2D<'_, T> {
    /// # Safety
    ///
    /// Calling this method with an out-of-bounds index is *[undefined behavior]*
    /// even if the resulting reference is not used.
    ///
    /// [undefined behavior]: https://doc.rust-lang.org/reference/behavior-considered-undefined.html
    #[inline(always)]
    pub unsafe fn get_unchecked(&self, x: usize, y: usize) -> &T {
        unsafe { self.data.get_unchecked(y * self.stride + x) }
    }

    /// # Safety
    ///
    /// Calling this method with an out-of-bounds index is *[undefined behavior]*
    /// even if the resulting reference is not used.
    ///
    /// [undefined behavior]: https://doc.rust-lang.org/reference/behavior-considered-undefined.html
    #[inline(always)]
    pub unsafe fn get_unchecked_mut(&mut self, x: usize, y: usize) -> &mut T {
        unsafe { self.data.get_unchecked_mut(y * self.stride + x) }
    }
}

impl<T> Array2D<T> {
    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// # Safety
    ///
    /// Calling this method with an out-of-bounds index is *[undefined behavior]*
    /// even if the resulting reference is not used.
    ///
    /// [undefined behavior]: https://doc.rust-lang.org/reference/behavior-considered-undefined.html
    #[inline(always)]
    pub unsafe fn get_unchecked(&self, x: usize, y: usize) -> &T {
        unsafe { self.data.get_unchecked(y * self.width + x) }
    }

    /// # Safety
    ///
    /// Calling this method with an out-of-bounds index is *[undefined behavior]*
    /// even if the resulting reference is not used.
    ///
    /// [undefined behavior]: https://doc.rust-lang.org/reference/behavior-considered-undefined.html
    #[inline(always)]
    pub unsafe fn get_unchecked_mut(&mut self, x: usize, y: usize) -> &mut T {
        unsafe { self.data.get_unchecked_mut(y * self.width + x) }
    }
}

impl<T> Array2D<T> {
    pub fn row_slice(&self, y: usize) -> &'_ [T] {
        assert!(y < self.height);
        &self.data.as_slice()[y * self.width..(y + 1) * self.width]
    }

    pub fn slice2d(&self, xr: Range<usize>, yr: Range<usize>) -> Slice2D<'_, T> {
        assert!(xr.end <= self.width);
        assert!(yr.end <= self.height);

        let start = yr.start * self.width + xr.start;
        // We don't need to form an end, and it would be problematic anyway
        // because no matter what we do the slice spills outside the 2d box.

        Slice2D {
            data: &self.data.as_slice()[start..],
            stride: self.width,
            width: xr.len(),
            height: yr.len(),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn mut_slice2d(&mut self, xr: Range<usize>, yr: Range<usize>) -> MutSlice2D<'_, T> {
        assert!(xr.end <= self.width);
        assert!(yr.end <= self.height);

        let start = yr.start * self.width + xr.start;
        // We don't need to form an end, and it would be problematic anyway
        // because no matter what we do the slice spills outside the 2d box.

        MutSlice2D {
            data: &mut self.data.as_mut_slice()[start..],
            stride: self.width,
            width: xr.len(),
            height: yr.len(),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn as_slice2d(&self) -> Slice2D<'_, T> {
        Slice2D {
            data: self.data.as_slice(),
            stride: self.width,
            width: self.width,
            height: self.height,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn as_mut_slice2d(&mut self) -> MutSlice2D<'_, T> {
        MutSlice2D {
            data: self.data.as_mut_slice(),
            stride: self.width,
            width: self.width,
            height: self.height,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn as_flat_slice(&self) -> &'_ [T] {
        self.data.as_slice()
    }

    pub fn as_flat_mut_slice(&mut self) -> &'_ mut [T] {
        self.data.as_mut_slice()
    }
}

impl<'a, T> Array2D<T> {
    pub fn as_positioned_chunks_mut(
        &'a mut self,
        chunk_width: usize,
        chunk_height: usize,
    ) -> Vec<(usize, usize, MutSlice2D<'a, T>)> {
        assert!(chunk_width > 0 && chunk_height > 0);

        let chunks_x = self.width.div_ceil(chunk_width);
        let chunks_y = self.height.div_ceil(chunk_height);

        let mut result = Vec::with_capacity(chunks_x * chunks_y);

        // We need raw pointer manipulation to hand out multiple non-overlapping
        // mutable subslices from a single &mut [T], since the borrow checker
        // can't reason about 2D strided non-overlap statically.
        //
        // Safety invariant: each MutSlice2D we produce covers a distinct set of
        // elements - verified by construction (different row ranges and column ranges).
        let ptr = self.data.as_mut_slice().as_mut_ptr();
        let total_len = self.data.as_mut_slice().len();

        for cy in 0..chunks_y {
            for cx in 0..chunks_x {
                let x0 = cx * chunk_width;
                let y0 = cy * chunk_height;

                let x1 = (x0 + chunk_width).min(self.width);
                let y1 = (y0 + chunk_height).min(self.height);

                let linearized_index = y0 * self.width + x0;

                // SAFETY:
                // - No two iterations produce overlapping index ranges, because each iteration
                //   offsets the start position by at least as much as the size of a chunk.
                //
                // NOTE:
                // We do return a longer slice than needed, but that's not a concern, because
                // the access is abstracted by 2-dimensional indices and size.
                let slice = unsafe {
                    std::slice::from_raw_parts_mut(
                        ptr.add(linearized_index),
                        total_len - linearized_index,
                    )
                };

                result.push((
                    x0,
                    y0,
                    MutSlice2D {
                        data: slice,
                        stride: self.width,
                        width: x1 - x0,
                        height: y1 - y0,
                        _marker: std::marker::PhantomData,
                    },
                ));
            }
        }

        result
    }
}

// TODO: how to avoid code duplication?
impl<T> Slice2D<'_, T> {
    pub fn slice2d(&self, xr: Range<usize>, yr: Range<usize>) -> Slice2D<'_, T> {
        assert!(xr.end <= self.width);
        assert!(yr.end <= self.height);

        let new_start = yr.start * self.stride + xr.start;
        // We don't need to form an end, and it would be problematic anyway
        // because no matter what we do the slice spills outside the 2d box.

        Slice2D {
            data: &self.data[new_start..],
            stride: self.stride,
            width: xr.len(),
            height: yr.len(),
            _marker: std::marker::PhantomData,
        }
    }
}

// TODO: how to avoid code duplication?
impl<T> MutSlice2D<'_, T> {
    pub fn mut_slice2d(&mut self, xr: Range<usize>, yr: Range<usize>) -> MutSlice2D<'_, T> {
        assert!(xr.end <= self.width);
        assert!(yr.end <= self.height);

        let new_start = yr.start * self.stride + xr.start;

        MutSlice2D {
            data: &mut self.data[new_start..],
            stride: self.stride,
            width: xr.len(),
            height: yr.len(),
            _marker: std::marker::PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::AssertUnwindSafe;

    #[derive(Clone, Copy, Eq, PartialEq, Debug, Ord, PartialOrd)]
    struct NonzeroDefault(u32);
    impl Default for NonzeroDefault {
        fn default() -> Self {
            NonzeroDefault(123456)
        }
    }

    /// Build a width×height Array2D<u32> where cell (x, y) = y * width + x.
    fn make_grid(width: usize, height: usize) -> Array2D<u32> {
        let mut a = Array2D::<u32>::new(width, height);
        for y in 0..height {
            for x in 0..width {
                a[(x, y)] = (y * width + x) as u32;
            }
        }
        a
    }

    #[test]
    fn new_reports_correct_dimensions() {
        let a = Array2D::<u32>::new(7, 3);
        assert_eq!(a.width(), 7);
        assert_eq!(a.height(), 3);
    }

    #[test]
    fn new_default_initialises_elements() {
        let a = Array2D::<NonzeroDefault>::new(4, 4);
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(a[(x, y)], NonzeroDefault::default(), "expected default at ({x},{y})");
            }
        }
    }

    #[test]
    fn new_1x1_works() {
        let mut a = Array2D::<i32>::new(1, 1);
        a[(0, 0)] = 42;
        assert_eq!(a[(0, 0)], 42);
    }

    #[test]
    fn is_row_major_layout_via_flat_slice() {
        let a = make_grid(3, 2); // [[0,1,2],[3,4,5]]
        assert_eq!(a.as_flat_slice(), &[0u32, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn index_mut_modifies_correct_cell() {
        let mut a = Array2D::<u8>::new(4, 4);
        a[(2, 3)] = 99;
        assert_eq!(a[(2, 3)], 99);
        // Neighbors untouched
        assert_eq!(a[(3, 2)], 0);
        assert_eq!(a[(3, 3)], 0);
        assert_eq!(a[(2, 2)], 0);
    }

    #[test]
    fn array2d_out_of_bounds_index_panics() {
        let a = make_grid(4, 4);

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = a[(3, 4)];
        }));

        assert!(result.is_err());

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = a[(4, 4)];
        }));

        assert!(result.is_err());

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = a[(4, 3)];
        }));

        assert!(result.is_err());
    }

    #[test]
    fn slice2d_out_of_bounds_index_panics() {
        let a = make_grid(4, 4);
        let a = a.as_slice2d();

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = a[(3, 4)];
        }));

        assert!(result.is_err());

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = a[(4, 4)];
        }));

        assert!(result.is_err());

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = a[(4, 3)];
        }));

        assert!(result.is_err());
    }

    #[test]
    fn mut_slice2d_out_of_bounds_index_panics() {
        let mut a = make_grid(4, 4);
        let a = a.as_mut_slice2d();

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = a[(3, 4)];
        }));

        assert!(result.is_err());

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = a[(4, 4)];
        }));

        assert!(result.is_err());

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = a[(4, 3)];
        }));

        assert!(result.is_err());
    }

    #[test]
    fn equal_arrays_compare_equal() {
        let a = make_grid(4, 3);
        let b = make_grid(4, 3);
        assert_eq!(a, b);
    }

    #[test]
    fn arrays_with_different_values_are_not_equal() {
        let a = make_grid(4, 3);
        let mut b = make_grid(4, 3);
        b[(1, 1)] = 999;
        assert_ne!(a, b);
    }

    #[test]
    fn arrays_with_different_widths_are_not_equal() {
        let a = Array2D::<u32>::new(4, 3);
        let b = Array2D::<u32>::new(3, 3);
        assert_ne!(a, b);
    }

    #[test]
    fn arrays_with_different_heights_are_not_equal() {
        let a = Array2D::<u32>::new(4, 3);
        let b = Array2D::<u32>::new(4, 4);
        assert_ne!(a, b);
    }

    #[test]
    fn clone_is_deep_copy() {
        let a = make_grid(3, 3);
        let mut b = a.clone();
        b[(0, 0)] = 255;
        // Original must not be affected
        assert_eq!(a[(0, 0)], 0);
    }

    #[test]
    fn row_slice_returns_correct_row() {
        let a = make_grid(4, 3);
        assert_eq!(a.row_slice(0), &[0u32, 1, 2, 3]);
        assert_eq!(a.row_slice(1), &[4u32, 5, 6, 7]);
        assert_eq!(a.row_slice(2), &[8u32, 9, 10, 11]);
    }

    #[test]
    fn creating_row_slice_out_of_range_panics() {
        let a = make_grid(4, 4);

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = a.row_slice(4);
        }));

        assert!(result.is_err());
    }

    #[test]
    fn creating_slice2d_out_of_range_panics() {
        let a = make_grid(4, 4);

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = a.slice2d(1..5, 0..2);
        }));

        assert!(result.is_err());
    }

    #[test]
    fn creating_mut_slice2d_out_of_range_panics() {
        let mut a = make_grid(4, 4);

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = a.mut_slice2d(1..5, 0..2);
        }));

        assert!(result.is_err());
    }

    #[test]
    fn mut_slice2d_creating_subslice_out_of_range_panics() {
        let a = make_grid(4, 4);

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = a.as_slice2d().slice2d(1..5, 0..2);
        }));

        assert!(result.is_err());
    }

    #[test]
    fn slice2d_creating_subslice_out_of_range_panics() {
        let mut a = make_grid(4, 4);

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = a.as_mut_slice2d().mut_slice2d(1..5, 0..2);
        }));

        assert!(result.is_err());
    }

    #[test]
    fn slice2d_dimensions_are_correct() {
        let a = make_grid(8, 6);
        let s = a.slice2d(2..5, 1..4);
        assert_eq!(s.width(), 3);
        assert_eq!(s.height(), 3);
    }

    #[test]
    fn slice2d_values_match_parent() {
        let a = make_grid(8, 6);
        // a[(x,y)] = y*8 + x
        let s = a.slice2d(2..5, 1..4);
        for dy in 0..3usize {
            for dx in 0..3usize {
                let expected = ((1 + dy) * 8 + (2 + dx)) as u32;
                assert_eq!(s[(dx, dy)], expected, "mismatch at slice ({dx},{dy})");
            }
        }
    }

    #[test]
    fn slice2d_of_slice2d_values_match_parent() {
        let a = make_grid(8, 8);
        let outer = a.slice2d(2..8, 2..8); // 6×6 window
        let inner = outer.slice2d(1..3, 1..3); // 2×2 sub-window, absolute (3..5, 3..5)
        assert_eq!(inner.width(), 2);
        assert_eq!(inner.height(), 2);
        for dy in 0..2usize {
            for dx in 0..2usize {
                let expected = ((3 + dy) * 8 + (3 + dx)) as u32;
                assert_eq!(inner[(dx, dy)], expected);
            }
        }
    }

    #[test]
    fn mut_slice2d_allows_writes_visible_in_parent() {
        let mut a = Array2D::<u32>::new(6, 6);
        {
            let mut s = a.mut_slice2d(1..4, 1..4);
            for dy in 0..3 {
                for dx in 0..3 {
                    s[(dx, dy)] = (dy * 3 + dx) as u32 + 1;
                }
            }
        }
        // Verify through parent
        for dy in 0..3usize {
            for dx in 0..3usize {
                assert_eq!(a[(1 + dx, 1 + dy)], (dy * 3 + dx) as u32 + 1);
            }
        }
        // Cells outside the slice must remain 0
        assert_eq!(a[(0, 0)], 0);
        assert_eq!(a[(5, 5)], 0);
    }

    #[test]
    fn as_slice2d_covers_full_array() {
        let a = make_grid(3, 2);
        let s = a.as_slice2d();
        assert_eq!(s.width(), 3);
        assert_eq!(s.height(), 2);
        for y in 0..2usize {
            for x in 0..3usize {
                assert_eq!(s[(x, y)], a[(x, y)]);
            }
        }
    }

    #[test]
    fn get_unchecked_matches_indexed_access() {
        let a = make_grid(5, 5);
        for y in 0..5 {
            for x in 0..5 {
                let via_index = a[(x, y)];
                let via_unchecked = unsafe { *a.get_unchecked(x, y) };
                assert_eq!(via_index, via_unchecked);
            }
        }
    }

    #[test]
    fn get_unchecked_mut_writes_correct_cell() {
        let mut a = Array2D::<u32>::new(5, 5);
        unsafe { *a.get_unchecked_mut(3, 2) = 77 };
        assert_eq!(a[(3, 2)], 77);
        assert_eq!(a[(2, 3)], 0);
        assert_eq!(a[(2, 2)], 0);
        assert_eq!(a[(3, 3)], 0);
    }

    /// Chunks should tile the entire array exactly once (no gaps, no overlaps),
    /// verified by adding 1 to every cell.
    #[test]
    fn chunks_mut_tile_array_without_gaps_or_overlaps() {
        let w = 7usize;
        let h = 5usize;
        let mut a = Array2D::<u32>::new(w, h);

        {
            let chunks = a.as_positioned_chunks_mut(3, 2);
            for (_x0, _y0, mut chunk) in chunks {
                for dy in 0..chunk.height() {
                    for dx in 0..chunk.width() {
                        // Encode absolute position as a unique value
                        chunk[(dx, dy)] += 1;
                    }
                }
            }
        }

        // Every cell must have been written exactly once
        for y in 0..h {
            for x in 0..w {
                assert_eq!(
                    a[(x, y)],
                    1,
                    "cell ({x},{y}) was not written exactly once"
                );
            }
        }
    }

    #[test]
    fn chunks_mut_correct_count_for_non_divisible_dimensions() {
        let mut a = Array2D::<u32>::new(7, 5);
        let chunks = a.as_positioned_chunks_mut(3, 2);
        // ceil(7/3)=3, ceil(5/2)=3 → 9 chunks
        assert_eq!(chunks.len(), 9);
    }

    #[test]
    fn chunks_mut_correct_count_for_exact_dimensions() {
        let mut a = Array2D::<u32>::new(6, 4);
        let chunks = a.as_positioned_chunks_mut(3, 2);
        // 2 × 2 = 4 chunks
        assert_eq!(chunks.len(), 4);
    }

    #[test]
    fn chunks_mut_reported_origins_are_correct() {
        let mut a = Array2D::<u32>::new(6, 4);
        let chunks = a.as_positioned_chunks_mut(3, 2);
        let origins: Vec<(usize, usize)> = chunks.iter().map(|(x, y, _)| (*x, *y)).collect();
        assert_eq!(
            origins,
            vec![(0, 0), (3, 0), (0, 2), (3, 2)]
        );
    }

    #[test]
    fn chunks_mut_boundary_chunks_have_clipped_dimensions() {
        let mut a = Array2D::<u32>::new(7, 5);
        let chunks = a.as_positioned_chunks_mut(3, 2);

        // Chunk at (6, 4) — bottom-right corner — should be 1×1
        let corner = chunks.iter().find(|(x, y, _)| *x == 6 && *y == 4);
        assert!(corner.is_some(), "expected a chunk at origin (6,4)");
        let (_, _, s) = corner.unwrap();
        assert_eq!(s.width(), 1);
        assert_eq!(s.height(), 1);
    }
}