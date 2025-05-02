use core::{
    cmp::min,
    iter::FusedIterator,
    ops::{BitAnd, BitOr},
    simd::{cmp::SimdPartialEq, LaneCount, Mask, Simd, SupportedLaneCount},
};

use crate::{BytesMask, Pattern};

/// exactly like `debug_assert!` but also adds an unreachable_unchecked branch
/// in release mode
macro_rules! debug_assert_opt {
    ($cond:expr) => {
        debug_assert!($cond);
        if !($cond) {
            unsafe { ::core::hint::unreachable_unchecked() };
        }
    };
}

/// An [`Iterator`] for searching a given [`Pattern`] in data
#[must_use]
#[derive(Clone)]
pub struct Scanner<'pattern, 'data, const ALIGNMENT: usize, const BYTES: usize>
where
    LaneCount<ALIGNMENT>: SupportedLaneCount,
    LaneCount<BYTES>: SupportedLaneCount,
{
    /// needle
    pattern: &'pattern Pattern<ALIGNMENT, BYTES>,
    /// one bit for each byte in [`BYTES`]
    /// little endian least significant bit corresponds to the first byte in the
    /// current slice of data
    candidates_mask: BytesMask,
    /// pointer to first valid byte of data
    data: &'data [u8],
    /// pointer to one byte past the end of data
    end: usize,
    /// iterator position
    position: usize,
    /// indicates that `self.position + BYTES > self.end`
    exhausted: bool,
}

impl<'pattern, 'data, const ALIGNMENT: usize, const BYTES: usize>
    Scanner<'pattern, 'data, ALIGNMENT, BYTES>
where
    LaneCount<ALIGNMENT>: SupportedLaneCount,
    LaneCount<BYTES>: SupportedLaneCount,
{
    const _ALIGNED: bool = Self::validate_alignment();

    const fn validate_alignment() -> bool {
        if ALIGNMENT > BYTES {
            panic!("Pattern ALIGNMENT must be less or equal to BYTES");
        }
        true
    }

    /// Creates an [`Iterator`], see also [`Pattern::matches`]
    ///
    /// # Panics
    /// Panics when `data.len() > usize::MAX - BYTES`.
    ///
    /// In the real world, it's near impossible to create a buffer near the size
    /// of [`usize::max`]. This reserved space is required to keep the hot loop
    /// efficient while still providing a correct algorithm.
    pub fn new(pattern: &'pattern Pattern<ALIGNMENT, BYTES>, data: &'data [u8]) -> Self {
        let _aligned = Self::_ALIGNED;
        debug_assert_opt!(data.len() <= usize::MAX - BYTES);
        debug_assert_opt!(!data.is_empty());
        debug_assert_opt!(
            ((&data[data.len() - 1]) as *const u8 as usize) <= usize::MAX - 3 * BYTES
        );

        // data + align_offset required to align to BYTES
        let align_offset = Self::first_offset(data.as_ptr(), pattern.first_byte_offset);
        let candidates_mask = Self::initial_candidates(pattern, data, align_offset);

        // set position out of bounds.
        // next() will use it as base for candidates offsets,
        // then increment by BYTES to search for new candidates,
        // increasing position to be in bounds again.
        // exception: align_offset > data.len()
        // this will be checked before searching for new candidates
        // # Safety
        // it is assumed that data.as_ptr() - BYTES doesn't underflow
        let data_addr = data.as_ptr().addr();
        let position = data_addr + align_offset - BYTES;
        let end = data_addr + data.len();

        Self {
            pattern,
            data,
            end,
            position,
            candidates_mask,
            exhausted: position + 2 * BYTES >= end,
        }
    }

    fn first_offset(data: *const u8, first_byte_offset: u8) -> usize {
        let mut align_offset = data.align_offset(align_of::<Simd<u8, BYTES>>());
        if align_offset == 0 {
            align_offset = BYTES;
        }
        let data_align = align_offset % ALIGNMENT;
        let first_possible = data_align + first_byte_offset as usize;
        if align_offset <= first_possible {
            align_offset += BYTES;
        }
        align_offset
    }

    #[inline]
    fn initial_candidates(
        pattern: &Pattern<ALIGNMENT, BYTES>,
        data: &[u8],
        align_offset: usize,
    ) -> BytesMask {
        // The general idea is to eliminate extra branches inside the hot loop.
        // For that, the potentially unaligned start of the dataset needs to get
        // prepared to behave exactly like the hot loop.
        // This is done by setting the data pointer out of bounds and using a candidate
        // mask that is shifted to have its end align with the start of the
        // first BYTES-aligned chunk.
        //
        // Consider these pointers:
        // ----------------dddddddddddddbbbbbbbbbbbbbbbbbbbbbb
        // ^               ^            ^-BYTES aligned data
        // ^               ^-real start of data
        // ^-aligned data sub BYTES
        // ----------------dd???aaaaaaaabbbbbbbbbbbbbbbbbbbbbb
        //                   ^  ^-wildcards at the start require offsetting, this could
        //                   ^    reach into the aligned part of data
        //                   ^-pattern alignment allows to throw away the unaligned
        //                     start
        // ----------------dd???|xxxxxxx|bbbbbbbbbbbbbbbbbbbbb
        //                      ^-first candidates search in this area, bail if len <= 0
        // ---------------------|x---x---------------|
        //                       ^-reduce bitmask to pattern alignment
        // ---------------------ddddddddd---------------------
        // |-----------------------x---x|
        //                             ^-shift to end

        // data + data_align is the offset of the first possible valid candidate
        // + the offset defined by the candidates pattern
        let data_align = align_offset % ALIGNMENT;

        // if the data is shorter than the pattern, there will never be a match
        if data.len().saturating_sub(data_align) < pattern.length as usize {
            return 0;
        }

        let first_possible = data_align + pattern.first_byte_offset as usize;
        let max_offset = min(align_offset, data.len());
        // alignment_first_possible_eq_data() is an edge case where valid inputs
        // can trigger this branch
        //
        // it is fine to not check candidates in this case because the pattern specifies
        // a required alignment. the alignment requirement reduces the amount of
        // valid bytes in data, essentially causing
        // `data[data_align..].len() < pattern.length`
        // (see above where this is checked now)
        //
        // if first_possible == max_offset {
        //     return 0;
        // }
        debug_assert_opt!(first_possible < max_offset);

        // compute the first candidates
        let result = unsafe {
            Self::build_candidates::<true>(
                data.as_ptr().add(first_possible),
                max_offset - first_possible,
                pattern,
            )
        };

        // shift result to align to end of currently aligned (out of bounds starting)
        // slice
        result << (BYTES + first_possible - align_offset)
    }

    fn end_candidates(&mut self) {
        debug_assert_opt!(self.end >= self.position);
        // # Safety
        // self.end and self.position are both initialized from self.data
        let remaining_length = self.end - self.position;

        self.candidates_mask = unsafe {
            Self::build_candidates::<true>(
                self.data.as_ptr().with_addr(self.position),
                remaining_length,
                self.pattern,
            )
        };
    }

    fn end_search(&mut self) -> Option<<Self as Iterator>::Item> {
        if let Some(position) = unsafe { self.consume_candidates::<true>() } {
            return Some(position);
        }
        if self.position + BYTES < self.end {
            self.position += BYTES;
            self.end_candidates();
        }

        unsafe { self.consume_candidates::<true>() }
    }
}

impl<'pattern, 'data, const ALIGNMENT: usize, const BYTES: usize> Iterator
    for Scanner<'pattern, 'data, ALIGNMENT, BYTES>
where
    LaneCount<ALIGNMENT>: SupportedLaneCount,
    LaneCount<BYTES>: SupportedLaneCount,
{
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        // In case of removing this, make sure self.position is not unconditionally
        // increased to prevent violating FusedIterator guarantees
        if self.exhausted {
            return self.end_search();
        }

        loop {
            if let Some(position) = unsafe { self.consume_candidates::<false>() } {
                #[cold]
                fn ret(pos: usize) -> Option<usize> {
                    Some(pos)
                }

                return ret(position);
            }

            // candidates are 0, check next chunk
            //
            // # Safety
            // It's near impossible to get close to address usize::max in the real
            // world, allowing to assume that self.position doesn't overflow.
            // This is checked using a debug_assert during init
            //
            // It is okay to unconditionally increase self.position because there is a short
            // circuit at the start of this function. Removing that short circuit will
            // violate FusedIterator guarantees
            self.position += BYTES;
            prefetch(self.data.as_ptr().with_addr(self.position));
            // check if the next 2 chunks are fully within bounds
            if self.position + 2 * BYTES >= self.end {
                #[cold]
                fn branch<'pattern, 'data, const ALIGNMENT: usize, const BYTES: usize>(
                    scanner: &mut Scanner<'pattern, 'data, ALIGNMENT, BYTES>,
                ) -> Option<usize>
                where
                    LaneCount<ALIGNMENT>: SupportedLaneCount,
                    LaneCount<BYTES>: SupportedLaneCount,
                {
                    scanner.exhausted = true;
                    scanner.candidates_mask = unsafe {
                        Scanner::<'pattern, 'data, ALIGNMENT, BYTES>::build_candidates::<false>(
                            scanner.data.as_ptr().with_addr(scanner.position),
                            BYTES,
                            scanner.pattern,
                        )
                    };

                    scanner.end_search()
                }

                return branch(self);
            }

            // # Safety
            // self.position was initialized to be aligned to BYTES, is only ever
            // increased in steps of BYTES, and self.position + BYTES is still within bounds
            // of self.data
            self.candidates_mask = unsafe {
                Self::build_candidates::<false>(
                    self.data.as_ptr().with_addr(self.position),
                    BYTES,
                    self.pattern,
                )
            };
        }
    }
}

impl<'pattern, 'data, const ALIGNMENT: usize, const BYTES: usize> FusedIterator
    for Scanner<'pattern, 'data, ALIGNMENT, BYTES>
where
    LaneCount<ALIGNMENT>: SupportedLaneCount,
    LaneCount<BYTES>: SupportedLaneCount,
{
}

impl<'pattern, 'data, const ALIGNMENT: usize, const BYTES: usize>
    Scanner<'pattern, 'data, ALIGNMENT, BYTES>
where
    LaneCount<ALIGNMENT>: SupportedLaneCount,
    LaneCount<BYTES>: SupportedLaneCount,
{
    /// if `UNALIGNED == false`, then the data pointer must be aligned to
    /// [`BYTES`] and `data + BYTES <= self.end`
    ///
    /// `data` must always be aligned to `ALIGNMENT`!
    #[inline]
    #[must_use]
    unsafe fn build_candidates<const UNALIGNED: bool>(
        data: *const u8,
        len: usize,
        pattern: &Pattern<ALIGNMENT, BYTES>,
    ) -> BytesMask {
        prefetch(data);
        prefetch((&raw const pattern.first_bytes).cast());
        if ALIGNMENT > 1 {
            prefetch((&raw const pattern.first_bytes_mask).cast());
        }
        let len_mask = Self::data_len_mask(len);
        // UNALIGNED is the first parameter on purpose
        // build_candidates is either called fully aligned or at the start or end
        // of the data slice. a full safe read is required when operating near edges
        let data = unsafe { Self::load::<UNALIGNED, false>(data, len_mask) };

        let mut search = data.simd_eq(pattern.first_bytes);
        if ALIGNMENT > 1 {
            search = search.bitor(pattern.first_bytes_mask);
        }
        let mut result = search.to_bitmask();

        if UNALIGNED {
            let mask =
                Self::mask_min_len(len_mask.to_bitmask(), pattern.first_bytes_mask.to_bitmask());
            result &= mask;
        }

        Self::reduce_bitmask(result)
    }

    /// This function guarantees:
    /// - only `self.candidates_mask` is modified
    /// - if `SAFE_READ == true`, then all bytes read are `>=self.position` and
    ///   `<=self.end`
    ///
    /// This function requires:
    /// - `self.position + candidate_offset` to be within bounds
    // This function is part of the hot loop. There is probably
    // a lot of potential for optimization still in here
    #[inline]
    unsafe fn consume_candidates<const SAFE_READ: bool>(
        &mut self,
    ) -> Option<<Self as Iterator>::Item> {
        let base_position = self.position - self.pattern.first_byte_offset as usize;
        if !SAFE_READ {
            prefetch(self.data.as_ptr().with_addr(base_position + BYTES));
        }
        prefetch((&raw const self.pattern.bytes).cast());
        prefetch((&raw const self.pattern.mask).cast());
        loop {
            if self.candidates_mask == 0 {
                return None;
            }

            let offset = self.candidates_mask.trailing_zeros() as usize;
            self.candidates_mask ^= 1 << offset;

            let offset_ptr = base_position + offset;
            let data_ptr = self.data.as_ptr().with_addr(offset_ptr);
            prefetch(data_ptr);
            // initial_candidates includes a bounds check at candidates creation
            // subsequent candidate creations cannot underflow
            debug_assert_opt!(offset_ptr >= self.data.as_ptr().addr());
            // # Safety
            // self.position is initialized from self.data
            let position = offset_ptr - self.data.as_ptr().addr();

            let len = self.data.len() - position;
            if SAFE_READ && len < self.pattern.length as usize {
                return None;
            }
            let data_len_mask = Self::data_len_mask(len);
            let data = unsafe { Self::load::<SAFE_READ, true>(data_ptr, data_len_mask) };

            let mut result = data.simd_eq(self.pattern.bytes).bitand(self.pattern.mask);

            if SAFE_READ {
                result &= data_len_mask;
            }

            if result == self.pattern.mask {
                return Some(position);
            }
        }
    }

    /// data_len_mask must be generated using [`Self::data_len_mask`]
    ///
    /// if `UNALIGNED == false`, then the data pointer must be aligned to
    /// [`BYTES`]
    #[inline]
    unsafe fn load<const SAFE_READ: bool, const UNALIGNED: bool>(
        data: *const u8,
        data_len_mask: Mask<i8, BYTES>,
    ) -> Simd<u8, BYTES> {
        if SAFE_READ {
            // # Safety
            // data_len_mask ensures that only valid bytes are read
            Simd::<u8, BYTES>::load_select_ptr(data, data_len_mask, Default::default())
        } else if UNALIGNED {
            core::ptr::read_unaligned(data as *const _)
        } else {
            *(data as *const _)
        }
    }
}

/// prefetches aligned 64 bytes containing the given address to L1 cache
#[inline(always)]
fn prefetch(ptr: *const u8) {
    #[cfg(target_arch = "x86")]
    use core::arch::x86 as arch;
    #[cfg(target_arch = "x86_64")]
    use core::arch::x86_64 as arch;
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    unsafe {
        arch::_mm_prefetch::<{ arch::_MM_HINT_T0 }>(ptr as _)
    }
}

#[cfg(test)]
mod tests {
    use core::slice;

    use super::*;

    const PATTERN: &str = "? ? ? 46 41 ? 54";
    const BYTES: usize = 64;

    mod candidates {
        use super::*;

        static DATA: Simd<u8, 64> = Simd::from_array([
            0, 0, 0, 0, 0, 0, 0, 0x46, 0x41, 0x53, 0x54, 0x46, 0x41, 0, 0, 0, 0, 0, 0x46, 0, 0, 0,
            0, 0, 0x46, 0x41, 0x53, 0x54, 0x46, 0x41, 0, 0, 0, 0, 0, 0x46, 0, 0, 0, 0, 0, 0, 0x46,
            0x41, 0x53, 0x54, 0x46, 0x41, 0, 0, 0, 0, 0, 0x46, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x46,
        ]);

        #[test]
        fn initial_candidates_1() {
            const ALIGNMENT: usize = 1;
            let pattern = Pattern::<ALIGNMENT, BYTES>::new(PATTERN);
            let data = &DATA[3..];
            let offset =
                Scanner::<ALIGNMENT, BYTES>::first_offset(data.as_ptr(), pattern.first_byte_offset);
            // DATA is BYTES aligned, which means that this value should never change
            assert_eq!(offset % BYTES, BYTES - 3);
            let result = Scanner::initial_candidates(&pattern, data, offset);

            let control: BytesMask =
                0b1000_0000_0010_0000_0100_0100_0000_1000_0001_0001_0000_0100_0000_1000_1000_0000;
            let control = control >> ((offset / BYTES) * BYTES) & (u64::MAX >> (64 - BYTES));

            assert_eq!(result, control);
        }

        #[test]
        fn initial_candidates_2() {
            const ALIGNMENT: usize = 2;
            let pattern = Pattern::<ALIGNMENT, BYTES>::new(PATTERN);
            let data = &DATA[3..];
            let offset =
                Scanner::<ALIGNMENT, BYTES>::first_offset(data.as_ptr(), pattern.first_byte_offset);
            // DATA is BYTES aligned, which means that this value should never change
            assert_eq!(offset % BYTES, BYTES - 3);
            let result = Scanner::initial_candidates(&pattern, data, offset);

            let control: BytesMask =
                0b0100_0000_0001_0000_0000_0000_0000_0100_0000_0000_0000_0000_0000_0100_0100_0000;
            let control = control >> ((offset / BYTES) * BYTES) & (u64::MAX >> (64 - BYTES));

            assert_eq!(result, control);
        }

        #[test]
        fn initial_candidates_4() {
            const ALIGNMENT: usize = 4;
            let pattern = Pattern::<ALIGNMENT, BYTES>::new(PATTERN);
            let data = &DATA[3..];
            let offset =
                Scanner::<ALIGNMENT, BYTES>::first_offset(data.as_ptr(), pattern.first_byte_offset);
            // DATA is BYTES aligned, which means that this value should never change
            assert_eq!(offset % BYTES, BYTES - 3);
            let result = Scanner::initial_candidates(&pattern, data, offset);

            let control: BytesMask =
                0b0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0001_0000_0000;
            let control = control >> ((offset / BYTES) * BYTES) & (u64::MAX >> (64 - BYTES));

            assert_eq!(result, control);
        }

        #[test]
        fn initial_candidates_8() {
            const ALIGNMENT: usize = 8;
            let pattern = Pattern::<ALIGNMENT, BYTES>::new(PATTERN);
            let data = &DATA[3..];
            let offset =
                Scanner::<ALIGNMENT, BYTES>::first_offset(data.as_ptr(), pattern.first_byte_offset);
            // DATA is BYTES aligned, which means that this value should never change
            assert_eq!(offset % BYTES, BYTES - 3);
            let result = Scanner::initial_candidates(&pattern, data, offset);

            let control: BytesMask = 0;
            let control = control >> ((offset / BYTES) * BYTES) & (u64::MAX >> (64 - BYTES));

            assert_eq!(result, control);
        }
    }

    mod regressions {
        use super::*;

        #[test]
        fn second_chunk_last_byte() {
            let mut data: [Simd<u8, BYTES>; 2] = Default::default();
            let data =
                unsafe { slice::from_raw_parts_mut(data.as_mut_ptr() as *mut u8, 2 * BYTES) };
            data[data.len() - 1] = 1;
            let pattern = Pattern::<1, BYTES>::new("01");
            let mut iter = pattern.matches(data);
            assert_eq!(iter.next().unwrap(), data.len() - 1);
            assert!(iter.next().is_none());
        }

        #[test]
        fn byte_offset_in_consume_candidates() {
            let mut data: [Simd<u8, BYTES>; 2] = Default::default();
            let data =
                unsafe { slice::from_raw_parts_mut(data.as_mut_ptr() as *mut u8, 2 * BYTES) };
            data[1] = 1;
            let pattern = Pattern::<1, BYTES>::new("?? 01");
            let mut iter = pattern.matches(data);
            assert_eq!(iter.next().unwrap(), 0);
            assert!(iter.next().is_none());
        }

        #[test]
        fn byte_offset_out_of_bounds_read() {
            let mut data: [Simd<u8, BYTES>; 2] = Default::default();
            let data =
                unsafe { slice::from_raw_parts_mut(data.as_mut_ptr() as *mut u8, 2 * BYTES) };
            data[0] = 1;
            let pattern = Pattern::<1, BYTES>::new("?? 01");
            let mut iter = pattern.matches(data);
            assert!(iter.next().is_none());
        }

        #[test]
        fn trailing_wildcard_at_eof() {
            let mut data: [Simd<u8, BYTES>; 2] = Default::default();
            let data =
                unsafe { slice::from_raw_parts_mut(data.as_mut_ptr() as *mut u8, 2 * BYTES) };
            data[data.len() - 1] = 1;
            let pattern = Pattern::<1, BYTES>::new("01 ??");
            let mut iter = pattern.matches(data);
            assert!(iter.next().is_none());
        }

        #[test]
        fn leading_wildcard_underflow() {
            let mut data: [Simd<u8, BYTES>; 2] = Default::default();
            let data =
                unsafe { slice::from_raw_parts_mut(data.as_mut_ptr() as *mut u8, 2 * BYTES) };
            data[BYTES] = 1;
            let pattern = Pattern::<1, BYTES>::new("? ? 01");
            let mut iter = pattern.matches(&data[BYTES - 1..BYTES + BYTES / 10]);
            assert!(iter.next().is_none());
        }

        #[test]
        fn leading_wildcard_boundary() {
            let mut data: [Simd<u8, BYTES>; 2] = Default::default();
            let data =
                unsafe { slice::from_raw_parts_mut(data.as_mut_ptr() as *mut u8, 2 * BYTES) };
            data[BYTES] = 1;
            let pattern = Pattern::<1, BYTES>::new("? 01");
            let mut iter = pattern.matches(&data[BYTES - 1..BYTES + BYTES / 7]);
            assert_eq!(iter.next().unwrap(), 0);
            assert!(iter.next().is_none());
        }

        #[test]
        fn pattern_gt_data() {
            let data = &[1];
            let pattern = Pattern::<1, BYTES>::new("? 01");
            let mut iter = pattern.matches(data);
            assert!(iter.next().is_none());
        }

        #[test]
        fn pattern_lt_alignment() {
            let mut data: [Simd<u8, BYTES>; 2] = Default::default();
            let data =
                unsafe { slice::from_raw_parts_mut(data.as_mut_ptr() as *mut u8, 2 * BYTES) };
            let src = &[0u8, 0x05, 0xff, 0xf7, 0x00];
            unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), data.as_mut_ptr(), src.len()) }
            let pat = Pattern::<2, BYTES>::new("00");
            let mut iter = pat.matches(&data[1..src.len()]);
            assert_eq!(iter.next().unwrap(), 3);
            assert!(iter.next().is_none());
        }

        #[test]
        fn max_wildcard_prefix() {
            let mut data: [Simd<u8, BYTES>; 2] = Default::default();
            let data =
                unsafe { slice::from_raw_parts_mut(data.as_mut_ptr() as *mut u8, 2 * BYTES) };
            data[data.len() - 1 - BYTES] = 1;
            data[data.len() - 1] = 1;
            let pattern = "? ".repeat(BYTES - 1) + "01";
            let pattern = Pattern::<1, BYTES>::new(&pattern);
            let mut iter = pattern.matches(data);
            assert_eq!(iter.next().unwrap(), 0);
            assert_eq!(iter.next().unwrap(), data.len() - BYTES);
            assert!(iter.next().is_none());
        }

        #[test]
        fn alignment_first_possible_eq_data() {
            let pat = Pattern::<2, BYTES>::new("? ? 01");
            let mut data: [Simd<u8, BYTES>; 2] = Default::default();
            let data =
                unsafe { slice::from_raw_parts_mut(data.as_mut_ptr() as *mut u8, 2 * BYTES) };
            let mut iter = pat.matches(&data[BYTES - 1..BYTES + 2]);
            assert!(iter.next().is_none());
        }

        #[test]
        fn leading_wildcards_match_start_to_end() {
            let pat = Pattern::<2, BYTES>::new("? ? ? ? 00");
            let mut data: [Simd<u8, BYTES>; 2] = Default::default();
            let data =
                unsafe { slice::from_raw_parts_mut(data.as_mut_ptr() as *mut u8, 2 * BYTES) };
            let mut iter = pat.matches(&data[10..15]);
            assert_eq!(iter.next().unwrap(), 0);
            assert!(iter.next().is_none());
        }
    }
}
