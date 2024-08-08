//! utility module to get around std functions not being const

use core::{num::IntErrorKind, str::from_utf8};

pub struct SplitAsciiWhitespace<'a> {
    bytes: &'a [u8],
}

impl<'a> SplitAsciiWhitespace<'a> {
    pub const fn new(s: &'a str) -> Self {
        Self {
            bytes: s.as_bytes(),
        }
    }

    pub const fn clone(&self) -> Self {
        Self { bytes: self.bytes }
    }

    pub const fn next(mut self) -> (Self, Option<&'a str>) {
        let bytes = self.bytes;

        let mut i = 0;
        let (mut chunk, mut rest) = bytes.split_at(i);
        while !rest.is_empty() && !rest[0].is_ascii_whitespace() {
            i += 1;
            (chunk, rest) = bytes.split_at(i);
        }

        if !rest.is_empty() {
            (_, rest) = rest.split_at(1);
        }
        self.bytes = rest;

        let n = if !chunk.is_empty() {
            match from_utf8(chunk) {
                Ok(t) => Some(t),
                Err(_) => None,
            }
        } else {
            None
        };

        (self, n)
    }

    pub const fn count(self) -> usize {
        let mut i = 0;
        let mut this = self;
        loop {
            let x;
            (this, x) = this.next();
            match x {
                Some(_) => i += 1,
                None => return i,
            }
        }
    }
}

/// allows . and ? as wildcard and only considers the first character
pub const fn is_wildcard(byte: &str) -> bool {
    const WILDCARD: u8 = b'.';
    byte.as_bytes()[0] & WILDCARD == WILDCARD
}

pub const fn hex_to_u8(hex: &str) -> Result<u8, IntErrorKind> {
    if hex.len() != 2 {
        return Err(IntErrorKind::InvalidDigit);
    }

    let mut index = 0;
    let mut result = 0;

    while index < 2 {
        let parsed = match hex.as_bytes()[index] {
            n @ b'0'..=b'9' => n - b'0',
            n @ b'A'..=b'F' => n - b'A' + 10,
            n @ b'a'..=b'f' => n - b'a' + 10,
            _ => return Err(IntErrorKind::InvalidDigit),
        };

        index += 1;
        result |= parsed << (4 * (2 - index));
    }

    Ok(result)
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::hex_to_u8;

    #[test]
    fn hex_00_to_u8() {
        assert_eq!(hex_to_u8("00").unwrap(), 0x00);
    }

    #[test]
    fn hex_99_to_u8() {
        assert_eq!(hex_to_u8("99").unwrap(), 0x99);
    }

    #[test]
    fn hex_AA_to_u8() {
        assert_eq!(hex_to_u8("AA").unwrap(), 0xAA);
    }

    #[test]
    fn hex_FF_to_u8() {
        assert_eq!(hex_to_u8("FF").unwrap(), 0xFF);
    }

    #[test]
    fn hex_aa_to_u8() {
        assert_eq!(hex_to_u8("aa").unwrap(), 0xaa);
    }

    #[test]
    fn hex_ff_to_u8() {
        assert_eq!(hex_to_u8("ff").unwrap(), 0xff);
    }
}
