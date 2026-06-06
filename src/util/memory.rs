use std::slice;

/// # SAFETY
///
/// See [`slice::from_raw_parts()`] for safety and usage.
pub unsafe fn view_as_bytes<T>(slice: &[T]) -> &[u8] {
    unsafe { slice::from_raw_parts(slice.as_ptr() as *const u8, size_of_val(slice)) }
}

/// # SAFETY
///
/// See [`slice::from_raw_parts_mut()`] for safety and usage.
pub unsafe fn view_as_bytes_mut<T>(slice: &mut [T]) -> &mut [u8] {
    unsafe { slice::from_raw_parts_mut(slice.as_mut_ptr() as *mut u8, size_of_val(slice)) }
}
