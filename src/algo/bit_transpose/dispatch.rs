use std::sync::OnceLock;
use super::sse2;
use super::avx2;
use super::avx512bw;

type FnTy = fn(&[u8], &mut [u8]);

static BIT_TRANSPOSE_TRAMPOLINE: OnceLock<FnTy> = OnceLock::new();
static INV_BIT_TRANSPOSE_TRAMPOLINE: OnceLock<FnTy> = OnceLock::new();

// Transposes bits in [u8;64] chunks such that each consecutive 8 bytes of the output
// contains 64 nth bits of the input. Least significant bits first.
pub fn bit_transpose(input: &[u8], output: &mut [u8]) {
    let f = BIT_TRANSPOSE_TRAMPOLINE.get_or_init(|| {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            if is_x86_feature_detected!("avx512bw") {
                return unsafe {
                    std::mem::transmute(avx512bw::bit_transpose_avx512bw as *const ())
                };
            }

            if is_x86_feature_detected!("avx2") {
                return unsafe {
                    std::mem::transmute(avx2::bit_transpose_avx2 as *const ())
                };
            }

            if is_x86_feature_detected!("sse2") {
                return unsafe {
                    std::mem::transmute(sse2::bit_transpose_sse2 as *const ())
                };
            }
        }

        panic!("Unimplemented");
    });

    f(input, output);
}

pub fn inv_bit_transpose(input: &[u8], output: &mut [u8]) {
    let f = INV_BIT_TRANSPOSE_TRAMPOLINE.get_or_init(|| {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            // AVX2 and AVX-512 could be faster here, but it gets complicated,
            // and we don't need faster inverse transpose for now.

            if is_x86_feature_detected!("sse2") {
                return unsafe {
                    std::mem::transmute(sse2::inv_bit_transpose_sse2 as *const ())
                };
            }
        }

        panic!("Unimplemented");
    });

    f(input, output);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(align(64))]
    struct Wrapper([u8; 128]);

    // 2 chunks to test chunked processing too
    fn make_test_case() -> (Wrapper, Wrapper, [u8; 128]) {
        let input: Wrapper = Wrapper(core::array::from_fn(|i| (i % 64) as u8));
        let output = Wrapper([0_u8; 128]);
        let expected: [u8; 128] = {
            let a = 0b10101010u8;
            let b = 0b11001100u8;
            let c = 0b11110000u8;
            let d = 0b11111111u8;
            [
                a, a, a, a, a, a, a, a, // lowest bit pattern
                b, b, b, b, b, b, b, b,
                c, c, c, c, c, c, c, c,
                0, d, 0, d, 0, d, 0, d,
                0, 0, d, d, 0, 0, d, d,
                0, 0, 0, 0, d, d, d, d,
                0, 0, 0, 0, 0, 0, 0, 0, // highest 2 bits are not set because max value is 63
                0, 0, 0, 0, 0, 0, 0, 0,
                // repeat for the second chunk
                a, a, a, a, a, a, a, a, // lowest bit pattern
                b, b, b, b, b, b, b, b,
                c, c, c, c, c, c, c, c,
                0, d, 0, d, 0, d, 0, d,
                0, 0, d, d, 0, 0, d, d,
                0, 0, 0, 0, d, d, d, d,
                0, 0, 0, 0, 0, 0, 0, 0, // highest 2 bits are not set because max value is 63
                0, 0, 0, 0, 0, 0, 0, 0,
            ]
        };

        (input, output, expected)
    }

    #[test]
    fn test_transpose_sse2() {
        if !is_x86_feature_detected!("sse2") {
            return;
        }

        let (input, mut output, expected) = make_test_case();

        sse2::bit_transpose_sse2(&input.0, &mut output.0);

        assert_eq!(output.0, expected);
    }

    #[test]
    fn test_transpose_avx2() {
        if !is_x86_feature_detected!("avx2") {
            return;
        }

        let (input, mut output, expected) = make_test_case();

        avx2::bit_transpose_avx2(&input.0, &mut output.0);

        assert_eq!(output.0, expected);
    }

    #[test]
    fn test_transpose_avx512bw() {
        if !is_x86_feature_detected!("avx512bw") {
            return;
        }

        let (input, mut output, expected) = make_test_case();

        avx512bw::bit_transpose_avx512bw(&input.0, &mut output.0);

        assert_eq!(output.0, expected);
    }

    #[test]
    fn test_round_trip_sse2() {
        if !is_x86_feature_detected!("sse2") {
            return;
        }

        let (input, mut output, expected) = make_test_case();

        sse2::bit_transpose_sse2(&input.0, &mut output.0);

        assert_eq!(output.0, expected);

        let mut output2 = Wrapper([0_u8; 128]);

        sse2::inv_bit_transpose_sse2(&output.0, &mut output2.0);

        assert_eq!(input.0, output2.0);
    }
}