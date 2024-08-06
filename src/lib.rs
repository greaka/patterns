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
// explore getting rid of pattern.length

#![feature(portable_simd)]
#![no_std]
// untested on big endian
#![cfg(target_endian = "little")]

pub use crate::{pattern::Pattern, scanner::Scanner};

mod const_utils;
mod masks;
mod pattern;
mod scanner;

/// Determines the LANES size.
/// Every block of data is processed in chunks of `BYTES` bytes.
/// Rust will compile this to other targets without issue, but will use inner
/// loops for that.
pub const BYTES: usize = 64;
/// The type that holds a bit for each byte in [`BYTES`]
pub type BytesMask = u64;

#[cfg(test)]
mod tests {
    use core::{simd::Simd, slice};

    use super::*;

    mod regressions {

        use super::*;

        #[test]
        fn second_chunk_last_byte() {
            let mut data: [Simd<u8, BYTES>; 2] = [Default::default(); 2];
            let data =
                unsafe { slice::from_raw_parts_mut(data.as_mut_ptr() as *mut u8, 2 * BYTES) };
            data[data.len() - 1] = 1;
            let pattern = Pattern::<1>::new("01");
            let mut iter = pattern.matches(data);
            assert_eq!(iter.next().unwrap(), data.len() - 1);
            assert!(iter.next().is_none());
        }

        #[test]
        fn byte_offset_in_consume_candidates() {
            let mut data: [Simd<u8, BYTES>; 2] = [Default::default(); 2];
            let data =
                unsafe { slice::from_raw_parts_mut(data.as_mut_ptr() as *mut u8, 2 * BYTES) };
            data[1] = 1;
            let pattern = Pattern::<1>::new("?? 01");
            let mut iter = pattern.matches(data);
            assert_eq!(iter.next().unwrap(), 0);
            assert!(iter.next().is_none());
        }

        #[test]
        fn byte_offset_out_of_bounds_read() {
            let mut data: [Simd<u8, BYTES>; 2] = [Default::default(); 2];
            let data =
                unsafe { slice::from_raw_parts_mut(data.as_mut_ptr() as *mut u8, 2 * BYTES) };
            data[0] = 1;
            let pattern = Pattern::<1>::new("?? 01");
            let mut iter = pattern.matches(data);
            assert!(iter.next().is_none());
        }

        #[test]
        fn trailing_wildcard_at_eof() {
            let mut data: [Simd<u8, BYTES>; 2] = [Default::default(); 2];
            let data =
                unsafe { slice::from_raw_parts_mut(data.as_mut_ptr() as *mut u8, 2 * BYTES) };
            data[data.len() - 1] = 1;
            let pattern = Pattern::<1>::new("01 ??");
            let mut iter = pattern.matches(data);
            assert!(iter.next().is_none());
        }

        #[test]
        fn leading_wildcard_underflow() {
            let mut data: [Simd<u8, BYTES>; 2] = [Default::default(); 2];
            let data =
                unsafe { slice::from_raw_parts_mut(data.as_mut_ptr() as *mut u8, 2 * BYTES) };
            data[BYTES] = 1;
            let pattern = Pattern::<1>::new("? ? 01");
            let mut iter = pattern.matches(&data[BYTES - 1..BYTES + BYTES / 10]);
            assert!(iter.next().is_none());
        }

        #[test]
        fn leading_wildcard_boundary() {
            let mut data: [Simd<u8, BYTES>; 2] = [Default::default(); 2];
            let data =
                unsafe { slice::from_raw_parts_mut(data.as_mut_ptr() as *mut u8, 2 * BYTES) };
            data[BYTES] = 1;
            let pattern = Pattern::<1>::new("? 01");
            let mut iter = pattern.matches(&data[BYTES - 1..BYTES + BYTES / 10]);
            assert_eq!(iter.next().unwrap(), 0);
            assert!(iter.next().is_none());
        }

        #[test]
        fn pattern_gt_data() {
            let data = &[1];
            let pattern = Pattern::<1>::new("? 01");
            let mut iter = pattern.matches(data);
            assert!(iter.next().is_none());
        }

        #[test]
        fn pattern_lt_alignment() {
            let pat = Pattern::<2>::new("00");
            let data = &[0, 0x05, 0xff, 0xf7, 0x00];
            let mut iter = pat.matches(&data[1..]);
            assert_eq!(iter.next().unwrap(), 3);
            assert!(iter.next().is_none());
        }

        #[test]
        fn max_wildcard_prefix() {
            let mut data: [Simd<u8, BYTES>; 2] = [Default::default(); 2];
            let data =
                unsafe { slice::from_raw_parts_mut(data.as_mut_ptr() as *mut u8, 2 * BYTES) };
            data[data.len() - 1 - BYTES] = 1;
            data[data.len() - 1] = 1;
            let pattern = "? ".repeat(BYTES - 1) + "01";
            let pattern = Pattern::<1>::new(&pattern);
            let mut iter = pattern.matches(data);
            assert_eq!(iter.next().unwrap(), 0);
            assert_eq!(iter.next().unwrap(), data.len() - BYTES);
            assert!(iter.next().is_none());
        }

        #[test]
        fn alignment_first_possible_eq_data() {
            let pat = Pattern::<2>::new("? ? 01");
            let mut data: [Simd<u8, BYTES>; 2] = [Default::default(); 2];
            let data =
                unsafe { slice::from_raw_parts_mut(data.as_mut_ptr() as *mut u8, 2 * BYTES) };
            let mut iter = pat.matches(&data[BYTES - 1..BYTES + 2]);
            assert!(iter.next().is_none());
        }

        #[test]
        fn leading_wildcards_match_start_to_end() {
            let pat = Pattern::<2>::new("? ? ? ? 00");
            let mut data: [Simd<u8, BYTES>; 2] = [Default::default(); 2];
            let data =
                unsafe { slice::from_raw_parts_mut(data.as_mut_ptr() as *mut u8, 2 * BYTES) };
            let mut iter = pat.matches(&data[10..15]);
            assert_eq!(iter.next().unwrap(), 0);
            assert!(iter.next().is_none());
        }
    }
}
