#[target_feature(enable = "avx512bw")]
unsafe fn bit_transpose_avx512bw_impl(input: &[u8], output: &mut [u8]) {
    use std::arch::x86_64::*;

    if input.len() % 64 != 0 {
        panic!("input size must be divisible by 64");
    }

    let mut in_ptr = input.as_ptr();
    let mut out_ptr = output.as_mut_ptr();

    let chunks = input.len() / 64;

    for _ in 0..chunks {
        unsafe {
            let v0 = _mm512_loadu_si512(in_ptr as *const __m512i);
            
            macro_rules! process_bit {
                ($v0:ident, $bit_index:literal, $out_ptr:ident) => {
                    // move target bit into sign bit
                    let shifted0 = _mm512_slli_epi16($v0, 7 - $bit_index);
            
                    // collect sign bits
                    let mask0 = _mm512_movepi8_mask(shifted0) as u64;
            
                    // store 16 gathered bits
                    *($out_ptr as *mut u64) = mask0;
            
                    $out_ptr = $out_ptr.add(8);
                };
            }
            
            process_bit!(v0, 0, out_ptr);
            process_bit!(v0, 1, out_ptr);
            process_bit!(v0, 2, out_ptr);
            process_bit!(v0, 3, out_ptr);
            process_bit!(v0, 4, out_ptr);
            process_bit!(v0, 5, out_ptr);
            process_bit!(v0, 6, out_ptr);
            process_bit!(v0, 7, out_ptr);

            in_ptr = in_ptr.add(64);
        }
    }
}

pub fn bit_transpose_avx512bw(input: &[u8], output: &mut [u8]) {
    unsafe {
        bit_transpose_avx512bw_impl(input, output)
    }
}
