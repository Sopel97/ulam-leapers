#[target_feature(enable = "avx512bw")]
unsafe fn bit_transpose_avx512bw_impl(input: &[u8], output: &mut [u8]) {
    use std::arch::x86_64::*;

    if !input.len().is_multiple_of(64) {
        panic!("input size must be divisible by 64");
    }

    let mut in_ptr = input.as_ptr();
    let mut out_ptr = output.as_mut_ptr();

    let chunks = input.len() / 64;

    let m0 = _mm512_set1_epi8(0x01);
    let m1 = _mm512_set1_epi8(0x02);
    let m2 = _mm512_set1_epi8(0x04);
    let m3 = _mm512_set1_epi8(0x08);
    let m4 = _mm512_set1_epi8(0x10);
    let m5 = _mm512_set1_epi8(0x20);
    let m6 = _mm512_set1_epi8(0x40);
    let m7 = _mm512_set1_epi8(-128i8);

    for _ in 0..chunks {
        unsafe {
            let v = _mm512_loadu_si512(in_ptr as *const __m512i);

            macro_rules! process_bit {
                ($v:ident, $bitmask:ident, $out_ptr:ident) => {
                    // Test specific bit directly into the mask register.
                    let k = _mm512_test_epi8_mask($v, $bitmask);

                    _store_mask64($out_ptr as *mut __mmask64, k);

                    $out_ptr = $out_ptr.add(8);
                };
            }

            process_bit!(v, m0, out_ptr);
            process_bit!(v, m1, out_ptr);
            process_bit!(v, m2, out_ptr);
            process_bit!(v, m3, out_ptr);
            process_bit!(v, m4, out_ptr);
            process_bit!(v, m5, out_ptr);
            process_bit!(v, m6, out_ptr);
            process_bit!(v, m7, out_ptr);

            in_ptr = in_ptr.add(64);
        }
    }
}

pub fn bit_transpose_avx512bw(input: &[u8], output: &mut [u8]) {
    unsafe {
        bit_transpose_avx512bw_impl(input, output)
    }
}
