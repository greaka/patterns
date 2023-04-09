use core::{slice, str::from_utf8};

use patterns::Pattern;

/// # Safety
/// `len` must be a valid length of `pat`. On success, `res` will not be null.
/// There is no guarantee about the layout of `res` and it should be considered
/// opaque. The buffer behind `res` must be of size 256 bytes
#[no_mangle]
pub unsafe extern "C" fn parse_pattern(pat: *const u8, len: usize, res: *mut Pattern) {
    if let Some(pattern) = from_utf8(slice::from_raw_parts(pat, len))
        .ok()
        .and_then(|x| x.parse().ok())
    {
        *res = pattern;
    }
}

/// # Safety
/// `pat` needs to be aligned to [`patterns::BYTES`] bytes! By default 64.
/// `len` must be the number of bytes of `data`. `res` must hold at least
/// `8 * number of results` bytes. The number required is unknown.
/// On success, `*num_res` will not be null.
#[no_mangle]
pub unsafe extern "C" fn match_pattern(
    pat: *const Pattern,
    data: *const u8,
    len: usize,
    res: *mut usize,
    num_res: *mut usize,
) {
    if pat.is_null() {
        return;
    }
    let data = slice::from_raw_parts(data, len);
    let pattern = &*pat;
    let scan = pattern.matches(data);
    for (index, found) in scan.enumerate() {
        *res.add(index) = found;
        *num_res = index + 1;
    }
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
        let mut num_results = 0usize;
        unsafe {
            parse_pattern(pattern.as_bytes().as_ptr(), pattern.len(), &mut res as _);
            match_pattern(
                &res as _,
                data.as_ptr() as _,
                data.len(),
                &mut results as _,
                &mut num_results as _,
            );
        }
        assert_eq!(num_results, 1);
        assert_eq!(results[0], data.len() - 8);
    }
}
