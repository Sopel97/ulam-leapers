use crate::util::align::MemoryAlignment;
use std::mem::MaybeUninit;
use std::ops::{Index, IndexMut};

pub struct AlignedBoxedSlice<T> {
    storage: Box<[T]>,
    align: MemoryAlignment,
    begin: usize,
    end: usize,
}

impl<T> AlignedBoxedSlice<T> {
    pub fn len(&self) -> usize {
        self.end - self.begin
    }
}

impl<T> Clone for AlignedBoxedSlice<T>
where
    T: Default + Copy,
{
    fn clone(&self) -> Self {
        let mut other = Self::new(self.end - self.begin, self.align);
        other.as_mut_slice().copy_from_slice(self.as_slice());
        other
    }
}

impl<T: Default + Clone> AlignedBoxedSlice<T> {
    pub fn new(size: usize, align: MemoryAlignment) -> AlignedBoxedSlice<T> {
        let extra = align.extra_elements::<T>();

        let storage = vec![Default::default(); size + extra].into_boxed_slice();

        let aligned_offset = align.unaligned_elements::<T>(storage.as_ptr() as usize);

        AlignedBoxedSlice {
            storage,
            align,
            begin: aligned_offset,
            end: aligned_offset + size,
        }
    }
}

impl<T: Default> AlignedBoxedSlice<MaybeUninit<T>> {
    pub fn new_uninit(size: usize, align: MemoryAlignment) -> Self {
        let extra = align.extra_elements::<T>();

        let storage = Box::new_uninit_slice(size + extra);

        let aligned_offset = align.unaligned_elements::<T>(storage.as_ptr() as usize);

        AlignedBoxedSlice {
            storage,
            align,
            begin: aligned_offset,
            end: aligned_offset + size,
        }
    }

    /// # SAFETY
    ///
    /// As with [`MaybeUninit::assume_init`],
    /// it is up to the caller to guarantee that the values
    /// really are in an initialized state.
    /// Calling this when the content is not yet fully initialized
    /// causes immediate undefined behavior.
    pub unsafe fn assume_init(mut self) -> AlignedBoxedSlice<T> {
        // We have to remember to initialize the elements the user has no access to.
        for x in &mut self.storage[..self.begin] {
            x.write(T::default());
        }

        for x in &mut self.storage[self.end..] {
            x.write(T::default());
        }

        // SAFETY: We have to assume the caller filled every element they have access too.
        //         We filled the padding above.
        let storage = unsafe { self.storage.assume_init() };
        AlignedBoxedSlice {
            storage,
            align: self.align,
            begin: self.begin,
            end: self.end,
        }
    }
}

impl<T> AlignedBoxedSlice<T> {
    pub fn as_slice(&self) -> &[T] {
        &self.storage[self.begin..self.end]
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.storage[self.begin..self.end]
    }

    /// # Safety
    ///
    /// Calling this method with an out-of-bounds index is *[undefined behavior]*
    /// even if the resulting reference is not used.
    ///
    /// [undefined behavior]: https://doc.rust-lang.org/reference/behavior-considered-undefined.html
    #[inline(always)]
    pub unsafe fn get_unchecked(&self, index: usize) -> &T {
        unsafe { self.storage.get_unchecked(index + self.begin) }
    }

    /// # Safety
    ///
    /// Calling this method with an out-of-bounds index is *[undefined behavior]*
    /// even if the resulting reference is not used.
    ///
    /// [undefined behavior]: https://doc.rust-lang.org/reference/behavior-considered-undefined.html
    #[inline(always)]
    pub unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut T {
        unsafe { self.storage.get_unchecked_mut(index + self.begin) }
    }
}

impl<T> Index<usize> for AlignedBoxedSlice<T> {
    type Output = T;

    #[inline(always)]
    fn index(&self, index: usize) -> &Self::Output {
        &self.storage[index + self.begin]
    }
}

impl<T> IndexMut<usize> for AlignedBoxedSlice<T> {
    #[inline(always)]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.storage[index + self.begin]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::align::MemoryAlignment;
    use std::mem::MaybeUninit;
    use std::panic::AssertUnwindSafe;

    #[derive(Clone, Copy, Eq, PartialEq, Debug, Ord, PartialOrd)]
    struct NonzeroDefault(u32);
    impl Default for NonzeroDefault {
        fn default() -> Self {
            NonzeroDefault(123456)
        }
    }

    /// All alignments we want to exercise in parameterised tests.
    fn all_alignments() -> Vec<MemoryAlignment> {
        vec![
            MemoryAlignment::new(16),
            MemoryAlignment::new(32),
            MemoryAlignment::new(64),
        ]
    }

    #[test]
    fn new_produces_correct_length() {
        for align in all_alignments() {
            let slice: AlignedBoxedSlice<u8> = AlignedBoxedSlice::new(64, align);
            assert_eq!(
                slice.len(),
                64,
                "Expected len 64 with alignment {align:?}"
            );
        }
    }

    #[test]
    fn new_zero_size_is_allowed() {
        for align in all_alignments() {
            let slice: AlignedBoxedSlice<u64> = AlignedBoxedSlice::new(0, align);
            assert_eq!(slice.len(), 0);
        }
    }

    #[test]
    fn new_data_pointer_is_aligned() {
        for align in all_alignments() {
            let slice: AlignedBoxedSlice<u8> = AlignedBoxedSlice::new(128, align);
            let ptr = slice.as_slice().as_ptr();
            assert!(
                align.is_ptr_aligned(ptr),
                "Pointer {ptr:p} is not aligned to {align:?}"
            );
        }
    }

    #[test]
    fn new_initialises_elements_to_default() {
        for align in all_alignments() {
            let slice: AlignedBoxedSlice<NonzeroDefault> = AlignedBoxedSlice::new(32, align);
            for (i, &v) in slice.as_slice().iter().enumerate() {
                assert_eq!(v, NonzeroDefault::default(), "Element {i} not zero-initialised (alignment {align:?})");
            }
        }
    }

    #[test]
    fn new_uninit_produces_correct_length() {
        for align in all_alignments() {
            let slice: AlignedBoxedSlice<MaybeUninit<u32>> =
                AlignedBoxedSlice::new_uninit(64, align);
            assert_eq!(slice.len(), 64);
        }
    }

    #[test]
    fn new_uninit_data_pointer_is_aligned() {
        for align in all_alignments() {
            let slice: AlignedBoxedSlice<MaybeUninit<u8>> =
                AlignedBoxedSlice::new_uninit(128, align);
            let ptr = slice.as_slice().as_ptr();
            assert!(
                align.is_ptr_aligned(ptr),
                "Uninit pointer {ptr:p} is not aligned to {align:?}"
            );
        }
    }

    #[test]
    fn assume_init_round_trips_values() {
        for align in all_alignments() {
            let mut uninit: AlignedBoxedSlice<MaybeUninit<u64>> =
                AlignedBoxedSlice::new_uninit(16, align);

            // Write every element the caller owns.
            for (i, slot) in uninit.as_mut_slice().iter_mut().enumerate() {
                slot.write(i as u64 * 7);
            }

            let init: AlignedBoxedSlice<u64> = unsafe { uninit.assume_init() };

            for (i, &v) in init.as_slice().iter().enumerate() {
                assert_eq!(
                    v,
                    i as u64 * 7,
                    "Value at index {i} corrupted after assume_init (alignment {align:?})"
                );
            }
        }
    }

    #[test]
    fn assume_init_pointer_is_still_aligned() {
        for align in all_alignments() {
            let mut uninit: AlignedBoxedSlice<MaybeUninit<u8>> =
                AlignedBoxedSlice::new_uninit(64, align);
            for slot in uninit.as_mut_slice().iter_mut() {
                slot.write(0);
            }
            let init: AlignedBoxedSlice<u8> = unsafe { uninit.assume_init() };
            let ptr = init.as_slice().as_ptr();
            assert!(align.is_ptr_aligned(ptr));
        }
    }

    #[test]
    fn assume_init_length_is_preserved() {
        for align in all_alignments() {
            let size = 48;
            let mut uninit: AlignedBoxedSlice<MaybeUninit<u32>> =
                AlignedBoxedSlice::new_uninit(size, align);
            for slot in uninit.as_mut_slice().iter_mut() {
                slot.write(0);
            }
            let init: AlignedBoxedSlice<u32> = unsafe { uninit.assume_init() };
            assert_eq!(init.len(), size);
        }
    }

    #[test]
    fn index_read_returns_correct_value() {
        for align in all_alignments() {
            let mut slice: AlignedBoxedSlice<i32> = AlignedBoxedSlice::new(8, align);
            for i in 0..8 {
                slice[i] = i as i32 * 3;
            }
            for i in 0..8 {
                assert_eq!(slice[i], i as i32 * 3);
            }
        }
    }

    #[test]
    #[should_panic]
    fn index_out_of_bounds_panics() {
        let slice: AlignedBoxedSlice<u8> = AlignedBoxedSlice::new(4, MemoryAlignment::new(16));

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = slice[4];
        }));

        assert!(result.is_err());
    }

    #[test]
    fn get_unchecked_returns_correct_value() {
        for align in all_alignments() {
            let mut slice: AlignedBoxedSlice<u32> = AlignedBoxedSlice::new(8, align);
            for i in 0..8 {
                slice[i] = i as u32 + 1;
            }
            for i in 0..8 {
                let v = unsafe { *slice.get_unchecked(i) };
                assert_eq!(v, i as u32 + 1);
            }
        }
    }

    #[test]
    fn get_unchecked_mut_allows_mutation() {
        for align in all_alignments() {
            let mut slice: AlignedBoxedSlice<u32> = AlignedBoxedSlice::new(4, align);
            unsafe {
                *slice.get_unchecked_mut(0) = 42;
                *slice.get_unchecked_mut(3) = 99;
            }
            assert_eq!(slice[0], 42);
            assert_eq!(slice[3], 99);
        }
    }

    #[test]
    fn as_slice_len_matches_requested_size() {
        for align in all_alignments() {
            let slice: AlignedBoxedSlice<f32> = AlignedBoxedSlice::new(100, align);
            assert_eq!(slice.as_slice().len(), 100);
        }
    }

    #[test]
    fn as_mut_slice_writes_are_visible_via_index() {
        for align in all_alignments() {
            let mut slice: AlignedBoxedSlice<u8> = AlignedBoxedSlice::new(8, align);
            for (i, byte) in slice.as_mut_slice().iter_mut().enumerate() {
                *byte = i as u8 * 2;
            }
            for i in 0..8 {
                assert_eq!(slice[i], i as u8 * 2);
            }
        }
    }

    #[test]
    fn as_slice_and_index_agree_on_same_data() {
        for align in all_alignments() {
            let mut slice: AlignedBoxedSlice<u16> = AlignedBoxedSlice::new(16, align);
            for i in 0..16 {
                slice[i] = i as u16;
            }
            let s = slice.as_slice();
            for i in 0..16 {
                assert_eq!(s[i], slice[i]);
            }
        }
    }

    #[test]
    fn clone_has_correct_alignment() {
        for align in all_alignments() {
            let slice: AlignedBoxedSlice<u32> = AlignedBoxedSlice::new(32, align);
            let cloned = slice.clone();
            let ptr = cloned.as_slice().as_ptr();
            assert!(
                align.is_ptr_aligned(ptr),
                "Clone pointer {ptr:p} not aligned to {align:?}"
            );
        }
    }

    #[test]
    fn clone_has_same_length() {
        for align in all_alignments() {
            let slice: AlignedBoxedSlice<u32> = AlignedBoxedSlice::new(32, align);
            let cloned = slice.clone();
            assert_eq!(cloned.len(), slice.len());
        }
    }

    #[test]
    fn clone_has_same_values() {
        for align in all_alignments() {
            let mut slice: AlignedBoxedSlice<u32> = AlignedBoxedSlice::new(8, align);
            for i in 0..8 {
                slice[i] = i as u32 * 5;
            }
            let cloned = slice.clone();
            assert_eq!(cloned.as_slice(), slice.as_slice());
        }
    }

    #[test]
    fn clone_is_independent_of_original() {
        for align in all_alignments() {
            let mut original: AlignedBoxedSlice<u32> = AlignedBoxedSlice::new(4, align);
            for i in 0..4 {
                original[i] = i as u32;
            }
            let mut cloned = original.clone();

            // Mutate the clone; original must not change.
            for i in 0..4 {
                cloned[i] = 999;
            }
            for i in 0..4 {
                assert_eq!(original[i], i as u32, "Original was mutated through clone");
            }
        }
    }
}
