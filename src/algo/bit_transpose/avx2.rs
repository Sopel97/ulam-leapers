macro_rules! process_bit {
    ($v0:ident, $v1:ident, $bit_index:literal, $out_ptr:ident) => {
        // move target bit into sign bit
        let shifted0 = _mm256_slli_epi16($v0, 7 - $bit_index);
        let shifted1 = _mm256_slli_epi16($v1, 7 - $bit_index);

        // collect sign bits
        let mask0 = _mm256_movemask_epi8(shifted0) as u32;
        let mask1 = _mm256_movemask_epi8(shifted1) as u32;

        // store 16 gathered bits
        *($out_ptr as *mut u32) = mask0;
        *($out_ptr.add(4) as *mut u32) = mask1;

        $out_ptr = $out_ptr.add(8);
    };
}

#[target_feature(enable = "avx2")]
unsafe fn bit_transpose_avx2_impl(input: &[u8], output: &mut [u8]) {
    use std::arch::x86_64::*;

    if input.len() % 64 != 0 {
        panic!("input size must be divisible by 64");
    }

    let mut in_ptr = input.as_ptr();
    let mut out_ptr = output.as_mut_ptr();

    let chunks = input.len() / 64;

    for _ in 0..chunks {
        unsafe {
            let v0 = _mm256_loadu_si256(in_ptr as *const __m256i);
            let v1 = _mm256_loadu_si256(in_ptr.add(32) as *const __m256i);

            process_bit!(v0, v1, 0, out_ptr);
            process_bit!(v0, v1, 1, out_ptr);
            process_bit!(v0, v1, 2, out_ptr);
            process_bit!(v0, v1, 3, out_ptr);
            process_bit!(v0, v1, 4, out_ptr);
            process_bit!(v0, v1, 5, out_ptr);
            process_bit!(v0, v1, 6, out_ptr);
            process_bit!(v0, v1, 7, out_ptr);

            in_ptr = in_ptr.add(64);
        }
    }
}

pub fn bit_transpose_avx2(input: &[u8], output: &mut [u8]) {
    unsafe {
        bit_transpose_avx2_impl(input, output)
    }
}
