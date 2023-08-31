//! # Pattern matching library
//! Allows you to search for a pattern within data via an iterator interface.
//! This library uses the core::simd abstraction and does not allocate.
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

#![feature(portable_simd)]
#![no_std]

use core::{
    cmp::min,
    num::ParseIntError,
    ops::BitAnd,
    simd::{Mask, Simd, SimdPartialEq, ToBitMask},
    str::FromStr,
};

/// Determines the LANES size. i.e.: register size;
/// Every block of data is processed in chunks of `BYTES` bytes.
pub const BYTES: usize = 64;

enum ScannerState {
    PreAlign(usize),
    Simd(u64),
    Tail,
    End,
}

/// An iterator for searching a given pattern in data
pub struct Scanner<'pattern, 'data: 'cursor, 'cursor> {
    pattern: &'pattern Pattern,
    data: &'data [u8],
    cursor: &'cursor [u8],
    state: ScannerState,
}

impl<'pattern, 'data: 'cursor, 'cursor> Scanner<'pattern, 'data, 'cursor> {
    /// Create an iterator, also see [`Pattern::matches`]
    #[must_use]
    #[inline]
    pub fn new(pattern: &'pattern Pattern, data: &'data [u8]) -> Scanner<'pattern, 'data, 'cursor> {
        let align = data.as_ptr().align_offset(BYTES);
        let align = min(align, BYTES); // by contract, align_offset may return usize::MAX
        Scanner {
            pattern,
            data,
            cursor: data,
            state: if align != 0 {
                ScannerState::PreAlign(align)
            } else {
                ScannerState::Simd(0)
            },
        }
    }
}

/// Match `pattern` against the start of `data` (without SIMD)
#[inline(always)]
fn plain_match(pattern: &Pattern, data: &[u8]) -> bool {
    pattern.bytes.as_array()[0..pattern.length]
        .iter()
        .zip(pattern.mask.to_array()[0..pattern.length].iter())
        .zip(data[0..pattern.length].iter())
        .all(|((pat, mask), val)| (!*mask) || *pat == *val)
}

/// Find the offset of `cursor` into `data`.
///
/// # Safety
/// Assumes that `cursor` is derived from `data` as in `data[K..]`
#[inline(always)]
fn cursor_offset(cursor: &&[u8], data: &[u8]) -> usize {
    (unsafe { cursor.as_ptr().offset_from(data.as_ptr()) } as usize)
}

/// Search for `pattern` in `data`, starting from `cursor` (without SIMD)
///
/// The `limit` parameter is an upper bound on the number of iterations
/// i.e. how many bytes of `data` are searched *for the first byte* of `pattern`
#[inline]
fn plain_search(
    pattern: &Pattern,
    data: &[u8],
    cursor: &mut &[u8],
    limit: &mut usize,
) -> Option<usize> {
    while *limit > 0 && cursor.len() >= pattern.length {
        if cursor[0] == pattern.first_byte[0] {
            #[cfg(feature = "second_byte")]
            if cursor[pattern.second_byte_offset] != pattern.second_byte[0] {
                continue;
            }
            // subtract wraps if matched too early -- wildcard prefix is before the start
            if let Some(index) = cursor_offset(cursor, data).checked_sub(pattern.wildcard_prefix) {
                // non-SIMD pattern comparison
                if plain_match(pattern, cursor) {
                    *cursor = &cursor[1..];
                    *limit -= 1;
                    return Some(index);
                }
            };
        }

        *cursor = &cursor[1..];
        *limit -= 1;
    }

    None
}

impl<'pattern, 'data: 'cursor, 'cursor> Iterator for Scanner<'pattern, 'data, 'cursor> {
    type Item = usize;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match &mut self.state {
                ScannerState::PreAlign(limit) => {
                    if let Some(index) =
                        plain_search(self.pattern, self.data, &mut self.cursor, limit)
                    {
                        return Some(index);
                    }

                    if self.cursor.len() < self.pattern.length {
                        self.state = ScannerState::End;
                    } else {
                        self.state = ScannerState::Simd(0);
                    }
                }
                ScannerState::Simd(first_byte) => {
                    if let Some(index) =
                        find_in_buffer(self.pattern, self.data, &mut self.cursor, first_byte)
                    {
                        return Some(index);
                    }

                    self.state = ScannerState::Tail;
                }
                ScannerState::Tail => {
                    let mut limit: usize = usize::MAX;
                    if let Some(index) =
                        plain_search(self.pattern, self.data, &mut self.cursor, &mut limit)
                    {
                        return Some(index);
                    }

                    self.state = ScannerState::End;
                }
                ScannerState::End => return None,
            }
        }
    }
}

fn find_in_buffer(
    pattern: &Pattern,
    data: &[u8],
    cursor: &mut &[u8],
    first_byte: &mut u64,
) -> Option<usize> {
    loop {
        if *first_byte == 0 {
            if cursor.len() < BYTES {
                break None;
            }

            let search = Simd::from_slice(cursor);
            // Look for the first non wildcard byte.
            *first_byte = search.simd_eq(pattern.first_byte).to_bitmask();

            #[cfg(feature = "second_byte")]
            if pattern.second_byte_offset != 0
                && cursor.len() - pattern.second_byte_offset >= BYTES
                && *first_byte != 0
            {
                let search2 = Simd::from_slice(&cursor[pattern.second_byte_offset..]);
                let second_byte = search2.simd_eq(pattern.second_byte).to_bitmask();
                *first_byte &= second_byte;
            }
        }

        while *first_byte != 0 {
            let offset = first_byte.trailing_zeros() as usize;

            // Shift the cursor to not check the same data again.
            *first_byte &= !(1 << offset);

            if cursor.len() - offset < BYTES {
                // Make sure we don't repeat values matched earlier in this SIMD slice
                // i.e. during previous iterations of this while loop
                *cursor = &cursor[offset..];
                return None;
            }

            // Save the position within data.
            let Some(index) =
                (cursor_offset(cursor, data) + offset).checked_sub(pattern.wildcard_prefix)
            else {
                // matched too early -- wildcard prefix is before the start
                continue;
            };

            let search = Simd::from_slice(&cursor[offset..]);
            // Check `BYTES` amount of bytes at the same time.
            let result = search.simd_eq(pattern.bytes);
            // Filter out results we are not interested in.
            let filtered_result = result.bitand(pattern.mask);

            // Perform an equality check on all registers of the final result.
            // Essentially this boils down to `result & mask == mask`
            if filtered_result == pattern.mask {
                if *first_byte == 0 {
                    *cursor = &cursor[BYTES..];
                }

                return Some(index);
            }
        }

        *cursor = &cursor[BYTES..];
    }
}

/// A prepared pattern
#[derive(Clone, Debug)]
pub struct Pattern {
    pub(crate) bytes: Simd<u8, BYTES>,
    pub(crate) mask: Mask<i8, BYTES>,
    pub(crate) first_byte: Simd<u8, BYTES>,
    #[cfg(feature = "second_byte")]
    pub(crate) second_byte: Simd<u8, BYTES>,
    #[cfg(feature = "second_byte")]
    pub(crate) second_byte_offset: usize,
    pub(crate) wildcard_prefix: usize,
    pub(crate) length: usize,
}

impl Pattern {
    /// Parse a pattern. Use the [`FromStr`] impl to return an error instead of
    /// panicking. # Panics
    /// Panics if [`ParsePatternError`] is returned.
    #[must_use]
    #[inline]
    pub fn new(pattern: &str) -> Self {
        pattern.parse().unwrap()
    }

    /// Creates an iterator through data.
    #[inline]
    #[must_use]
    pub fn matches<'pattern, 'data: 'cursor, 'cursor>(
        &'pattern self,
        data: &'data [u8],
    ) -> Scanner<'pattern, 'data, 'cursor> {
        Scanner::new(self, data)
    }
}

impl FromStr for Pattern {
    type Err = ParsePatternError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        /// allows . and ? as wildcard and only considers the first character
        fn is_wildcard(byte: &str) -> bool {
            const WILDCARD: u8 = b'.';
            byte.as_bytes()[0] & WILDCARD == WILDCARD
        }

        let bytes = s.split_ascii_whitespace();

        // count and skip over prefix wildcards
        let wildcard_prefix = bytes.clone().take_while(|x| is_wildcard(x)).count();
        let bytes = bytes.skip(wildcard_prefix);

        let length = bytes.clone().count();
        if length > BYTES {
            return Err(ParsePatternError::PatternTooLong);
        }

        let mut buffer = [0_u8; BYTES];
        let mut mask = [false; BYTES];

        for (index, byte) in bytes.enumerate() {
            if is_wildcard(byte) {
                continue;
            }
            buffer[index] = u8::from_str_radix(byte, 16)?;
            mask[index] = true;
        }

        // since prefix wildcards were skipped, the first byte must be non-wildcard
        if !mask[0] {
            return Err(ParsePatternError::MissingNonWildcardByte);
        }

        let first_byte = Simd::from_array([buffer[0]; BYTES]);

        #[cfg(feature = "second_byte")]
        let second_byte_offset = mask
            .iter()
            .skip(1)
            .position(|x| *x)
            .map(|x| x + 1)
            .unwrap_or(0);
        #[cfg(feature = "second_byte")]
        let second_byte = Simd::from_array([buffer[second_byte_offset]; BYTES]);

        Ok(Self {
            bytes: Simd::from_array(buffer),
            mask: Mask::from_array(mask),
            first_byte,
            #[cfg(feature = "second_byte")]
            second_byte,
            #[cfg(feature = "second_byte")]
            second_byte_offset,
            wildcard_prefix,
            length,
        })
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum ParsePatternError {
    PatternTooLong,
    InvalidHexNumber(ParseIntError),
    MissingNonWildcardByte,
}

impl From<ParseIntError> for ParsePatternError {
    #[inline]
    fn from(value: ParseIntError) -> Self {
        Self::InvalidHexNumber(value)
    }
}
