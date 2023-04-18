use core::{slice, str::from_utf8};

use patterns::Pattern;

/// # Safety
/// `len` must be a valid length of `pat`. On success, the content of `res` will
/// not be null. There is no guarantee about the layout of `res` and it should
/// be considered opaque. The buffer behind `res` must be of size 256 bytes and
/// needs to be aligned to [`patterns::BYTES`] bytes! By default 64.
/// `pat` needs to be valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn parse_pattern(pat: *const u8, len: usize, res: *mut Pattern) {
    if pat.is_null() || res.is_null() {
        return;
    }
    if let Some(pattern) = from_utf8(slice::from_raw_parts(pat, len))
        .ok()
        .and_then(|x| x.parse().ok())
    {
        *res = pattern;
    }
}

/// # Safety
/// [in] `pat` must be the same pointer that was filled by [`parse_pattern`].
/// [in] `data` is the data to search through
/// [in] `len` must be the number of bytes of `data`.
/// [out] `res` will be filled with the result
/// [in] `res_len` is the amount of results that fit into `res`
/// [return] returns how many offsets were found
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
    let mut scan = pattern.matches(data);
    for (index, element) in res.iter_mut().enumerate() {
        let Some(offset) = scan.next() else {
            return index;
        };

        *element = offset;
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
            parse_pattern(pattern.as_bytes().as_ptr(), pattern.len(), &mut res as _);
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
