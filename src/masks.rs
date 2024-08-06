use core::simd::{cmp::SimdPartialOrd, LaneCount, Mask, Simd, SupportedLaneCount};

use crate::{BytesMask, Scanner, BYTES};

impl<'pattern, 'data, const ALIGNMENT: usize> Scanner<'pattern, 'data, ALIGNMENT>
where
    LaneCount<ALIGNMENT>: SupportedLaneCount,
{
    /// generates a mask that yields true until position `len`
    #[inline]
    pub(crate) fn data_len_mask(len: usize) -> Mask<i8, BYTES> {
        let len = len.min(BYTES);

        let mut index = [0u8; BYTES];
        index
            .iter_mut()
            .enumerate()
            .for_each(|(index, entry)| *entry = index as u8);
        let index = Simd::<u8, BYTES>::from_array(index);

        index.simd_lt(Simd::<u8, BYTES>::splat(len as u8))
    }

    /// Extends a length mask to ALIGNMENT if the given pattern mask fills the
    /// remaining bits until ALIGNMENT
    ///
    /// ```text
    /// mask  1011_1011_1011
    /// len   1111_1100_0000
    /// res   1111_1111_0000
    ///
    /// mask  1101_1101_1101
    /// len   1111_1000_0000
    /// res   1111_1000_0000
    /// ```
    pub(crate) const fn mask_min_len(len: BytesMask, pattern_mask: BytesMask) -> BytesMask {
        let groups = Self::reduce_bitmask(pattern_mask | len);
        // 1000_1000_0000
        let spread = Self::extend_bitmask(groups);
        // 1111_1111_0000
        spread | len
    }

    pub(crate) const fn chunk_mask() -> BytesMask {
        let pattern = 1;
        let mut mask = 0;
        let mut i = 0;
        while i < BYTES / ALIGNMENT {
            mask |= pattern << (ALIGNMENT * i);
            i += 1;
        }
        mask
    }

    /// filters the bitmask to valid chunks, little endian least-significant bit
    /// remains set
    ///
    /// ```text
    /// ALIGNMENT = 4
    /// start:  1111 1110 1101 1111
    /// result: 0001 0000 0000 0001
    /// ```
    #[inline]
    pub(crate) const fn reduce_bitmask(mut bitmask: BytesMask) -> BytesMask {
        let mut shift = 1;
        while shift < ALIGNMENT {
            bitmask &= bitmask >> shift;
            shift <<= 1;
        }

        bitmask & Self::chunk_mask()
    }

    /// extends the bitmask to entire chunks, little endian least-significant
    /// bit indicates chunk to extend
    ///
    /// ```text
    /// ALIGNMENT = 4
    /// start:  0101 1000 0010 0001
    /// result: 1111 0000 0000 1111
    /// ```
    #[inline]
    pub(crate) const fn extend_bitmask(mut bitmask: BytesMask) -> BytesMask {
        bitmask &= Self::chunk_mask();

        let mut shift = 1;
        while shift < ALIGNMENT {
            bitmask |= bitmask << shift;
            shift <<= 1;
        }

        bitmask
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn extend_align_1() {
        let reduced = Scanner::<'_, '_, 1>::extend_bitmask(MASK);
        let control = MASK;

        assert_eq!(reduced, control);
    }

    #[test]
    fn extend_align_2() {
        let reduced = Scanner::<'_, '_, 2>::extend_bitmask(MASK);
        let control =
            0b1111_1100_1111_0011_1111_1100_0000_0011_1111_0011_1100_0000_1100_0000_0011_0000;

        assert_eq!(reduced, control);
    }

    #[test]
    fn extend_align_4() {
        let reduced = Scanner::<'_, '_, 4>::extend_bitmask(MASK);
        let control =
            0b1111_0000_1111_1111_1111_0000_0000_1111_1111_1111_0000_0000_0000_0000_1111_0000;

        assert_eq!(reduced, control);
    }

    #[test]
    fn extend_align_8() {
        let reduced = Scanner::<'_, '_, 8>::extend_bitmask(MASK);
        let control =
            0b0000_0000_1111_1111_0000_0000_1111_1111_1111_1111_0000_0000_0000_0000_0000_0000;

        assert_eq!(reduced, control);
    }
}
