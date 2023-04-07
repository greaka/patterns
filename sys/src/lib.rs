#![no_std]

use core::{slice, str::from_utf8};

use patterns::{Pattern, BYTES};

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

/// # Safety
/// `len` must be a valid length of `pat`. On success, `res` will not be null.
/// There is no guarantee about the layout of `res` and it should be considered
/// opaque.
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
/// `len` must be a valid length of `data`. `res` must be at least as big as the
/// number of results. On success, `*num_res` will not be null.
#[no_mangle]
pub unsafe extern "C" fn match_pattern(
    pat: *mut Pattern,
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
    let mut buf = [0; 2 * BYTES];
    let scan = pattern.matches(data, &mut buf);
    for (index, found) in scan.enumerate() {
        *res.add(index) = found;
        *num_res = index;
    }
}
