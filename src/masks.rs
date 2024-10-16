use core::simd::{
    cmp::{SimdPartialEq, SimdPartialOrd},
    LaneCount, Mask, Simd, SupportedLaneCount, Swizzle,
};

use crate::{transmute_yolo, Scanner};

impl<'pattern, 'data, const ALIGNMENT: usize, const BYTES: usize>
    Scanner<'pattern, 'data, ALIGNMENT, BYTES>
where
    LaneCount<ALIGNMENT>: SupportedLaneCount,
    LaneCount<BYTES>: SupportedLaneCount,
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
    pub(crate) fn mask_min_len(
        len: Mask<i8, BYTES>,
        pattern_mask: Mask<i8, BYTES>,
    ) -> Mask<i8, BYTES> {
        let groups = Self::reduce_bitmask(pattern_mask | len);
        // 1000_1000_0000
        let spread = Self::extend_bitmask(groups);
        // 1111_1111_0000
        spread | len
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
    pub(crate) fn reduce_bitmask(bitmask: Mask<i8, BYTES>) -> Mask<i8, BYTES> {
        match ALIGNMENT {
            1 => bitmask,
            2 => match BYTES {
                64 => {
                    let bitmask: Simd<i16, 32> = transmute_yolo!(bitmask);
                    let eq = bitmask.simd_eq(Simd::splat(-1));
                    transmute_yolo!(eq)
                }
                _ => unimplemented!(),
            },
            _ => unimplemented!(),
        }
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
    pub(crate) fn extend_bitmask(bitmask: Mask<i8, BYTES>) -> Mask<i8, BYTES> {
        unsafe {
            *(&<Splatter<ALIGNMENT> as Swizzle<BYTES>>::swizzle(
                *(&bitmask as *const Mask<i8, BYTES> as *const Simd<i8, BYTES>),
            ) as *const _ as *const _)
        }
    }
}

struct Splatter<const WIDTH: usize>;
impl<const N: usize, const WIDTH: usize> Swizzle<N> for Splatter<WIDTH> {
    const INDEX: [usize; N] = const {
        let mut index = [0; N];
        let mut i = 0;
        while i < N / WIDTH {
            let mut j = 0;
            while j < WIDTH {
                index[i * WIDTH + j] = i * WIDTH;
                j += 1;
            }
            i += 1;
        }
        index
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BytesMask;

    const MASK: BytesMask =
        0b1111_1110_1101_1011_0111_1100_1010_1001_0101_0011_0110_1000_0100_0010_0001_0000;
    const BYTES: usize = 64;

    fn mask() -> Mask<i8, BYTES> {
        Mask::from_bitmask(MASK)
    }

    #[test]
    fn reduce_align_1() {
        let reduced = Scanner::<'_, '_, 1, BYTES>::reduce_bitmask(mask()).to_bitmask();
        let control = MASK;
        let control = control & (u64::MAX >> (64 - BYTES));

        assert_eq!(reduced, control);
    }

    #[test]
    fn reduce_align_2() {
        let reduced = Scanner::<'_, '_, 2, BYTES>::reduce_bitmask(mask()).to_bitmask();
        let control =
            0b0101_0100_0100_0001_0001_0100_0000_0000_0000_0001_0000_0000_0000_0000_0000_0000;
        let control = control & (u64::MAX >> (64 - BYTES));

        assert_eq!(reduced, control);
    }

    #[test]
    fn reduce_align_4() {
        let reduced = Scanner::<'_, '_, 4, BYTES>::reduce_bitmask(mask()).to_bitmask();
        let control = 1 << 60;
        let control = control & (u64::MAX >> (64 - BYTES));

        assert_eq!(reduced, control);
    }

    #[test]
    fn reduce_align_8() {
        let reduced = Scanner::<'_, '_, 8, BYTES>::reduce_bitmask(mask()).to_bitmask();
        let control = 0;
        let control = control & (u64::MAX >> (64 - BYTES));

        assert_eq!(reduced, control);
    }

    #[test]
    fn extend_align_1() {
        let reduced = Scanner::<'_, '_, 1, BYTES>::extend_bitmask(mask()).to_bitmask();
        let control = MASK;
        let control = control & (u64::MAX >> (64 - BYTES));

        assert_eq!(reduced, control);
    }

    #[test]
    fn extend_align_2() {
        let reduced = Scanner::<'_, '_, 2, BYTES>::extend_bitmask(mask()).to_bitmask();
        let control =
            0b1111_1100_1111_0011_1111_1100_0000_0011_1111_0011_1100_0000_1100_0000_0011_0000;
        let control = control & (u64::MAX >> (64 - BYTES));

        assert_eq!(reduced, control);
    }

    #[test]
    fn extend_align_4() {
        let reduced = Scanner::<'_, '_, 4, BYTES>::extend_bitmask(mask()).to_bitmask();
        let control =
            0b1111_0000_1111_1111_1111_0000_0000_1111_1111_1111_0000_0000_0000_0000_1111_0000;
        let control = control & (u64::MAX >> (64 - BYTES));

        assert_eq!(reduced, control);
    }

    #[test]
    fn extend_align_8() {
        let reduced = Scanner::<'_, '_, 8, BYTES>::extend_bitmask(mask()).to_bitmask();
        let control =
            0b0000_0000_1111_1111_0000_0000_1111_1111_1111_1111_0000_0000_0000_0000_0000_0000;
        let control = control & (u64::MAX >> (64 - BYTES));

        assert_eq!(reduced, control);
    }
}
