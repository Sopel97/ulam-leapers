#[derive(Debug, PartialEq, PartialOrd, Eq, Ord, Clone, Copy)]
pub struct MemoryAlignment(usize);

impl MemoryAlignment {
    pub const fn new(value: usize) -> Self {
        if value == 0 {
            panic!("Memory alignment cannot be zero");
        }

        if !value.is_power_of_two() {
            panic!("Alignment value must be a power of two");
        }

        MemoryAlignment(value)
    }

    pub fn bytes(&self) -> usize {
        self.0
    }

    pub fn is_ptr_aligned<T>(&self, ptr: *const T) -> bool {
        (ptr as usize).is_multiple_of(self.0)
    }

    pub fn is_aligned(&self, ptr: usize) -> bool {
        ptr.is_multiple_of(self.0)
    }

    pub fn extra_elements<T>(&self) -> usize {
        let elem_size = size_of::<T>();
        self.0.div_ceil(elem_size)
    }

    pub fn align(&self, ptr: usize) -> usize {
        let mask = self.0 - 1;
        (ptr + mask) & !mask
    }

    pub fn unaligned_elements<T>(&self, ptr: usize) -> usize {
        let elem_size = size_of::<T>();
        let aligned_ptr = self.align(ptr);
        (aligned_ptr - ptr) / elem_size
    }
}

pub static CACHE_LINE_SIZE: MemoryAlignment = MemoryAlignment(64);

#[cfg(test)]
mod tests {
    use super::*;

    const fn a(n: usize) -> MemoryAlignment {
        MemoryAlignment::new(n)
    }

    /// All power-of-two alignments we exercise throughout the suite.
    const ALIGNMENTS: &[MemoryAlignment] = &[
        a(1),
        a(2),
        a(4),
        a(8),
        a(16),
        a(32),
        a(64),
        a(128),
        a(256),
        a(4096),
    ];

    #[test]
    #[should_panic]
    fn new_panics_on_zero() {
        MemoryAlignment::new(0);
    }

    #[test]
    #[should_panic]
    fn new_panics_on_non_power_of_two() {
        MemoryAlignment::new(3);
    }

    #[test]
    fn align_already_aligned_is_unchanged() {
        for align in ALIGNMENTS {
            let ptr = align.bytes() * 4;
            assert_eq!(
                align.align(ptr),
                ptr,
                "{align:?}: already-aligned ptr {ptr:#x} was moved"
            );
        }
    }

    #[test]
    fn align_result_is_multiple_of_alignment() {
        for align in ALIGNMENTS {
            for offset in 0..=align.bytes() {
                let ptr = align.bytes() * 8 + offset; // vary the low bits
                let aligned = align.align(ptr);
                assert!(
                    align.is_aligned(aligned),
                    "{align:?}: offset={offset}: result {aligned:#x} not aligned"
                );
            }
        }
    }

    #[test]
    fn align_does_not_undershoot() {
        for align in ALIGNMENTS {
            for offset in 0..=align.bytes() {
                let ptr = align.bytes() * 8 + offset;
                let aligned = align.align(ptr);
                assert!(
                    aligned >= ptr,
                    "{align:?}: align returned {aligned:#x} < ptr {ptr:#x}"
                );
            }
        }
    }

    #[test]
    fn align_advances_by_less_than_alignment() {
        for align in ALIGNMENTS {
            for offset in 0..=align.bytes() {
                let ptr = align.bytes() * 8 + offset;
                let aligned = align.align(ptr);
                let diff = aligned - ptr;
                assert!(
                    diff < align.bytes(),
                    "{align:?}: advanced by {diff} >= alignment",
                );
            }
        }
    }

    #[test]
    fn is_aligned_true_when_aligned() {
        for align in ALIGNMENTS {
            assert!(align.is_aligned(align.bytes()));
        }
    }

    #[test]
    fn is_ptr_aligned_false_when_not_aligned() {
        // A pointer to an odd byte address can never be 2-aligned or higher.
        for align in ALIGNMENTS.iter().filter(|&&a| a.bytes() > 1) {
            assert!(!align.is_aligned(align.bytes() + 1));
        }
    }

    #[test]
    fn extra_elements_u8_equals_alignment_bytes() {
        for align in ALIGNMENTS {
            assert_eq!(align.extra_elements::<u8>(), align.bytes());
        }
    }

    #[test]
    fn extra_elements_is_div_ceil() {
        struct Size3(u8, u8, u8);
        for align in ALIGNMENTS {
            let expected = align.bytes().div_ceil(3);
            assert_eq!(align.extra_elements::<Size3>(), expected);
        }
    }

    #[test]
    fn unaligned_elements_already_aligned_returns_zero() {
        for align in ALIGNMENTS {
            let ptr = align.bytes() * 16;
            assert_eq!(align.unaligned_elements::<u8>(ptr), 0);
        }
    }

    #[test]
    fn unaligned_elements_result_brings_ptr_to_alignment() {
        for align in ALIGNMENTS {
            for offset_bytes in 0..align.bytes() {
                let base = align.bytes() * 16 + offset_bytes;
                let skip = align.unaligned_elements::<u8>(base);
                let adjusted = base + skip * size_of::<u8>();
                assert!(align.is_aligned(adjusted));
            }
        }
    }

    #[test]
    fn unaligned_elements_u64_result_brings_ptr_to_alignment() {
        for align in ALIGNMENTS {
            for offset_bytes in (0..align.bytes()).step_by(8) {
                let base = align.bytes() * 16 + offset_bytes;
                let skip = align.unaligned_elements::<u64>(base);
                let adjusted = base + skip * size_of::<u64>();
                assert!(align.is_aligned(adjusted));
            }
        }
    }

    #[test]
    fn unaligned_elements_skip_is_strictly_less_than_extra_elements() {
        for align in ALIGNMENTS {
            let extra = align.extra_elements::<u8>();
            for offset_bytes in 0..align.bytes() {
                let ptr = align.bytes() * 16 + offset_bytes;
                let skip = align.unaligned_elements::<u8>(ptr);
                assert!(skip <= extra, "{align:?}: skip {skip} > extra {extra}");
            }
        }
    }

    #[test]
    fn equality_and_ordering() {
        assert_eq!(a(16), a(16));
        assert_ne!(a(16), a(32));
        assert!(a(16) < a(32));
        assert!(a(64) > a(32));
    }

    #[test]
    #[allow(clippy::clone_on_copy)]
    fn clone_and_copy() {
        let original = a(64);
        let cloned = original; // Copy
        let also_cloned = original.clone();
        assert_eq!(original, cloned);
        assert_eq!(original, also_cloned);
    }
}
