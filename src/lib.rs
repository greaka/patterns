#![feature(portable_simd)]
#![no_std]

use core::{
    num::ParseIntError,
    ops::{BitAnd, Deref},
    simd::{Mask, Simd, SimdPartialEq, ToBitMask},
    str::FromStr,
};

/// Determines the LANES size. i.e.: register size;
/// Every block of data is processed in chunks of `BYTES` bytes.
pub const BYTES: usize = 64;

pub struct Scanner<'pattern, 'data: 'cursor, 'cursor> {
    pattern: &'pattern Pattern,
    data: &'data [u8],
    cursor: &'cursor [u8],
    position: usize,
    buffer: Buffer,
}

impl<'pattern, 'data: 'cursor, 'cursor> Scanner<'pattern, 'data, 'cursor> {
    pub fn new(pattern: &'pattern Pattern, data: &'data [u8]) -> Scanner<'pattern, 'data, 'cursor> {
        Scanner {
            pattern,
            data,
            cursor: data,
            buffer: Buffer::new(),
            position: 0,
        }
    }
}

impl<'pattern, 'data: 'cursor, 'cursor> Iterator for Scanner<'pattern, 'data, 'cursor> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(index) = find_in_buffer(self.pattern, self.data, &mut self.cursor) {
                return Some(self.position + index);
            }
            if self.buffer.in_use() {
                return None;
            }
            self.copy_to_buffer();
        }
    }
}

impl<'pattern, 'data: 'cursor, 'cursor> Scanner<'pattern, 'data, 'cursor> {
    fn copy_to_buffer(&mut self) {
        self.save_position();
        self.buffer.copy(self.cursor);
        // This is instant UB, but I don't know how to fix this.
        // This is UB because we violate aliasing and extend the lifetime.
        // self.buffer is a mutable reference while self.data is an immutable reference.
        self.cursor = unsafe { &*(self.buffer.deref() as *const [u8]) };
        self.data = unsafe { &*(self.buffer.deref() as *const [u8]) };
    }

    fn save_position(&mut self) {
        unsafe {
            self.position = self.cursor.as_ptr().offset_from(self.data.as_ptr()) as usize;
        }
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

    pub fn matches<'pattern, 'data: 'cursor, 'cursor>(
        &'pattern self,
        data: &'data [u8],
    ) -> Scanner<'pattern, 'data, 'cursor> {
        Scanner::new(self, data)
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

struct Buffer {
    // 3 * BYTES = 1x for rest of the data, 1x to not overrun,
    // 1x for weird patterns with a lot of prefix wildcards
    inner: [u8; 3 * BYTES],
    in_use: bool,
}

impl Buffer {
    pub(crate) fn new() -> Self {
        Self {
            in_use: false,
            inner: [0u8; 3 * BYTES],
        }
    }

    pub(crate) fn copy(&mut self, data: &[u8]) {
        if self.in_use {
            panic!("buffer reused");
        }
        self.in_use = true;
        let (data_stub, _) = self.inner.split_at_mut(data.len());
        data_stub.copy_from_slice(data);
    }

    pub(crate) fn in_use(&self) -> bool {
        self.in_use
    }
}

impl Deref for Buffer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.inner
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
