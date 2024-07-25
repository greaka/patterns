use aligned_vec::AVec;
use xxhash_rust::xxh3;

pub(crate) fn xxh3_data(length: usize) -> AVec<u8> {
    AVec::<u8>::from_iter(
        64,
        (0..length.div_ceil(8))
            .flat_map(|i| xxh3::xxh3_64(&i.to_be_bytes()).to_be_bytes())
            .take(length),
    )
}

pub(crate) fn with_misaligned<F: FnOnce(&[u8]) -> T, T>(data: &[u8], offset: usize, f: F) -> T {
    let vec = aligned_vec::AVec::<u8>::from_iter(
        64,
        core::iter::repeat(&0_u8)
            .take(offset)
            .chain(data.iter())
            .copied(),
    );
    f(&vec[offset..])
}
