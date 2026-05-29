use std::ops::{Index, IndexMut};
use crate::util::align::MemoryAlignment;

pub struct AlignedBoxedSlice<T> {
    storage: Box<[T]>,
    begin: usize,
    end: usize,
}

impl<T: Default + Clone> AlignedBoxedSlice<T> {
    pub fn new(size: usize, align: MemoryAlignment) -> AlignedBoxedSlice<T> {
        let extra = align.extra_elements::<T>();

        let storage = vec![Default::default(); size + extra].into_boxed_slice();
        
        let aligned_offset = align.unaligned_elements::<T>(storage.as_ptr() as usize);

        AlignedBoxedSlice {
            storage,
            begin: aligned_offset,
            end: aligned_offset + size,
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

    pub unsafe fn get_unchecked(&self, index: usize) -> &T {
        unsafe {
            self.storage.get_unchecked(index + self.begin)
        }
    }

    pub unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut T {
        unsafe {
            self.storage.get_unchecked_mut(index + self.begin)
        }
    }
}

impl<T> Index<usize> for AlignedBoxedSlice<T> {
    type Output = T;
    
    fn index(&self, index: usize) -> &Self::Output {
        &self.storage[index + self.begin]
    }
}

impl<T> IndexMut<usize> for AlignedBoxedSlice<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.storage[index + self.begin]
    }
}
