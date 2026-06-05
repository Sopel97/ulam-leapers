/// # Safety
///
/// This function is called locally on a known good memory region.
/// Alignment is also checked by the caller.
#[inline(always)]
#[allow(clippy::erasing_op)]
#[allow(clippy::identity_op)]
unsafe fn transpose_u8_8x8_kernel(
    src: *const u8,
    dst: *mut u8,
    src_stride: usize,
    dst_stride: usize,
) {
    unsafe {
        // load rows of source matrix
        let a0 = *(src.add(0 * src_stride) as *const u64);
        let a1 = *(src.add(1 * src_stride) as *const u64);
        let a2 = *(src.add(2 * src_stride) as *const u64);
        let a3 = *(src.add(3 * src_stride) as *const u64);
        let a4 = *(src.add(4 * src_stride) as *const u64);
        let a5 = *(src.add(5 * src_stride) as *const u64);
        let a6 = *(src.add(6 * src_stride) as *const u64);
        let a7 = *(src.add(7 * src_stride) as *const u64);

        // 2x2 block matrices
        let b0 = (a0 & 0x00ff00ff00ff00ffu64)
            | ((a1 << 8) & 0xff00ff00ff00ff00u64);
        let b1 = (a1 & 0xff00ff00ff00ff00u64)
            | ((a0 >> 8) & 0x00ff00ff00ff00ffu64);

        let b2 = (a2 & 0x00ff00ff00ff00ffu64)
            | ((a3 << 8) & 0xff00ff00ff00ff00u64);
        let b3 = (a3 & 0xff00ff00ff00ff00u64)
            | ((a2 >> 8) & 0x00ff00ff00ff00ffu64);

        let b4 = (a4 & 0x00ff00ff00ff00ffu64)
            | ((a5 << 8) & 0xff00ff00ff00ff00u64);
        let b5 = (a5 & 0xff00ff00ff00ff00u64)
            | ((a4 >> 8) & 0x00ff00ff00ff00ffu64);

        let b6 = (a6 & 0x00ff00ff00ff00ffu64)
            | ((a7 << 8) & 0xff00ff00ff00ff00u64);
        let b7 = (a7 & 0xff00ff00ff00ff00u64)
            | ((a6 >> 8) & 0x00ff00ff00ff00ffu64);

        // 4x4 block matrices
        let c0 = (b0 & 0x0000ffff0000ffffu64)
            | ((b2 << 16) & 0xffff0000ffff0000u64);
        let c1 = (b1 & 0x0000ffff0000ffffu64)
            | ((b3 << 16) & 0xffff0000ffff0000u64);

        let c2 = (b2 & 0xffff0000ffff0000u64)
            | ((b0 >> 16) & 0x0000ffff0000ffffu64);
        let c3 = (b3 & 0xffff0000ffff0000u64)
            | ((b1 >> 16) & 0x0000ffff0000ffffu64);

        let c4 = (b4 & 0x0000ffff0000ffffu64)
            | ((b6 << 16) & 0xffff0000ffff0000u64);
        let c5 = (b5 & 0x0000ffff0000ffffu64)
            | ((b7 << 16) & 0xffff0000ffff0000u64);

        let c6 = (b6 & 0xffff0000ffff0000u64)
            | ((b4 >> 16) & 0x0000ffff0000ffffu64);
        let c7 = (b7 & 0xffff0000ffff0000u64)
            | ((b5 >> 16) & 0x0000ffff0000ffffu64);

        // 8x8 block matrix
        let d0 = (c0 & 0x00000000ffffffffu64)
            | ((c4 << 32) & 0xffffffff00000000u64);
        let d1 = (c1 & 0x00000000ffffffffu64)
            | ((c5 << 32) & 0xffffffff00000000u64);
        let d2 = (c2 & 0x00000000ffffffffu64)
            | ((c6 << 32) & 0xffffffff00000000u64);
        let d3 = (c3 & 0x00000000ffffffffu64)
            | ((c7 << 32) & 0xffffffff00000000u64);

        let d4 = (c4 & 0xffffffff00000000u64)
            | ((c0 >> 32) & 0x00000000ffffffffu64);
        let d5 = (c5 & 0xffffffff00000000u64)
            | ((c1 >> 32) & 0x00000000ffffffffu64);
        let d6 = (c6 & 0xffffffff00000000u64)
            | ((c2 >> 32) & 0x00000000ffffffffu64);
        let d7 = (c7 & 0xffffffff00000000u64)
            | ((c3 >> 32) & 0x00000000ffffffffu64);

        // store rows
        *(dst.add(0 * dst_stride) as *mut u64) = d0;
        *(dst.add(1 * dst_stride) as *mut u64) = d1;
        *(dst.add(2 * dst_stride) as *mut u64) = d2;
        *(dst.add(3 * dst_stride) as *mut u64) = d3;
        *(dst.add(4 * dst_stride) as *mut u64) = d4;
        *(dst.add(5 * dst_stride) as *mut u64) = d5;
        *(dst.add(6 * dst_stride) as *mut u64) = d6;
        *(dst.add(7 * dst_stride) as *mut u64) = d7;
    }
}

/// Requires cache line aligned source and destination buffers.
/// Sizes must be a multiple of 64.
/// Panics if these requirements are not met.
pub fn transpose_u8(src: &[u8], dst: &mut [u8], cols: usize, rows: usize) {
    const BLOCK_SIZE: usize = 64;
    const CACHE_LINE_SIZE: usize = 64;

    assert!(cols.is_multiple_of(64));
    assert!(rows.is_multiple_of(64));

    assert_eq!(src.len(), cols * rows);
    assert_eq!(dst.len(), cols * rows);

    let src_stride = cols;
    let dst_stride = rows;

    let src_base = src.as_ptr();
    let dst_base = dst.as_mut_ptr();

    assert_eq!(src_base.align_offset(CACHE_LINE_SIZE), 0);
    assert_eq!(dst_base.align_offset(CACHE_LINE_SIZE), 0);

    // SAFETY: All indexing is done within the bounds of the buffers.
    unsafe {
        // iterate over 64×64 blocks
        for row_block in 0..(rows / BLOCK_SIZE) {
            for col_block in 0..(cols / BLOCK_SIZE) {
                let src_block_origin =
                    src_base.add(row_block * BLOCK_SIZE * src_stride + col_block * BLOCK_SIZE);

                let dst_block_origin =
                    dst_base.add(col_block * BLOCK_SIZE * dst_stride + row_block * BLOCK_SIZE);

                // iterate over 8×8 sub-blocks inside the 64×64 block
                for row_subblock in 0..8 {
                    for col_subblock in 0..8 {
                        let src_subblock_origin =
                            src_block_origin.add(col_subblock * src_stride * 8 + row_subblock * 8);

                        let dst_subblock_origin =
                            dst_block_origin.add(row_subblock * dst_stride * 8 + col_subblock * 8);

                        transpose_u8_8x8_kernel(
                            src_subblock_origin,
                            dst_subblock_origin,
                            src_stride,
                            dst_stride,
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::aligned_boxed_slice::AlignedBoxedSlice;
    use crate::util::align::MemoryAlignment;

    fn test_transpose(rows: usize, cols: usize) {
        let len = rows * cols;
        let mut src = AlignedBoxedSlice::new(len, MemoryAlignment::new(64));
        for r in 0..rows {
            for c in 0..cols {
                let i = r * cols + c;
                src[i] = (i % 179) as u8;
            }
        }
        let mut dst = AlignedBoxedSlice::new(len, MemoryAlignment::new(64));
        transpose_u8(src.as_slice(), dst.as_mut_slice(), cols, rows);
        for c in 0..cols {
            for r in 0..rows {
                let i = c * rows + r;
                let v = r * cols + c;
                assert_eq!(dst[i], (v % 179) as u8);
            }
        }
    }

    #[test]
    fn test_transpose_square() {
        test_transpose(64*2, 64*2);
    }

    #[test]
    fn test_transpose_rect_horizontal() {
        test_transpose(64*4, 64*2);
    }

    #[test]
    fn test_transpose_rect_vertical() {
        test_transpose(64*2, 64*4);
    }
}