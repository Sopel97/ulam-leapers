use std::ops::{Index, IndexMut};

struct AlignedBoxedSlice<T> {
    storage: Box<[T]>,
    begin: usize,
    end: usize,
}

impl<T: Default + Clone> AlignedBoxedSlice<T> {
    pub fn new(size: usize, align: usize) -> AlignedBoxedSlice<T> {
        let elem_size = size_of::<T>();
        let extra = (align + elem_size - 1) / elem_size; // ceil_div

        let storage = vec![Default::default(); size + extra].into_boxed_slice();
        
        let ptr = storage.as_ptr() as usize;
        let aligned_ptr = (ptr + 63) & !63usize;
        let aligned_offset = (aligned_ptr - ptr) / elem_size;

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
