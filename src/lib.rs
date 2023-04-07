#![feature(portable_simd)]
#![no_std]

use core::{
    num::ParseIntError,
    ops::BitAnd,
    ptr,
    simd::{Mask, Simd, SimdPartialEq, ToBitMask},
    str::FromStr,
};

pub const BYTES: usize = 64;

pub struct Scanner<'pattern, 'data: 'cursor, 'cursor, 'buffer: 'data + 'cursor> {
    pattern: &'pattern Pattern,
    data: &'data [u8],
    cursor: &'cursor [u8],
    buffer: &'buffer mut [u8; 2 * BYTES],
    buffer_in_use: bool,
}

impl<'pattern, 'data: 'cursor, 'cursor, 'buffer: 'data + 'cursor>
    Scanner<'pattern, 'data, 'cursor, 'buffer>
{
    pub fn new(
        pattern: &'pattern Pattern,
        data: &'data [u8],
        buffer: &'buffer mut [u8; 2 * BYTES],
    ) -> Scanner<'pattern, 'data, 'cursor, 'buffer> {
        Scanner {
            pattern,
            data,
            cursor: data,
            buffer,
            buffer_in_use: false,
        }
    }
}

impl<'pattern, 'data: 'cursor, 'cursor, 'buffer: 'data + 'cursor> Iterator
    for Scanner<'pattern, 'data, 'cursor, 'buffer>
{
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(index) = find_in_buffer(self.pattern, self.data, &mut self.cursor) {
            return Some(index);
        }
        if self.buffer_in_use {
            return None;
        }
        self.buffer_in_use = true;
        *self.buffer = [0; 2 * BYTES];
        let (data_stub, _) = self.buffer.split_at_mut(self.cursor.len());
        data_stub.copy_from_slice(self.cursor);
        // This is instant UB, but I don't know how to fix this.
        // This is UB because we violate aliasing and extend the lifetime.
        // self.buffer is a mutable reference while self.data is an immutable reference.
        let ptr = ptr::slice_from_raw_parts(self.buffer.as_ptr(), self.buffer.len());
        self.data = unsafe { &*ptr };
        self.cursor = unsafe { &*ptr };
        find_in_buffer(self.pattern, self.data, &mut self.cursor)
    }
}

fn find_in_buffer(pattern: &Pattern, data: &[u8], cursor: &mut &[u8]) -> Option<usize> {
    loop {
        if cursor.len() < BYTES + pattern.wildcard_prefix {
            break None;
        }

        let search = Simd::from_slice(&cursor[pattern.wildcard_prefix..]);
        let first_byte = search.simd_eq(pattern.first_byte).to_bitmask();

        if first_byte == 0 {
            *cursor = &cursor[BYTES..];
            continue;
        }
        *cursor = &cursor[first_byte.trailing_zeros() as usize..];

        if cursor.len() < BYTES {
            break None;
        }

        let search = Simd::from_slice(cursor);
        let result = search.simd_eq(pattern.bytes);
        let filtered_result = result.bitand(pattern.mask);
        let index = unsafe { cursor.as_ptr().offset_from(data.as_ptr()) };
        *cursor = &cursor[1..];
        if filtered_result == pattern.mask {
            return Some(index as usize);
        }
    }
}

#[derive(Clone)]
pub struct Pattern {
    pub(crate) bytes: Simd<u8, BYTES>,
    pub(crate) mask: Mask<i8, BYTES>,
    pub(crate) wildcard_prefix: usize,
    pub(crate) first_byte: Simd<u8, BYTES>,
}

impl Pattern {
    pub fn new(pattern: &str) -> Self {
        pattern.parse().unwrap()
    }

    pub fn matches<'pattern, 'data: 'cursor, 'cursor, 'buffer: 'data + 'cursor>(
        &'pattern self,
        data: &'data [u8],
        buffer: &'buffer mut [u8; 2 * BYTES],
    ) -> Scanner<'pattern, 'data, 'cursor, 'buffer> {
        Scanner::new(self, data, buffer)
    }
}

impl FromStr for Pattern {
    type Err = ParsePatternError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let length = s.split_ascii_whitespace().count();
        if length > BYTES {
            return Err(ParsePatternError::PatternTooLong);
        }

        let bytes = s.split_ascii_whitespace();
        let mut buffer = [0u8; BYTES];
        let mut mask = [false; BYTES];
        const WILDCARD: u8 = b'.';

        for (index, byte) in bytes.enumerate() {
            // allows . and ? as wildcard and only considers the first character
            if byte.as_bytes()[0] & WILDCARD == WILDCARD {
                continue;
            }
            buffer[index] = u8::from_str_radix(byte, 16)?;
            mask[index] = true;
        }

        let wildcard_prefix = mask.iter().take_while(|&&x| !x).count();
        let first_byte = Simd::from_array([buffer[wildcard_prefix]; BYTES]);

        Ok(Self {
            bytes: Simd::from_array(buffer),
            mask: Mask::from_array(mask),
            wildcard_prefix,
            first_byte,
        })
    }
}

#[derive(Debug)]
pub enum ParsePatternError {
    PatternTooLong,
    InvalidHexNumber(ParseIntError),
}

impl From<ParseIntError> for ParsePatternError {
    fn from(value: ParseIntError) -> Self {
        Self::InvalidHexNumber(value)
    }
}
