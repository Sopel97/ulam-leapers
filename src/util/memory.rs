use std::slice;

pub fn as_bytes<T>(slice: &[T]) -> &[u8] {
    unsafe {
        slice::from_raw_parts(
            slice.as_ptr() as *const u8,
            std::mem::size_of_val(slice),
        )
    }
}

pub fn as_bytes_mut<T>(slice: &mut [T]) -> &mut [u8] {
    unsafe {
        slice::from_raw_parts_mut(
            slice.as_mut_ptr() as *mut u8,
            std::mem::size_of_val(slice),
        )
    }
}
