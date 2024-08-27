#include <stdalign.h>
#include <stddef.h>

// needs to stay in sync with lib.rs
#ifndef BYTES_IN_USE
    #define BYTES_IN_USE 64
#endif

struct pattern
{
    alignas(BYTES_IN_USE) char pat[BYTES_IN_USE * 4 + 2];
    char align;
};

/// # Safety
/// [in] `pat` needs to be valid UTF-8.
/// [in] `len` must be a valid length of `pat`.
/// [in] `align` must be a power of 2 less than or equal to 64.
/// [out] The buffer behind `res` must be of size `4 * 64 + 2` bytes and needs
/// to be aligned to 64 bytes. There is no guarantee about the layout of
/// `res->pat` and it should be considered opaque.
/// [return] returns true on success.
char parse_pattern(const char *pat, size_t len, char align, struct pattern *res);

/// # Safety
/// [in] `pat` must be the same pointer that was filled by [`parse_pattern`].
/// [in] `data` is the data to search through.
/// [in] `len` must be the number of bytes of `data`.
/// [out] `res` will be filled with the result.
/// [in] `res_len` is the amount of results that fit into `res`.
/// [return] returns how many offsets were found.
size_t match_pattern(
    const struct pattern *pat,
    const char *data,
    size_t len,
    size_t *res,
    size_t res_len
);
