use crate::util::align::MemoryAlignment;
use std::mem::MaybeUninit;
use std::ops::{Index, IndexMut};

pub struct AlignedBoxedSlice<T> {
    storage: Box<[T]>,
    align: MemoryAlignment,
    begin: usize,
    end: usize,
}

impl<T> Clone for AlignedBoxedSlice<T>
where
    T: Default + Copy,
{
    fn clone(&self) -> Self {
        let mut other = Self::new(self.end - self.begin, self.align);
        other.as_mut_slice().copy_from_slice(self.as_slice());
        other
    }
}

impl<T: Default + Clone> AlignedBoxedSlice<T> {
    pub fn new(size: usize, align: MemoryAlignment) -> AlignedBoxedSlice<T> {
        let extra = align.extra_elements::<T>();

        let storage = vec![Default::default(); size + extra].into_boxed_slice();

        let aligned_offset = align.unaligned_elements::<T>(storage.as_ptr() as usize);

        AlignedBoxedSlice {
            storage,
            align,
            begin: aligned_offset,
            end: aligned_offset + size,
        }
    }
}

impl<T: Default> AlignedBoxedSlice<MaybeUninit<T>> {
    pub fn new_uninit(size: usize, align: MemoryAlignment) -> Self {
        let extra = align.extra_elements::<T>();

        let storage = Box::new_uninit_slice(size + extra);

        let aligned_offset = align.unaligned_elements::<T>(storage.as_ptr() as usize);

        AlignedBoxedSlice {
            storage,
            align,
            begin: aligned_offset,
            end: aligned_offset + size,
        }
    }

    /// # SAFETY
    ///
    /// As with [`MaybeUninit::assume_init`],
    /// it is up to the caller to guarantee that the values
    /// really are in an initialized state.
    /// Calling this when the content is not yet fully initialized
    /// causes immediate undefined behavior.
    pub unsafe fn assume_init(mut self) -> AlignedBoxedSlice<T> {
        // We have to remember to initialize the elements the user has no access to.
        for x in &mut self.storage[..self.begin] {
            x.write(T::default());
        }

        for x in &mut self.storage[self.end..] {
            x.write(T::default());
        }

        // SAFETY: We have to assume the caller filled every element they have access too.
        //         We filled the padding above.
        let storage = unsafe { self.storage.assume_init() };
        AlignedBoxedSlice {
            storage,
            align: self.align,
            begin: self.begin,
            end: self.end,
        }
    }
}

impl<T> AlignedBoxedSlice<T> {
    pub fn as_slice(&self) -> &[T] {
        &self.storage[self.begin..self.end]
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.storage[self.begin..self.end]
    }

    /// # Safety
    ///
    /// Calling this method with an out-of-bounds index is *[undefined behavior]*
    /// even if the resulting reference is not used.
    ///
    /// [undefined behavior]: https://doc.rust-lang.org/reference/behavior-considered-undefined.html
    #[inline(always)]
    pub unsafe fn get_unchecked(&self, index: usize) -> &T {
        unsafe { self.storage.get_unchecked(index + self.begin) }
    }

    /// # Safety
    ///
    /// Calling this method with an out-of-bounds index is *[undefined behavior]*
    /// even if the resulting reference is not used.
    ///
    /// [undefined behavior]: https://doc.rust-lang.org/reference/behavior-considered-undefined.html
    #[inline(always)]
    pub unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut T {
        unsafe { self.storage.get_unchecked_mut(index + self.begin) }
    }
}

impl<T> Index<usize> for AlignedBoxedSlice<T> {
    type Output = T;

    #[inline(always)]
    fn index(&self, index: usize) -> &Self::Output {
        &self.storage[index + self.begin]
    }
}

impl<T> IndexMut<usize> for AlignedBoxedSlice<T> {
    #[inline(always)]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.storage[index + self.begin]
    }
}
