use core::{slice, str::from_utf8};

use patterns::Pattern as InternalPattern;

#[cfg(feature = "bytes_64")]
const BYTES_IN_USE: usize = 64;
#[cfg(feature = "bytes_32")]
const BYTES_IN_USE: usize = 32;
#[cfg(feature = "bytes_16")]
const BYTES_IN_USE: usize = 16;
#[cfg(feature = "bytes_8")]
const BYTES_IN_USE: usize = 8;
#[cfg(feature = "bytes_4")]
const BYTES_IN_USE: usize = 4;
#[cfg(feature = "bytes_2")]
const BYTES_IN_USE: usize = 2;
#[cfg(feature = "bytes_1")]
const BYTES_IN_USE: usize = 1;

type PatternInUse = InternalPattern<1, BYTES_IN_USE>;

#[repr(C)]
pub struct Pattern {
    pat: PatternInUse,
    pub align: u8,
}

/// # Safety
/// [in] `pat` needs to be valid UTF-8. The resulting pattern must not be
/// greater than 64.
/// [in] `len` must be a valid length of `pat`.
/// [in] `align` must be a power of 2 less than or equal to 64.
/// [out] The buffer behind `res` must be of size `4 * 64 + 2` bytes and needs
/// to be aligned to 64 bytes. There is no guarantee about the layout of
/// `res->pat` and it should be considered opaque.
/// On success, the content of `res->align` will not be null.
/// [return] returns true on success.
#[no_mangle]
pub unsafe extern "C" fn parse_pattern(
    pat: *const u8,
    len: usize,
    align: u8,
    res: *mut Pattern,
) -> bool {
    if let Some(pattern) = from_utf8(slice::from_raw_parts(pat, len))
        .ok()
        .and_then(|x| {
            macro_rules! parse {
                ($align:expr) => {
                    core::ptr::read(&InternalPattern::<$align, BYTES_IN_USE>::from_str(x).ok()
                        as *const _ as *const _)
                };
            }

            match align {
                #[cfg(feature = "align_1")]
                1 => PatternInUse::from_str(x).ok(),
                #[cfg(feature = "align_2")]
                2 => parse!(2),
                #[cfg(feature = "align_4")]
                4 => parse!(4),
                #[cfg(feature = "align_8")]
                8 => parse!(8),
                #[cfg(feature = "align_16")]
                16 => parse!(16),
                #[cfg(feature = "align_32")]
                32 => parse!(32),
                #[cfg(feature = "align_64")]
                64 => parse!(64),
                _ => unreachable!(),
            }
        })
    {
        let res = &mut *res;
        res.pat = pattern;
        res.align = align;
        true
    } else {
        false
    }
}

#[allow(clippy::useless_transmute)]
#[no_mangle]
pub unsafe extern "C" fn load_pattern(
    pat: *const u8,
    len: usize,
    mask: u64,
    align: u8,
    res: *mut Pattern,
) {
    macro_rules! load {
        ($align:expr) => {
            core::ptr::read(&InternalPattern::<$align, BYTES_IN_USE>::from_slice(
                slice::from_raw_parts(pat, len),
                mask,
            ) as *const _ as *const _)
        };
    }
    let pattern = match align {
        #[cfg(feature = "align_1")]
        1 => load!(1),
        #[cfg(feature = "align_2")]
        2 => load!(2),
        #[cfg(feature = "align_4")]
        4 => load!(4),
        #[cfg(feature = "align_8")]
        8 => load!(8),
        #[cfg(feature = "align_16")]
        16 => load!(16),
        #[cfg(feature = "align_32")]
        32 => load!(32),
        #[cfg(feature = "align_64")]
        64 => load!(64),
        _ => unreachable!(),
    };

    let res = &mut *res;
    res.pat = pattern;
    res.align = align;
}

/// # Safety
/// [in] `pat` must be the same pointer that was filled by [`parse_pattern`].
/// [in] `data` is the data to search through.
/// [in] `len` must be the number of bytes of `data`.
/// [out] `res` will be filled with the result.
/// [in] `res_len` is the amount of results that fit into `res`.
/// [return] returns how many offsets were found.
#[no_mangle]
pub unsafe extern "C" fn match_pattern(
    pat: *const Pattern,
    data: *const u8,
    len: usize,
    res: *mut usize,
    res_len: usize,
) -> usize {
    if pat.is_null() || res.is_null() || data.is_null() {
        return 0;
    }
    let data = slice::from_raw_parts(data, len);
    let res = slice::from_raw_parts_mut(res, res_len);
    let pattern = &*pat;

    macro_rules! execute {
        ($align:expr) => {{
            let pattern = &*(&pattern.pat as *const PatternInUse
                as *const InternalPattern<$align, BYTES_IN_USE>);
            let mut scan = pattern.matches(data);
            for (index, element) in res.iter_mut().enumerate() {
                let Some(offset) = scan.next() else {
                    return index;
                };

                *element = offset;
            }
        }};
    }

    match pattern.align {
        #[cfg(feature = "align_1")]
        1 => execute!(1),
        #[cfg(feature = "align_2")]
        2 => execute!(2),
        #[cfg(feature = "align_4")]
        4 => execute!(4),
        #[cfg(feature = "align_8")]
        8 => execute!(8),
        #[cfg(feature = "align_16")]
        16 => execute!(16),
        #[cfg(feature = "align_32")]
        32 => execute!(32),
        #[cfg(feature = "align_64")]
        64 => execute!(64),
        _ => unreachable!(),
    }

    res_len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn sanity() {
        let pattern = "01 01 01 01 01 01 01 01";
        let mut data = vec![0u8; 1_000_000];
        let len = data.len();
        data[len - 8..].fill(1);
        let mut res: Pattern = unsafe { core::mem::zeroed() };
        let mut results = [0usize; 1];
        let num_results = 1usize;
        unsafe {
            parse_pattern(pattern.as_bytes().as_ptr(), pattern.len(), 1, &mut res as _);
            match_pattern(
                &res as _,
                data.as_ptr() as _,
                data.len(),
                results.as_mut_ptr(),
                1,
            );
        }
        assert_eq!(num_results, 1);
        assert_eq!(results[0], data.len() - 8);
    }
}
