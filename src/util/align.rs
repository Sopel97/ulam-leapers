#[derive(Debug, PartialEq, PartialOrd, Eq, Ord, Clone, Copy)]
pub struct MemoryAlignment(usize);

impl MemoryAlignment {
    pub const fn new(value: usize) -> Self {
        if !value.is_power_of_two() {
            panic!("Alignment value must be a power of two");
        }
        
        MemoryAlignment(value)
    }
    
    pub fn extra_elements<T>(&self) -> usize {
        let elem_size = size_of::<T>();
        (self.0 + elem_size - 1) / elem_size // ceil_div
    }
    
    pub fn align_ptr(&self, ptr: usize) -> usize {
        let mask = self.0 - 1;
        (ptr + mask) & !mask
    }
    
    pub fn unaligned_elements<T>(&self, ptr: usize) -> usize {
        let elem_size = size_of::<T>();
        let aligned_ptr = self.align_ptr(ptr);
        (aligned_ptr - ptr) / elem_size
    }
}

pub static CACHE_LINE_SIZE: MemoryAlignment = MemoryAlignment(64);
