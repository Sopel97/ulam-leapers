use std::ops::{Index, IndexMut, Range};
use crate::collections::aligned_boxed_slice::AlignedBoxedSlice;
use crate::util::align::MemoryAlignment;

// Row-major 2 dimensional array.
pub struct Array2D<T> {
    data: AlignedBoxedSlice<T>,
    width: usize,
    height: usize,
}

impl<T: Default + Clone> Array2D<T> {
    pub fn new_aligned(width: usize, height: usize, align: MemoryAlignment) -> Self {
        Array2D::<T> {
            data: AlignedBoxedSlice::<T>::new(width * height, align),
            width,
            height,
        }
    }
}

impl<T> Index<(usize, usize)> for Array2D<T> {
    type Output = T;

    fn index(&self, (x, y): (usize, usize)) -> &Self::Output {
        &self.data[y * self.width + x]
    }
}

impl<T> IndexMut<(usize, usize)> for Array2D<T> {
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

pub type Slice2D<'a, T> = Slice2DInternal<'a, T, &'a [T]>;
pub type MutSlice2D<'a, T> = Slice2DInternal<'a, T, &'a mut [T]>;

impl<'a, T> Index<(usize, usize)> for Slice2D<'a, T> {
    type Output = T;

    fn index(&self, (x, y): (usize, usize)) -> &Self::Output {
        &self.data[y * self.stride + x]
    }
}

impl<'a, T> Index<(usize, usize)> for MutSlice2D<'a, T> {
    type Output = T;

    fn index(&self, (x, y): (usize, usize)) -> &Self::Output {
        &self.data[y * self.stride + x]
    }
}

impl<'a, T> IndexMut<(usize, usize)> for MutSlice2D<'a, T> {
    fn index_mut(&mut self, (x, y): (usize, usize)) -> &mut Self::Output {
        &mut self.data[y * self.stride + x]
    }
}

impl<T> Array2D<T> {
    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }
}

impl<'a, T> Array2D<T> {
    pub fn slice2d(&'a self, xr: Range<usize>, yr: Range<usize>) -> Slice2D<'a, T> {
        let width = xr.len();
        let height = yr.len();
        let size = width * height;

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

    pub fn mut_slice2d(&'a mut self, xr: Range<usize>, yr: Range<usize>) -> MutSlice2D<'a, T> {
        let width = xr.len();
        let height = yr.len();
        let size = width * height;

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
}