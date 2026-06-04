use crate::collections::aligned_boxed_slice::AlignedBoxedSlice;
use crate::util::align::MemoryAlignment;
use std::mem::MaybeUninit;
use std::ops::{Index, IndexMut, Range};

// Row-major 2-dimensional array.
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
        &self.data[y * self.width + x]
    }
}

impl<T> IndexMut<(usize, usize)> for Array2D<T> {
    #[inline(always)]
    fn index_mut(&mut self, (x, y): (usize, usize)) -> &mut Self::Output {
        &mut self.data[y * self.width + x]
    }
}

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
        &self.data[y * self.stride + x]
    }
}

impl<T> IndexMut<(usize, usize)> for MutSlice2D<'_, T> {
    #[inline(always)]
    fn index_mut(&mut self, (x, y): (usize, usize)) -> &mut Self::Output {
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
        &self.data.as_slice()[y * self.width..(y + 1) * self.width]
    }

    pub fn slice2d(&self, xr: Range<usize>, yr: Range<usize>) -> Slice2D<'_, T> {
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
        let new_start = yr.start * self.stride + xr.start;
        // We don't need to form an end, and it would be problematic anyway
        // because no matter what we do the slice spills outside the 2d box.

        Slice2D {
            data: &self.data[new_start..],
            stride: self.width,
            width: xr.len(),
            height: yr.len(),
            _marker: std::marker::PhantomData,
        }
    }
}

// TODO: how to avoid code duplication?
impl<T> MutSlice2D<'_, T> {
    pub fn mut_slice2d(&mut self, xr: Range<usize>, yr: Range<usize>) -> MutSlice2D<'_, T> {
        let new_start = yr.start * self.stride + xr.start;

        MutSlice2D {
            data: &mut self.data[new_start..],
            stride: self.width,
            width: xr.len(),
            height: yr.len(),
            _marker: std::marker::PhantomData,
        }
    }
}
