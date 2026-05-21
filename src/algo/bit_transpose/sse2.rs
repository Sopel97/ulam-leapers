#[target_feature(enable = "sse2")]
unsafe fn bit_transpose_sse2_impl(input: &[u8], output: &mut [u8]) {
    use std::arch::x86_64::*;

    if input.len() % 64 != 0 {
        panic!("input size must be divisible by 64");
    }

    let mut in_ptr = input.as_ptr();
    let mut out_ptr = output.as_mut_ptr();

    let chunks = input.len() / 64;

    for _ in 0..chunks {
        unsafe {
            let v0 = _mm_loadu_si128(in_ptr as *const __m128i);
            let v1 = _mm_loadu_si128(in_ptr.add(16) as *const __m128i);
            let v2 = _mm_loadu_si128(in_ptr.add(32) as *const __m128i);
            let v3 = _mm_loadu_si128(in_ptr.add(48) as *const __m128i);
            
            macro_rules! process_bit {
                ($v0:ident, $v1:ident, $v2:ident, $v3:ident, $bit_index:literal, $out_ptr:ident) => {
                    // move target bit into sign bit
                    let shifted0 = _mm_slli_epi16($v0, 7 - $bit_index);
                    let shifted1 = _mm_slli_epi16($v1, 7 - $bit_index);
                    let shifted2 = _mm_slli_epi16($v2, 7 - $bit_index);
                    let shifted3 = _mm_slli_epi16($v3, 7 - $bit_index);
            
                    // collect sign bits
                    let mask0 = _mm_movemask_epi8(shifted0) as u16;
                    let mask1 = _mm_movemask_epi8(shifted1) as u16;
                    let mask2 = _mm_movemask_epi8(shifted2) as u16;
                    let mask3 = _mm_movemask_epi8(shifted3) as u16;
            
                    // store 16 gathered bits
                    *($out_ptr as *mut u16) = mask0;
                    *($out_ptr.add(2) as *mut u16) = mask1;
                    *($out_ptr.add(4) as *mut u16) = mask2;
                    *($out_ptr.add(6) as *mut u16) = mask3;
            
                    $out_ptr = $out_ptr.add(8);
                };
            }
            
            process_bit!(v0, v1, v2, v3, 0, out_ptr);
            process_bit!(v0, v1, v2, v3, 1, out_ptr);
            process_bit!(v0, v1, v2, v3, 2, out_ptr);
            process_bit!(v0, v1, v2, v3, 3, out_ptr);
            process_bit!(v0, v1, v2, v3, 4, out_ptr);
            process_bit!(v0, v1, v2, v3, 5, out_ptr);
            process_bit!(v0, v1, v2, v3, 6, out_ptr);
            process_bit!(v0, v1, v2, v3, 7, out_ptr);

            in_ptr = in_ptr.add(64);
        }
    }
}

pub fn bit_transpose_sse2(input: &[u8], output: &mut [u8]) {
    unsafe {
        bit_transpose_sse2_impl(input, output)
    }
}

#[target_feature(enable = "sse2")]
unsafe fn inv_bit_transpose_sse2_impl(input: &[u8], output: &mut [u8]) {
    use std::arch::x86_64::*;

    if input.len() % 64 != 0 {
        panic!("input size must be divisible by 64");
    }

    let mut in_ptr = input.as_ptr();
    let mut out_ptr = output.as_mut_ptr();

    let chunks = input.len() / 64;

    for _ in 0..chunks {
        unsafe {
            // The digits denote a byte containing only nth bits.
            // We start with 8 consecutive bytes for each original bit position.
            // We want to transpose the bytes such that each subsequent byte contains
            // a new bit, so that we can reform the numbers via movemask.
            // v0 = 0 0 0 0 0 0 0 0 | 1 1 1 1 1 1 1 1
            // v1 = 2 2 2 2 2 2 2 2 | 3 3 3 3 3 3 3 3
            let v0 = _mm_loadu_si128(in_ptr as *const __m128i);
            let v1 = _mm_loadu_si128(in_ptr.add(16) as *const __m128i);
            let v2 = _mm_loadu_si128(in_ptr.add(32) as *const __m128i);
            let v3 = _mm_loadu_si128(in_ptr.add(48) as *const __m128i);

            // x0 = 0 0 0 0 0 0 0 0 | 2 2 2 2 2 2 2 2
            // x1 = 1 1 1 1 1 1 1 1 | 3 3 3 3 3 3 3 3
            let x0 = _mm_unpacklo_epi64(v0, v1);
            let x1 = _mm_unpackhi_epi64(v0, v1);
            let x2 = _mm_unpacklo_epi64(v2, v3);
            let x3 = _mm_unpackhi_epi64(v2, v3);

            // y0 = 0 1 0 1 0 1 0 1 | 0 1 0 1 0 1 0 1
            // y1 = 2 3 2 3 2 3 2 3 | 2 3 2 3 2 3 2 3
            let y0 = _mm_unpacklo_epi8(x0, x1);
            let y1 = _mm_unpackhi_epi8(x0, x1);
            let y2 = _mm_unpacklo_epi8(x2, x3);
            let y3 = _mm_unpackhi_epi8(x2, x3);

            // z0 ~= z1 = 0 1 2 3 0 1 2 3 | 0 1 2 3 0 1 2 3
            // z2 ~= z3 = 4 5 6 7 4 5 6 7 | 4 5 6 7 4 5 6 7
            let z0 = _mm_unpacklo_epi16(y0, y1);
            let z1 = _mm_unpackhi_epi16(y0, y1);
            let z2 = _mm_unpacklo_epi16(y2, y3);
            let z3 = _mm_unpackhi_epi16(y2, y3);

            // a0 ~= a1 ~= a2 ~= a3 = 0 1 2 3 4 5 6 7 | 0 1 2 3 4 5 6 7
            let a0 = _mm_unpacklo_epi32(z0, z2);
            let a1 = _mm_unpackhi_epi32(z0, z2);
            let a2 = _mm_unpacklo_epi32(z1, z3);
            let a3 = _mm_unpackhi_epi32(z1, z3);

            // Since there is no _mm_movemask_epi16 we have to produce 2 output bytes at a time.
            // To achieve this in a way that doesn't use 64-bit registers we can
            // interleave the bits and do 4 iterations of _mm_movemask_epi8 instead of 8.
            // We are effectively unrolling by a factor of 2 by doing shifts n and n+1 at once.
            let b0 = _mm_unpacklo_epi64(_mm_slli_epi16(a0, 1), a0);
            let b1 = _mm_unpackhi_epi64(_mm_slli_epi16(a0, 1), a0);
            let b2 = _mm_unpacklo_epi64(_mm_slli_epi16(a1, 1), a1);
            let b3 = _mm_unpackhi_epi64(_mm_slli_epi16(a1, 1), a1);
            let b4 = _mm_unpacklo_epi64(_mm_slli_epi16(a2, 1), a2);
            let b5 = _mm_unpackhi_epi64(_mm_slli_epi16(a2, 1), a2);
            let b6 = _mm_unpacklo_epi64(_mm_slli_epi16(a3, 1), a3);
            let b7 = _mm_unpackhi_epi64(_mm_slli_epi16(a3, 1), a3);

            macro_rules! process_bit {
                ($v0:ident, $out_ptr:ident) => {
                    // We set it up to only need even shifts because
                    // both 64-bit halves contribute to the output
                    let shifted0 = _mm_slli_epi16($v0, 6);
                    let shifted1 = _mm_slli_epi16($v0, 4);
                    let shifted2 = _mm_slli_epi16($v0, 2);
                    let shifted3 = $v0;

                    let mask0 = _mm_movemask_epi8(shifted0) as u16;
                    let mask1 = _mm_movemask_epi8(shifted1) as u16;
                    let mask2 = _mm_movemask_epi8(shifted2) as u16;
                    let mask3 = _mm_movemask_epi8(shifted3) as u16;

                    *($out_ptr as *mut u16) = mask0;
                    *($out_ptr.add(2) as *mut u16) = mask1;
                    *($out_ptr.add(4) as *mut u16) = mask2;
                    *($out_ptr.add(6) as *mut u16) = mask3;

                    $out_ptr = $out_ptr.add(8);
                };
            }

            process_bit!(b0, out_ptr);
            process_bit!(b1, out_ptr);
            process_bit!(b2, out_ptr);
            process_bit!(b3, out_ptr);
            process_bit!(b4, out_ptr);
            process_bit!(b5, out_ptr);
            process_bit!(b6, out_ptr);
            process_bit!(b7, out_ptr);

            in_ptr = in_ptr.add(64);
        }
    }
}

pub fn inv_bit_transpose_sse2(input: &[u8], output: &mut [u8]) {
    unsafe {
        inv_bit_transpose_sse2_impl(input, output)
    }
}
