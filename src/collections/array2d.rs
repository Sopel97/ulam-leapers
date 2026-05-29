use crate::collections::aligned_boxed_slice::AlignedBoxedSlice;
use crate::util::align::MemoryAlignment;
use std::ops::{Index, IndexMut, Range};

// Row-major 2-dimensional array.
pub struct Array2D<T> {
    data: AlignedBoxedSlice<T>,
    width: usize,
    height: usize,
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

    fn index(&self, (x, y): (usize, usize)) -> &Self::Output {
        &self.data[y * self.stride + x]
    }
}

impl<T> Slice2D<'_, T> {
    pub unsafe fn get_unchecked(&self, x: usize, y: usize) -> &T {
        unsafe {
            self.data.get_unchecked(y * self.stride + x)
        }
    }
}

impl<T> Index<(usize, usize)> for MutSlice2D<'_, T> {
    type Output = T;

    fn index(&self, (x, y): (usize, usize)) -> &Self::Output {
        &self.data[y * self.stride + x]
    }
}

impl<T> IndexMut<(usize, usize)> for MutSlice2D<'_, T> {
    fn index_mut(&mut self, (x, y): (usize, usize)) -> &mut Self::Output {
        &mut self.data[y * self.stride + x]
    }
}


impl<T> MutSlice2D<'_, T> {
    pub unsafe fn get_unchecked(&self, x: usize, y: usize) -> &T {
        unsafe {
            self.data.get_unchecked(y * self.stride + x)
        }
    }

    pub unsafe fn get_unchecked_mut(&mut self, x: usize, y: usize) -> &mut T {
        unsafe {
            self.data.get_unchecked_mut(y * self.stride + x)
        }
    }
}

impl<T> Array2D<T> {
    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub unsafe fn get_unchecked(&self, x: usize, y: usize) -> &T {
        unsafe {
            self.data.get_unchecked(y * self.width + x)
        }
    }

    pub unsafe fn get_unchecked_mut(&mut self, x: usize, y: usize) -> &mut T {
        unsafe {
            self.data.get_unchecked_mut(y * self.width + x)
        }
    }
}

impl<'a, T> Array2D<T> {
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
