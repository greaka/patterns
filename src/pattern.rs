use core::{
    marker::PhantomData,
    num::IntErrorKind,
    ops::Not,
    simd::{LaneCount, Simd, SupportedLaneCount},
    str::FromStr,
};

use crate::{const_utils, BytesMask, Scanner, VUNKNOWN as DEFAULT_BYTES};

/// A prepared pattern. Allows to search for a given byte sequence in data.
/// Supports masking and alignment requirements.
///
/// [`BYTES`] Determines the LANES size.
/// Every block of data is processed in chunks of `BYTES` bytes.
/// Rust will compile this to other targets without issue, but will use
/// inner loops for that.
/// It is also the max length for patterns.
#[derive(Clone, Debug)]
pub struct Pattern<const ALIGNMENT: usize = 1, const BYTES: usize = DEFAULT_BYTES>
where
    LaneCount<ALIGNMENT>: SupportedLaneCount,
    LaneCount<BYTES>: SupportedLaneCount,
{
    pub(crate) bytes: Simd<u8, BYTES>,
    pub(crate) first_bytes: Simd<u8, BYTES>,
    pub(crate) mask: BytesMask,
    /// first bytes mask is inverted
    /// x & mask == mask === x | ^mask == -1
    pub(crate) first_bytes_mask: BytesMask,
    pub(crate) first_byte_offset: u8,
    pub(crate) length: u8,
    phantom: PhantomData<[u8; ALIGNMENT]>,
}

impl<const ALIGNMENT: usize, const BYTES: usize> Pattern<ALIGNMENT, BYTES>
where
    LaneCount<ALIGNMENT>: SupportedLaneCount,
    LaneCount<BYTES>: SupportedLaneCount,
{
    /// Parse a pattern. Use the [`FromStr`] impl to return an error instead of
    /// panicking.
    ///
    /// # Panics
    /// Panics if [`ParsePatternError`] is returned.
    #[must_use]
    #[inline]
    pub const fn new(pattern: &str) -> Self {
        match Self::from_str(pattern) {
            Ok(p) => p,
            Err(ParsePatternError::PatternTooLong) => panic!("PatternTooLong"),
            Err(ParsePatternError::InvalidHexNumber(..)) => panic!("InvalidHexNumber"),
            Err(ParsePatternError::MissingNonWildcardByte) => panic!("MissingNonWildcardByte"),
        }
    }

    /// Create a pattern from a byte slice and a mask.
    /// Byte slices longer than [`BYTES`] are cut short.
    /// Mask expects a [`u64`] bitencoding. A 0 bit marks the byte as wildcard.
    /// Mask is trimmed to `bytes.len()`.
    ///
    /// # Panics
    /// Panics when all bytes are masked as wildcards.
    pub fn from_slice(bytes: &[u8], mask: u64) -> Self {
        let mut input: [u8; BYTES] = [0; BYTES];
        let length = bytes.len().min(BYTES);
        input[..length].copy_from_slice(bytes);
        let mask = u64::MAX.checked_shr(length as u32).unwrap_or(0).not() & mask;
        let bytes = Simd::<u8, BYTES>::from_array(input);
        let mask = mask.reverse_bits();

        let first_byte_offset = Self::find_first_byte_offset(mask).unwrap();

        let (first_bytes, first_bytes_mask) = fill_first_bytes::<ALIGNMENT, BYTES>(
            &input[first_byte_offset..],
            mask >> first_byte_offset,
        );

        Self {
            bytes,
            mask,
            first_bytes,
            first_bytes_mask,
            first_byte_offset: first_byte_offset as _,
            length: length as _,
            phantom: PhantomData,
        }
    }

    pub const fn from_str(s: &str) -> Result<Self, ParsePatternError> {
        let bytes = const_utils::SplitAsciiWhitespace::new(s);

        let length = bytes.clone().count();
        if length > BYTES {
            return Err(ParsePatternError::PatternTooLong);
        }

        let (buffer, mask) = {
            let mut buffer = [0_u8; BYTES];
            let mut mask = 0;
            let mut index = 0;
            let mut bytes = bytes;

            loop {
                let byte;
                (bytes, byte) = bytes.next();
                let byte = match byte {
                    Some(b) => b,
                    None => break,
                };

                if !const_utils::is_wildcard(byte) {
                    let parsed = match const_utils::hex_to_u8(byte) {
                        Ok(parsed) => parsed,
                        Err(e) => return Err(ParsePatternError::InvalidHexNumber(e)),
                    };
                    buffer[index] = parsed;
                    mask |= 1 << index;
                }

                index += 1;
            }

            (buffer, mask)
        };

        let first_byte_offset = match Self::find_first_byte_offset(mask) {
            Ok(offset) => offset,
            Err(e) => return Err(e),
        };

        let (_, chunk) = buffer.split_at(first_byte_offset);
        let mask_chunk = mask >> first_byte_offset;
        let (first_bytes, first_bytes_mask) =
            fill_first_bytes::<ALIGNMENT, BYTES>(chunk, mask_chunk);

        Ok(Self {
            bytes: Simd::<u8, BYTES>::from_array(buffer),
            mask,
            first_bytes,
            first_bytes_mask,
            first_byte_offset: first_byte_offset as _,
            length: length as _,
            phantom: PhantomData,
        })
    }

    /// Creates an iterator through data. See [`Scanner::new`] for remarks.
    #[inline]
    pub fn matches<'pattern, 'data>(
        &'pattern self,
        data: &'data [u8],
    ) -> Scanner<'pattern, 'data, ALIGNMENT, BYTES> {
        Scanner::new(self, data)
    }

    const fn find_first_byte_offset(mut mask: BytesMask) -> Result<usize, ParsePatternError> {
        let align_mask = Scanner::<ALIGNMENT, BYTES>::data_len_mask(ALIGNMENT);
        let mut i = 0;
        let mut smallest = 0;
        let mut highest_count = 0;
        loop {
            if mask == 0 {
                break;
            }
            let chunk = mask & align_mask;
            mask = if let Some(mask) = mask.checked_shr(ALIGNMENT as u32) {
                mask
            } else {
                0
            };

            let chunk_count = chunk.count_ones();

            if chunk_count > highest_count {
                highest_count = chunk_count;
                smallest = i;
            }

            i += 1;
        }

        if highest_count == 0 {
            Err(ParsePatternError::MissingNonWildcardByte)
        } else {
            Ok(smallest * ALIGNMENT)
        }
    }
}

const fn fill_first_bytes<const ALIGNMENT: usize, const BYTES: usize>(
    chunk: &[u8],
    mask: BytesMask,
) -> (Simd<u8, BYTES>, BytesMask)
where
    LaneCount<ALIGNMENT>: SupportedLaneCount,
    LaneCount<BYTES>: SupportedLaneCount,
{
    let mut first = [0u8; BYTES];
    let mut first_mask = 0;

    let mut i = 0;

    while i < BYTES / ALIGNMENT {
        let mut j = 0;
        while j < ALIGNMENT {
            first[i * ALIGNMENT + j] = chunk[j];
            first_mask |= ((((mask >> j) as i8 & 1) == 0) as BytesMask) << (i * ALIGNMENT + j);
            j += 1;
        }
        i += 1;
    }

    let bytes = Simd::from_array(first);

    (bytes, first_mask)
}

impl FromStr for Pattern {
    type Err = ParsePatternError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Pattern::from_str(s)
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum ParsePatternError {
    PatternTooLong,
    InvalidHexNumber(IntErrorKind),
    MissingNonWildcardByte,
}

impl From<IntErrorKind> for ParsePatternError {
    #[inline]
    fn from(value: IntErrorKind) -> Self {
        Self::InvalidHexNumber(value)
    }
}
