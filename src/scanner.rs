use core::{
    cmp::min,
    iter::FusedIterator,
    ops::{BitAnd, BitOr},
    simd::{cmp::SimdPartialEq, LaneCount, Mask, Simd, SupportedLaneCount},
};

use crate::{BytesMask, Pattern, BYTES};

/// An [`Iterator`] for searching a given [`Pattern`] in data
#[must_use]
pub struct Scanner<'pattern, 'data, const ALIGNMENT: usize>
where
    LaneCount<ALIGNMENT>: SupportedLaneCount,
{
    /// needle
    pattern: &'pattern Pattern<ALIGNMENT>,
    /// one bit for each byte in [`BYTES`]
    /// little endian least significant bit corresponds to the first byte in the
    /// current slice of data
    candidates_mask: BytesMask,
    /// pointer to first valid byte of data
    data: &'data [u8],
    /// pointer to one byte past the end of data
    end: *const u8,
    /// iterator position
    position: *const u8,
    /// indicates that `self.position + BYTES > self.end`
    exhausted: bool,
}

impl<'pattern, 'data, const ALIGNMENT: usize> Scanner<'pattern, 'data, ALIGNMENT>
where
    LaneCount<ALIGNMENT>: SupportedLaneCount,
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
    pub fn new(pattern: &'pattern Pattern<ALIGNMENT>, data: &'data [u8]) -> Self {
        let _aligned = Self::_ALIGNED;
        debug_assert!(data.len() <= usize::MAX - BYTES);
        debug_assert!(!data.is_empty());
        debug_assert!(((&data[data.len() - 1]) as *const u8 as usize) <= usize::MAX - 3 * BYTES);

        // data + align_offset required to align to BYTES
        let mut align_offset = data.as_ptr().align_offset(align_of::<Simd<u8, BYTES>>());
        if align_offset == 0 {
            align_offset = BYTES;
        }
        let data_align = align_offset % ALIGNMENT;
        let first_possible = data_align + pattern.first_byte_offset as usize;
        if align_offset <= first_possible {
            align_offset += BYTES;
        }
        let candidates_mask = Self::initial_candidates(pattern, data, align_offset);

        // set position out of bounds.
        // next() will use it as base for candidates offsets,
        // then increment by BYTES to search for new candidates,
        // increasing position to be in bounds again.
        // exception: align_offset > data.len()
        // this will be checked before searching for new candidates
        // # Safety
        // it is assumed that data.as_ptr() - BYTES doesn't underflow
        let position = data
            .as_ptr()
            .wrapping_add(align_offset)
            .wrapping_offset(-(BYTES as isize));

        let end = unsafe { data.as_ptr().add(data.len()) };

        Self {
            pattern,
            data,
            end,
            position,
            candidates_mask,
            exhausted: position.wrapping_add(2 * BYTES) >= end,
        }
    }

    #[inline]
    fn initial_candidates(
        pattern: &Pattern<ALIGNMENT>,
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
        debug_assert!(first_possible < max_offset);

        // compute the first candidates
        let result = unsafe {
            Self::build_candidates::<true>(
                &data[first_possible],
                max_offset - first_possible,
                pattern,
            )
        };

        // shift result to align to end of currently aligned (out of bounds starting)
        // slice
        result << (BYTES + first_possible - align_offset)
    }

    fn end_candidates(&mut self) {
        // # Safety
        // self.end and self.position are both initialized from self.data
        let remaining_length = unsafe { self.end.offset_from(self.position) };
        debug_assert!(remaining_length >= 0);
        let remaining_length = remaining_length as usize;

        self.candidates_mask = unsafe {
            Self::build_candidates::<true>(self.position, remaining_length, self.pattern)
        };
    }

    fn end_search(&mut self) -> Option<<Self as Iterator>::Item> {
        if let Some(position) = unsafe { self.consume_candidates::<true>() } {
            return Some(position);
        }
        if self.position.wrapping_add(BYTES) < self.end {
            self.position = self.position.wrapping_add(BYTES);
            self.end_candidates();
        }

        unsafe { self.consume_candidates::<true>() }
    }
}

impl<'pattern, 'data, const ALIGNMENT: usize> Iterator for Scanner<'pattern, 'data, ALIGNMENT>
where
    LaneCount<ALIGNMENT>: SupportedLaneCount,
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
                return Some(position);
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
            self.position = self.position.wrapping_add(BYTES);
            // check if the next 2 chunks are fully within bounds
            if self.position.wrapping_add(2 * BYTES) >= self.end {
                self.exhausted = true;
                self.candidates_mask =
                    unsafe { Self::build_candidates::<false>(self.position, BYTES, self.pattern) };

                return self.end_search();
            }

            // # Safety
            // self.position was initialized to be aligned to BYTES, is only ever
            // increased in steps of BYTES, and self.position + BYTES is still within bounds
            // of self.data
            self.candidates_mask =
                unsafe { Self::build_candidates::<false>(self.position, BYTES, self.pattern) };
        }
    }
}

impl<'pattern, 'data, const ALIGNMENT: usize> FusedIterator for Scanner<'pattern, 'data, ALIGNMENT> where
    LaneCount<ALIGNMENT>: SupportedLaneCount
{
}

impl<'pattern, 'data, const ALIGNMENT: usize> Scanner<'pattern, 'data, ALIGNMENT>
where
    LaneCount<ALIGNMENT>: SupportedLaneCount,
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
        pattern: &Pattern<ALIGNMENT>,
    ) -> BytesMask {
        let len_mask = Self::data_len_mask(len);
        // UNALIGNED is the first parameter on purpose
        // build_candidates is either called fully aligned or at the start or end
        // of the data slice. a full safe read is required when operating near edges
        let data = unsafe { Self::load::<UNALIGNED, false>(data, len_mask) };

        let mut result = data
            .simd_eq(pattern.first_bytes)
            .bitor(pattern.first_bytes_mask)
            .to_bitmask();

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
    /// - `self.position` to be within bounds
    // This function is part of the hot loop. There is probably
    // a lot of potential for optimization still in here
    #[inline]
    unsafe fn consume_candidates<const SAFE_READ: bool>(
        &mut self,
    ) -> Option<<Self as Iterator>::Item> {
        loop {
            if self.candidates_mask == 0 {
                return None;
            }

            let offset = self.candidates_mask.trailing_zeros() as usize;
            self.candidates_mask ^= 1 << offset;

            let offset_ptr = self
                .position
                .wrapping_add(offset)
                .wrapping_sub(self.pattern.first_byte_offset as usize);
            // # Safety
            // self.position is initialized from self.data
            let position = unsafe { offset_ptr.offset_from(self.data.as_ptr()) };
            // initial_candidates includes a bounds check at candidates creation
            // subsequent candidate creations cannot underflow
            debug_assert!(position >= 0);
            let position = position as usize;

            let len = self.data.len() - position;
            if SAFE_READ && len < self.pattern.length as usize {
                return None;
            }
            let data_len_mask = Self::data_len_mask(len);
            let data = unsafe { Self::load::<SAFE_READ, true>(offset_ptr, data_len_mask) };

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

#[cfg(test)]
mod tests {
    use super::*;

    const PATTERN: &str = "? ? ? 46 41 ? 54";

    mod candidates {
        use super::*;

        static DATA: Simd<u8, BYTES> = Simd::from_array([
            0, 0, 0, 0, 0, 0, 0, 0x46, 0x41, 0x53, 0x54, 0x46, 0x41, 0, 0, 0, 0, 0, 0x46, 0, 0, 0,
            0, 0, 0x46, 0x41, 0x53, 0x54, 0x46, 0x41, 0, 0, 0, 0, 0, 0x46, 0, 0, 0, 0, 0, 0, 0x46,
            0x41, 0x53, 0x54, 0x46, 0x41, 0, 0, 0, 0, 0, 0x46, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x46,
        ]);

        #[test]
        fn initial_candidates_1() {
            const ALIGNMENT: usize = 1;
            let pattern = Pattern::<ALIGNMENT>::new(PATTERN);
            let data = &DATA[3..];
            let offset = data.as_ptr().align_offset(align_of::<Simd<u8, BYTES>>());
            // DATA is BYTES aligned, which means that this value should never change
            assert_eq!(offset, 61);
            let result = Scanner::initial_candidates(&pattern, data, offset);

            let control: BytesMask =
                0b1000_0000_0010_0000_0100_0100_0000_1000_0001_0001_0000_0100_0000_1000_1000_0000;

            assert_eq!(result, control);
        }

        #[test]
        fn initial_candidates_2() {
            const ALIGNMENT: usize = 2;
            let pattern = Pattern::<ALIGNMENT>::new(PATTERN);
            let data = &DATA[3..];
            let offset = data.as_ptr().align_offset(align_of::<Simd<u8, BYTES>>());
            // DATA is BYTES aligned, which means that this value should never change
            assert_eq!(offset, 61);
            let result = Scanner::initial_candidates(&pattern, data, offset);

            let control: BytesMask =
                0b0100_0000_0001_0000_0000_0000_0000_0100_0000_0000_0000_0000_0000_0100_0100_0000;

            assert_eq!(result, control);
        }

        #[test]
        fn initial_candidates_4() {
            const ALIGNMENT: usize = 4;
            let pattern = Pattern::<ALIGNMENT>::new(PATTERN);
            let data = &DATA[3..];
            let offset = data.as_ptr().align_offset(align_of::<Simd<u8, BYTES>>());
            // DATA is BYTES aligned, which means that this value should never change
            assert_eq!(offset, 61);
            let result = Scanner::initial_candidates(&pattern, data, offset);

            let control: BytesMask =
                0b0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0001_0000_0000;

            assert_eq!(result, control);
        }

        #[test]
        fn initial_candidates_8() {
            const ALIGNMENT: usize = 8;
            let pattern = Pattern::<ALIGNMENT>::new(PATTERN);
            let data = &DATA[3..];
            let offset = data.as_ptr().align_offset(align_of::<Simd<u8, BYTES>>());
            // DATA is BYTES aligned, which means that this value should never change
            assert_eq!(offset, 61);
            let result = Scanner::initial_candidates(&pattern, data, offset);

            let control: BytesMask = 0;

            assert_eq!(result, control);
        }
    }
}
