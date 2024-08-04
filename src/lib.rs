//! # Pattern matching library
//! Allows you to search for a pattern within data via an iterator interface.
//! This library uses the core::simd abstraction and is fully no_std compatible.
//!
//! ## Usage
//! ```
//! use patterns::Pattern;
//!
//! let data = [0_u8; 1_000_00];
//! // Allows . and ? as wildcard.
//! // Any number of wildcard characters between spaces is considered a wildcard byte.
//! let pattern: Pattern = "01 02 00 ? 59 ff".parse().unwrap();
//! let mut iterator = pattern.matches(&data);
//!
//! for _found in iterator {
//!     // use _found
//! }
//! ```
//!
//! ## Limitations
//! - The maximum amount of bytes supported inside a pattern are 64 bytes
//! - Target alignment of the pattern to search for must be less or equal to 64
//! - The pointer of data to search through must follow these invariants:
//!   - `data.as_ptr() - 64 > `[`usize::MIN`]
//!   - `data.as_ptr() + data.len() + 64 < `[`usize::MAX`]

// todos
// optimize pattern.len() <= alignment

#![feature(portable_simd)]
#![no_std]
// untested on big endian
#![cfg(target_endian = "little")]

use core::{
    cmp::min,
    iter::FusedIterator,
    ops::{BitAnd, BitOr, Not},
    simd::{
        cmp::{SimdPartialEq, SimdPartialOrd},
        LaneCount, Mask, Simd, SupportedLaneCount,
    },
};

use crate::pattern::Pattern;

pub mod pattern;
mod utils;

/// Determines the LANES size.
/// Every block of data is processed in chunks of `BYTES` bytes.
/// Rust will compile this to other targets without issue, but will use inner
/// loops for that.
pub const BYTES: usize = 64;
/// The type that holds a bit for each byte in [`BYTES`]
pub type BytesMask = u64;

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
    /// pointer to last valid byte of data
    end: &'data u8,
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
        debug_assert!(((&data[data.len() - 1]) as *const u8 as usize) < usize::MAX - BYTES);

        // data + align_offset required to align to BYTES
        let align_offset = data
            .as_ptr()
            .align_offset(core::mem::align_of::<Simd<u8, BYTES>>());
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

        Self {
            pattern,
            data,
            end: &data[data.len() - 1],
            position,
            candidates_mask,
            // FIXME: check that this is correct, including with unaligned data
            exhausted: data.len() < BYTES,
        }
    }

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
        let first_possible = data_align + pattern.first_byte_offset;
        let max_offset = min(align_offset, data.len());
        if first_possible >= max_offset {
            return 0;
        }

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
        result << (BYTES - align_offset + data_align)
    }

    fn end_candidates(&mut self) {
        // # Safety
        // self.end and self.position are both initialized from self.data
        let remaining_length =
            unsafe { (self.end as *const u8).offset_from(self.position) } as usize;

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
            if self.position.wrapping_add(2 * BYTES) > self.end {
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
    /// filters the bitmask to valid chunks, little endian least significant bit
    /// remains set
    ///
    /// ```text
    /// ALIGNMENT = 4
    /// start:  1111 1110 1101 1111
    /// result: 0001 0000 0000 0001
    /// ```
    #[inline]
    const fn reduce_bitmask(mut bitmask: BytesMask) -> BytesMask {
        let mut shift = 1;
        while shift < ALIGNMENT {
            bitmask &= bitmask >> shift;
            shift <<= 1;
        }

        const fn mask<const ALIGNMENT: usize>() -> BytesMask {
            let pattern = 1;
            let mut mask = 0;
            let mut i = 0;
            while i < BYTES / ALIGNMENT {
                mask |= pattern << (ALIGNMENT * i);
                i += 1;
            }
            mask
        }

        bitmask & mask::<ALIGNMENT>()
    }

    /// if `UNALIGNED == false`, then the data pointer must be aligned to
    /// [`BYTES`] and `data + BYTES <= self.end`
    #[inline]
    #[must_use]
    unsafe fn build_candidates<const UNALIGNED: bool>(
        data: *const u8,
        len: usize,
        pattern: &Pattern<ALIGNMENT>,
    ) -> BytesMask {
        let mask = Self::data_len_mask(len);
        let data = unsafe { Self::load::<UNALIGNED, false>(data, mask) };

        let mut result = data
            .simd_eq(pattern.first_bytes)
            .bitor(pattern.first_bytes_mask);

        if UNALIGNED {
            result = result.bitand(mask)
        }
        let result = result.to_bitmask();

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

            let offset_ptr = self.position.wrapping_add(offset);
            // # Safety
            // self.position is initialized from self.data
            // self.position is within bounds at this stage
            let position = unsafe { offset_ptr.offset_from(self.data.as_ptr()) } as usize;

            let data_len_mask = Self::data_len_mask(self.data.len() - position);
            let data = unsafe { Self::load::<SAFE_READ, true>(offset_ptr, data_len_mask) };

            let mut result = data.simd_eq(self.pattern.bytes).bitand(self.pattern.mask);

            if SAFE_READ {
                result = result.bitand(data_len_mask)
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
            let mut tmp = core::mem::MaybeUninit::<Simd<u8, BYTES>>::uninit();
            // # Safety
            // offset_ptr..(offset_ptr + BYTES) is within bounds of data
            unsafe {
                core::ptr::copy_nonoverlapping(data, tmp.as_mut_ptr().cast(), 1);
                tmp.assume_init()
            }
        } else {
            *(data as *const _)
        }
    }

    /// generates a mask that yields true until position `len`
    #[inline]
    fn data_len_mask(len: usize) -> Mask<i8, BYTES> {
        let len = len.min(BYTES);

        let mut index = [0u8; BYTES];
        index
            .iter_mut()
            .enumerate()
            .for_each(|(index, entry)| *entry = index as u8);
        let index = Simd::<u8, BYTES>::from_array(index);

        index.simd_lt(Simd::<u8, BYTES>::splat(len as u8))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATTERN: &str = "? ? ? 46 41 ? 54";

    mod candidates {
        use super::*;

        const MASK: BytesMask =
            0b1111_1110_1101_1011_0111_1100_1010_1001_0101_0011_0110_1000_0100_0010_0001_0000;

        #[test]
        fn reduce_align_1() {
            let reduced = Scanner::<'_, '_, 1>::reduce_bitmask(MASK);
            let control = MASK;

            assert_eq!(reduced, control);
        }

        #[test]
        fn reduce_align_2() {
            let reduced = Scanner::<'_, '_, 2>::reduce_bitmask(MASK);
            let control =
                0b0101_0100_0100_0001_0001_0100_0000_0000_0000_0001_0000_0000_0000_0000_0000_0000;

            assert_eq!(reduced, control);
        }

        #[test]
        fn reduce_align_4() {
            let reduced = Scanner::<'_, '_, 4>::reduce_bitmask(MASK);
            let control = 1 << 60;

            assert_eq!(reduced, control);
        }

        #[test]
        fn reduce_align_8() {
            let reduced = Scanner::<'_, '_, 8>::reduce_bitmask(MASK);
            let control = 0;

            assert_eq!(reduced, control);
        }

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
            let offset = data
                .as_ptr()
                .align_offset(core::mem::align_of::<Simd<u8, BYTES>>());
            // DATA is BYTES aligned, which means that this value should never change
            assert_eq!(offset, 61);
            let result = Scanner::initial_candidates(&pattern, data, offset);

            let control: BytesMask =
                0b0001_0000_0000_0100_0000_1000_1000_0001_0000_0010_0010_0000_1000_0001_0001_0000;

            assert_eq!(result, control);
        }

        #[test]
        fn initial_candidates_2() {
            const ALIGNMENT: usize = 2;
            let pattern = Pattern::<ALIGNMENT>::new(PATTERN);
            let data = &DATA[3..];
            let offset = data
                .as_ptr()
                .align_offset(core::mem::align_of::<Simd<u8, BYTES>>());
            // DATA is BYTES aligned, which means that this value should never change
            assert_eq!(offset, 61);
            let result = Scanner::initial_candidates(&pattern, data, offset);

            let control: BytesMask =
                0b0001_0000_0000_0100_0000_0000_0000_0001_0000_0000_0000_0000_0000_0001_0001_0000;

            assert_eq!(result, control);
        }

        #[test]
        fn initial_candidates_4() {
            const ALIGNMENT: usize = 4;
            let pattern = Pattern::<ALIGNMENT>::new(PATTERN);
            let data = &DATA[3..];
            let offset = data
                .as_ptr()
                .align_offset(core::mem::align_of::<Simd<u8, BYTES>>());
            // DATA is BYTES aligned, which means that this value should never change
            assert_eq!(offset, 61);
            let result = Scanner::initial_candidates(&pattern, data, offset);

            let control: BytesMask =
                0b0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0001_0000;

            assert_eq!(result, control);
        }

        #[test]
        fn initial_candidates_8() {
            const ALIGNMENT: usize = 8;
            let pattern = Pattern::<ALIGNMENT>::new(PATTERN);
            let data = &DATA[3..];
            let offset = data
                .as_ptr()
                .align_offset(core::mem::align_of::<Simd<u8, BYTES>>());
            // DATA is BYTES aligned, which means that this value should never change
            assert_eq!(offset, 61);
            let result = Scanner::initial_candidates(&pattern, data, offset);

            let control: BytesMask = 0;

            assert_eq!(result, control);
        }
    }
}

/*
fn find_in_buffer(pattern: &Pattern, data: &[u8], cursor: &mut &[u8]) -> Option<usize> {
    loop {
        if cursor.len() < BYTES + pattern.wildcard_prefix {
            break None;
        }

        // We can skip bytes that are wildcards.
        let search = Simd::from_slice(&cursor[pattern.wildcard_prefix..]);
        // Look for the first non wildcard byte.
        let first_byte = search.simd_eq(pattern.first_byte).to_bitmask();

        // If no match was found, shift by the amount of bytes we check at once and
        // start over.
        if first_byte == 0 {
            *cursor = &cursor[BYTES..];
            continue;
        }
        // ... else shift the cursor to match the first match.
        *cursor = &cursor[first_byte.trailing_zeros() as usize..];

        if cursor.len() < BYTES {
            break None;
        }

        let search = Simd::from_slice(cursor);
        // Check `BYTES` amount of bytes at the same time.
        let result = search.simd_eq(pattern.bytes);
        // Filter out results we are not interested in.
        let filtered_result = result.bitand(pattern.mask);
        // Save the position within data.
        // Safety: This is fine because we make sure that cursor always points to data
        let index = unsafe { cursor.as_ptr().offset_from(data.as_ptr()) }; // Shift the cursor by one to not check the same data again.
        *cursor = &cursor[1..];
        // Perform an equality check on all registers of the final result.
        // Essentially this boils down to `result & mask == mask`
        if filtered_result == pattern.mask {
            return Some(index as usize);
        }
    }
}
*/
