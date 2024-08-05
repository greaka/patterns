use arbitrary::{Arbitrary, Result, Unstructured};

#[derive(Debug, Clone)]
pub struct PatternStr(pub String);
pub const BYTES: usize = 64;

fn u8_to_bits(n: &u8) -> [bool; 8] {
    let mut ret = [false; 8];
    for (i, ri) in ret.iter_mut().enumerate() {
        *ri = (n >> i) & 1 == 1;
    }
    ret
}

impl<'a> Arbitrary<'a> for PatternStr {
    fn size_hint(_depth: usize) -> (usize, Option<usize>) {
        // len + bytes + optional mask bits
        (1 + BYTES, Some(1 + BYTES + ((BYTES + 7) / 8)))
    }

    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        let overall_len: u8 = u.int_in_range(1u8..=BYTES as u8 / 8)?;

        let bytes = u.bytes(overall_len as usize)?;

        let mut mask = u
            .bytes((overall_len as usize + 7) / 8)
            .map(|i| i.iter().flat_map(u8_to_bits).collect::<Vec<bool>>())
            .unwrap_or_else(|_| std::iter::repeat(true).take(overall_len as usize).collect());

        if mask.iter().take(overall_len as usize).all(|m| !m) {
            // mask is all-false, flip it
            mask.iter_mut().for_each(|m| *m = true);
        }

        Ok(PatternStr(
            bytes
                .iter()
                .zip(mask)
                .map(|(byte, mask)| {
                    if mask {
                        format!("{byte:02x} ")
                    } else {
                        "? ".to_string()
                    }
                })
                .collect::<String>()
                .trim_end()
                .to_string(),
        ))
    }
}
