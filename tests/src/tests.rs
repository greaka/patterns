mod utils;
use patterns::*;
use utils::*;

fn simple(pattern: &str, data: &[u8]) -> Vec<usize> {
    dbg!(pattern);
    pattern.parse::<Pattern>().unwrap().matches(data).collect()
}

#[test]
fn basic() {
    assert_eq!(simple("42", &[0x42]), &[0]);
    assert_eq!(simple("24", &[0x42]), &[]);
    assert_eq!(simple("42", &[0x42; 2]), &[0, 1]);
}

#[test]
fn leading_wildcard() {
    assert_eq!(simple("? 42", &[0x42]), &[]);
    assert_eq!(simple("? 42", &[0x22, 0x42]), &[0]);
}

#[test]
fn trailing_wildcard() {
    assert_eq!(simple("42 ?", &[0x42]), &[]);
}

#[test]
fn trailing_zero() {
    assert_eq!(simple("00", &[0x42]), &[]);
    assert_eq!(simple("42 00", &[0x42]), &[]);
}

#[test]
fn xxh3_data_test() {
    assert_eq!(
        xxh3_data(16).as_slice(),
        &[199, 123, 58, 187, 111, 135, 172, 217, 243, 107, 74, 26, 68, 247, 139, 243]
    );
}

#[test]
fn repeat_across_buffer() {
    let mut data = [0_u8; 64];
    data[0] = 1;
    data[1] = 1;
    assert_eq!(simple("01", &data), [0, 1]);
}

#[test]
fn small() {
    //    00 01 02 03 04 05 06 07 08 09 0A 0B 0C 0D 0E 0F
    // 0x c7 7b 3a bb 6f 87 ac d9 f3 6b 4a 1a 44 f7 8b f3
    // 1x 3e 69 48 79 79 85 51 1c d0 36 c6 a9 c6 b3 1c 1d
    // 2x 93 47 f2 9a a4 16 00 1e c2 8f 1f 5e 73 70 05 06
    // 3x 4c 14 53 22 e9 63 61 c2 f8 c0 12 6b 89 b4 fa fc
    let data = xxh3_data(64);
    for offset in 0..63 {
        with_misaligned(&data, offset, |data| {
            assert_eq!(simple("c7 7b", &data), &[0]);
            assert_eq!(simple("c7 7b ?", &data), &[0]);
            assert_eq!(simple("? c7 7b", &data), &[]);
            assert_eq!(simple("f3", &data), &[0x08, 0x0F]);
            assert_eq!(simple("f3 ? 4a", &data), &[0x08]);
            assert_eq!(simple("f3 ? 69", &data), &[0x0F]);
            assert_eq!(simple("c2", &data), &[0x28, 0x37]);
            assert_eq!(simple("c2 ? ? 5e", &data), &[0x28]);
            assert_eq!(simple("c2 ? ? 12", &data), &[0x37]);
            assert_eq!(simple("14 53 22 e9 63", &data), &[0x31]);

            // uneven tail
            assert_eq!(simple("c2", &data[..=0x37]), &[0x28, 0x37]);
            assert_eq!(simple("14 53 22 e9 63", &data[..=0x37]), &[0x31]);
        });
    }

    // double
    let data2 = data.repeat(2);
    for offset in 0..63 {
        with_misaligned(&data2, offset, |data2| {
            assert_eq!(simple("c7 7b", &data2), &[0, 64]);
            assert_eq!(simple("c7 7b ?", &data2), &[0, 64]);
            assert_eq!(simple("? c7 7b", &data2), &[63]);
            assert_eq!(simple("f3", &data2), &[0x08, 0x0F, 0x48, 0x4F]);
            assert_eq!(simple("f3 ? 4a", &data2), &[0x08, 0x48]);
            assert_eq!(simple("f3 ? 69", &data2), &[0x0F, 0x4F]);
            assert_eq!(simple("c2", &data2), &[0x28, 0x37, 0x68, 0x77]);
            assert_eq!(simple("c2 ? ? 5e", &data2), &[0x28, 0x68]);
            assert_eq!(simple("c2 ? ? 12", &data2), &[0x37, 0x77]);
            assert_eq!(simple("14 53 22 e9 63", &data2), &[0x31, 0x71]);

            // across block boundary
            assert_eq!(simple("fa fc c7", &data2), &[0x3E]);
            assert_eq!(simple("fc c7 7b", &data2), &[0x3F]);
            assert_eq!(simple("fc ?? 7b", &data2), &[0x3F]);

            // uneven tail
            assert_eq!(simple("c2", &data2[..=0x77]), &[0x28, 0x37, 0x68, 0x77]);
            assert_eq!(simple("14 53 22 e9 63", &data2[..=0x77]), &[0x31, 0x71]);

            // wildcard beyond the end, 2nd iterator call
            assert_eq!(simple("6b ?? ?? ?? ?? ??", &data2), &[0x09, 0x3B, 0x49]);
        });
    }
}
