// //! Reference implementation for differential testing
use std::num::ParseIntError;

pub struct Pattern {
    /// Bytes of the pattern. `None` used for wildcard bytes.
    pub pattern: Vec<Option<u8>>,
    alignment: u8,
}

pub struct Scanner<'pattern, 'data> {
    pattern: &'pattern Pattern,
    data: &'data [u8],
    offset: usize,
}

impl<'pattern, 'data> Scanner<'pattern, 'data> {
    pub fn new(pattern: &'pattern Pattern, data: &'data [u8]) -> Self {
        Self {
            pattern,
            data,
            offset: data.as_ptr().align_offset(pattern.alignment as usize),
        }
    }
}

impl<'pattern, 'data> Iterator for Scanner<'pattern, 'data> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        while self.data.len() >= self.offset + self.pattern.pattern.len() {
            let ret = plain_match(self.pattern, &self.data[self.offset..]).then_some(self.offset);
            self.offset += self.pattern.alignment as usize;
            if ret.is_some() {
                return ret;
            }
        }

        None
    }
}

impl Pattern {
    pub fn from_str(s: &str, alignment: u8) -> Result<Self, ParseIntError> {
        assert!((1..=64).contains(&alignment));

        /// allows . and ? as wildcard and only considers the first character
        fn is_wildcard(byte: &str) -> bool {
            let c = byte.chars().next().unwrap_or_default();
            c == '.' || c == '?'
        }

        let pattern = s
            .split_ascii_whitespace()
            .map(|s| {
                if is_wildcard(s) {
                    Ok(None)
                } else {
                    Ok(Some(u8::from_str_radix(s, 16)?))
                }
            })
            .collect::<Result<Vec<_>, ParseIntError>>()?;

        assert!(pattern.iter().any(Option::is_some));

        Ok(Self { pattern, alignment })
    }

    pub fn matches<'pattern, 'data>(&'pattern self, data: &'data [u8]) -> Scanner<'pattern, 'data> {
        Scanner::new(self, data)
    }
}

/// Match `pattern` against the start of `data` (without SIMD)
///
/// Assumes that data.len() >= pattern.length
fn plain_match(pattern: &Pattern, data: &[u8]) -> bool {
    // Triple-zip iterator over the pattern.length prefix pattern, mask and data
    pattern.pattern
        .iter()
        .zip(data[..pattern.pattern.len()].iter())
        // If all pattern bytes are either masked or equal the data bytes, the pattern matches the data
        .all(|(&pattern_byte, &data_byte)| pattern_byte.map(|p| p == data_byte).unwrap_or(true))
}

#[cfg(test)]
mod test {
    use std::{collections::HashMap, panic::catch_unwind};

    use aligned_vec::AVec;
    use xxhash_rust::xxh3;

    use super::*;

    fn xxh3_data(length: usize) -> AVec<u8> {
        AVec::<u8>::from_iter(
            64,
            (0..length.div_ceil(8))
                .flat_map(|i| xxh3::xxh3_64(&i.to_be_bytes()).to_be_bytes())
                .take(length),
        )
    }

    fn with_misaligned<F: FnOnce(&[u8]) -> T, T>(data: &[u8], offset: usize, f: F) -> T {
        let vec = aligned_vec::AVec::<u8>::from_iter(
            64,
            core::iter::repeat(&0_u8)
                .take(offset)
                .chain(data.iter())
                .copied(),
        );
        f(&vec[offset..])
    }

    #[track_caller]
    fn all_alignments(pattern: &str, data: &[u8], matches: &[usize]) -> bool {
        let location = std::panic::Location::caller();
        let parsed = Pattern::from_str(pattern, 1).unwrap();

        let run = |data: &[u8]| -> Vec<Result<Vec<usize>, String>> {
            (0..=63)
                .map(|i| {
                    with_misaligned(data, i, |data| {
                        // hide panic backtraces
                        let hook = std::panic::take_hook();
                        std::panic::set_hook(Box::new(|_| {}));

                        let ret = catch_unwind(|| parsed.matches(data).collect::<Vec<_>>())
                            .map_err(|msg| {
                                msg.downcast::<String>()
                                    .map(|s| *s)
                                    .or_else(|msg| msg.downcast::<&str>().map(|s| s.to_string()))
                                    .unwrap_or_else(|_| "other panic".to_owned())
                            });

                        std::panic::set_hook(hook);
                        ret
                    })
                })
                .collect()
        };

        let results = run(data);

        if results
            .iter()
            .all(|result| result.as_ref().is_ok_and(|r| r == matches))
        {
            return true;
        }

        eprintln!();

        eprintln!("[{location}] TEST FAILED");
        eprintln!("[{location}] pattern = {pattern:?}");
        if data.len() < 16 {
            eprintln!(
                "[{location}] data    = [{}]",
                data.iter()
                    .map(|i| format!("{:02x}", i))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        eprintln!("[{location}] matches = {matches:x?}");

        // key: result, value: alignments
        let mut hm: HashMap<Result<&[usize], &String>, Vec<usize>> = HashMap::new();
        for (i, r) in results.iter().enumerate() {
            match r {
                Ok(r) => hm.entry(Ok(r)).or_default().push(i),
                Err(msg) => hm.entry(Err(msg)).or_default().push(i),
            };
        }

        // trivial case -- result is the same for all alignments
        if hm.len() == 1 {
            match &results[0] {
                Ok(r) => eprintln!("[{location}] result = {:x?}", r),
                Err(msg) => eprintln!("[{location}] result = panic {:?}", msg),
            }
            return false;
        }

        hm.remove(&Ok(matches));
        // hm.remove(&Ok(&[]));

        if (1..=10).contains(&hm.len()) {
            let mut aligns = ['.'; 64];
            let mut tmp = hm.iter().collect::<Vec<_>>();
            tmp.sort_by_key(|(_, v)| *v);
            for (i, (_, v)) in tmp.iter().enumerate() {
                for &a in *v {
                    aligns[a] = char::from_digit(i as u32, 10).unwrap();
                }
            }
            eprintln!(
                "[{location}] offset   ({})",
                results
                    .iter()
                    .enumerate()
                    .map(|(i, _)| char::from_digit(i as u32 / 10, 10).unwrap())
                    .collect::<String>()
            );
            eprintln!(
                "[{location}]          ({})",
                results
                    .iter()
                    .enumerate()
                    .map(|(i, _)| char::from_digit(i as u32 % 10, 10).unwrap())
                    .collect::<String>()
            );
            eprintln!(
                "[{location}] result[] ({})",
                aligns.iter().collect::<String>()
            );
            for (i, (k, _)) in tmp.iter().enumerate() {
                match k {
                    Ok(r) => eprintln!("[{location}] result[{i}] = {r:x?}"),
                    Err(msg) => eprintln!("[{location}] result[{i}] = panic: {msg:?}"),
                }
            }
        } else {
            let mut tmp = hm.iter().collect::<Vec<_>>();
            tmp.sort_by_key(|(_, v)| *v);
            for (result, alignments) in tmp {
                eprintln!(
                    "[{location}] aligns {}",
                    (0..63)
                        .map(|i| if alignments.contains(&i) { "#" } else { "." })
                        .collect::<Vec<_>>()
                        .join("")
                );
                eprintln!("[{location}] result = {result:x?}");
            }
        }
        eprintln!();

        false
    }

    #[test]
    fn basic() {
        let mut ok = true;
        ok &= all_alignments("42", &[0x42], &[0]);
        ok &= all_alignments("24", &[0x42], &[]);
        ok &= all_alignments("42", &[0x42, 0x42], &[0, 1]);
        assert!(ok);
    }

    #[test]
    fn leading_wildcard() {
        let mut ok = true;
        ok &= all_alignments("? 42", &[0x42], &[]);
        ok &= all_alignments("? 42", &[0x42, 0x22], &[]);
        ok &= all_alignments("? 42", &[0x22, 0x42], &[0]);
        assert!(ok);
    }

    #[test]
    fn trailing_wildcard() {
        let mut ok = true;
        ok &= all_alignments("42 ?", &[0x42], &[]);
        ok &= all_alignments("42 ?", &[0x42, 0x22], &[0]);
        assert!(ok);
    }

    #[test]
    fn trailing_zero() {
        let mut ok = true;
        ok &= all_alignments("00", &[0x42], &[]);
        ok &= all_alignments("42 00", &[0x42], &[]);
        ok &= all_alignments("00", &[0x00], &[0]);
        ok &= all_alignments("42 00", &[0x42, 0x00], &[0]);
        assert!(ok);
    }

    #[test]
    fn xxh3_data_test() {
        assert_eq!(
            xxh3_data(16).as_slice(),
            &[199, 123, 58, 187, 111, 135, 172, 217, 243, 107, 74, 26, 68, 247, 139, 243]
        );
    }

    #[test]
    fn overlap() {
        let mut ok = true;
        let data = &[0xab, 0xcd, 0xab, 0xcd, 0xab, 0xcd];
        ok &= all_alignments("ab ?? ?? cd", data, &[0, 2]);
        ok &= all_alignments("ab ?? ??", data, &[0, 2]);
        ok &= all_alignments("?? ?? cd", data, &[1, 3]);
        assert!(ok);
    }

    #[test]
    fn repeat_across_buffer() {
        let mut ok = true;
        let mut data = [0_u8; 64];
        data[0] = 1;
        data[1] = 1;
        ok &= all_alignments("01", &data, &[0, 1]);
        assert!(ok);
    }

    #[test]
    fn small() {
        let mut ok = true;
        //    00 01 02 03 04 05 06 07 08 09 0A 0B 0C 0D 0E 0F
        // 0x c7 7b 3a bb 6f 87 ac d9 f3 6b 4a 1a 44 f7 8b f3
        // 1x 3e 69 48 79 79 85 51 1c d0 36 c6 a9 c6 b3 1c 1d
        // 2x 93 47 f2 9a a4 16 00 1e c2 8f 1f 5e 73 70 05 06
        // 3x 4c 14 53 22 e9 63 61 c2 f8 c0 12 6b 89 b4 fa fc
        let data = xxh3_data(64);
        ok &= all_alignments("c7 7b", &data, &[0]);
        ok &= all_alignments("c7 7b ?", &data, &[0]);
        ok &= all_alignments("? c7 7b", &data, &[]);
        ok &= all_alignments("f3", &data, &[0x08, 0x0F]);
        ok &= all_alignments("f3 ? 4a", &data, &[0x08]);
        ok &= all_alignments("f3 ? 69", &data, &[0x0F]);
        ok &= all_alignments("c2", &data, &[0x28, 0x37]);
        ok &= all_alignments("c2 ? ? 5e", &data, &[0x28]);
        ok &= all_alignments("c2 ? ? 12", &data, &[0x37]);
        ok &= all_alignments("14 53 22 e9 63", &data, &[0x31]);

        // uneven tail
        ok &= all_alignments("c2", &data[..=0x37], &[0x28, 0x37]);
        ok &= all_alignments("14 53 22 e9 63", &data[..=0x37], &[0x31]);

        // double
        let data2 = data.repeat(2);
        ok &= all_alignments("c7 7b", &data2, &[0, 64]);
        ok &= all_alignments("c7 7b ?", &data2, &[0, 64]);
        ok &= all_alignments("? c7 7b", &data2, &[63]);
        ok &= all_alignments("f3", &data2, &[0x08, 0x0F, 0x48, 0x4F]);
        ok &= all_alignments("f3 ? 4a", &data2, &[0x08, 0x48]);
        ok &= all_alignments("f3 ? 69", &data2, &[0x0F, 0x4F]);
        ok &= all_alignments("c2", &data2, &[0x28, 0x37, 0x68, 0x77]);
        ok &= all_alignments("c2 ? ? 5e", &data2, &[0x28, 0x68]);
        ok &= all_alignments("c2 ? ? 12", &data2, &[0x37, 0x77]);
        ok &= all_alignments("14 53 22 e9 63", &data2, &[0x31, 0x71]);

        // across block boundary
        ok &= all_alignments("fa fc c7", &data2, &[0x3E]);
        ok &= all_alignments("fc c7 7b", &data2, &[0x3F]);
        ok &= all_alignments("fc ?? 7b", &data2, &[0x3F]);

        // uneven tail
        ok &= all_alignments("c2", &data2[..=0x77], &[0x28, 0x37, 0x68, 0x77]);
        ok &= all_alignments("14 53 22 e9 63", &data2[..=0x77], &[0x31, 0x71]);

        // wildcard beyond the end, 2nd iterator call
        ok &= all_alignments("6b ?? ?? ?? ?? ??", &data2, &[0x09, 0x3B, 0x49]);
        assert!(ok);
    }

    #[test]
    fn medium() {
        let mut ok = true;
        let data = xxh3_data(256);
        // c7 7b 3a bb 6f 87 ac d9 f3 6b 4a 1a 44 f7 8b f3
        // 3e 69 48 79 79 85 51 1c d0 36 c6 a9 c6 b3 1c 1d
        // 93 47 f2 9a a4 16 00 1e c2 8f 1f 5e 73 70 05 06
        // 4c 14 53 22 e9 63 61 c2 f8 c0 12 6b 89 b4 fa fc
        // 6c 21 67 3f 75 92 6a 82 07 ac a3 37 bc 38 e9 8c
        // a5 39 d2 ef 8a 0c 4d 7c d5 70 24 b4 a6 06 1d 82
        // a9 48 a2 a5 d1 12 54 f0 5e 92 75 e4 75 f7 fd f6
        // f2 58 95 64 7d 5e ee b9 cb 87 78 89 3d 73 c3 50
        // 9b 3f a9 34 a5 38 6f ad cc 24 10 47 83 77 a5 cc
        // a7 a9 44 76 4b 57 0c 4a 07 f6 80 e9 21 72 35 f5
        // ac 5b 0e 60 2e a9 fb 55 2b fa ed 4b ec c3 18 62
        // a4 9d fb 52 e1 90 45 fe e3 90 4f d2 fc 7b 02 bc
        // 40 89 6e 4e 66 fe 67 4b 8e 09 cb 44 03 83 4f 0a
        // 67 df 82 f4 0e a2 f2 f2 45 e5 27 b3 6e e5 31 82
        // d8 40 5f e6 f9 7d cc 6f 7d 30 4a 80 db 66 6c b3
        // 85 67 0c a2 5c 6d 82 e0 35 d9 ee 0d 66 ec 03 3f
        ok &= all_alignments("34", &data, &[0x83]);
        ok &= all_alignments("34 a5", &data, &[0x83]);
        ok &= all_alignments("34 a5 38", &data, &[0x83]);
        assert!(ok);
    }
}
