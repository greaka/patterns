#![no_main]

use std::fmt::Debug;

use libfuzzer_sys::{
    arbitrary::{Arbitrary, Unstructured},
    fuzz_target, Corpus,
};
use reference::PatternStr;

#[derive(Debug)]
struct FuzzData(String, Vec<u8>, u8);
impl<'a> Arbitrary<'a> for FuzzData {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let pat = PatternStr::arbitrary(u)?.0;
        let misalign = u.int_in_range(0..=63).unwrap_or(0);
        let data = Vec::<u8>::arbitrary(u)?;
        if data.is_empty() {
            return Err(arbitrary::Error::NotEnoughData);
        }
        Ok(Self(pat, data, misalign))
    }

    fn arbitrary_take_rest(mut u: Unstructured<'a>) -> arbitrary::Result<Self> {
        let pat = PatternStr::arbitrary(&mut u)?.0;
        let misalign = u.int_in_range(0..=63).unwrap_or(0);
        let data = Vec::<u8>::arbitrary_take_rest(u)?;
        if data.is_empty() {
            return Err(arbitrary::Error::NotEnoughData);
        }
        Ok(Self(pat, data, misalign))
    }
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

fn hex_list<T: std::fmt::LowerHex>(list: &[T]) -> String {
    format!(
        "[{}]",
        list.iter()
            .map(|i| format!("0x{i:02x}"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[rustfmt::skip]
fn repro(input: &FuzzData, alignment: u8, bytes: u8, reference: &[usize]) -> String {
    let FuzzData(ref patstr, ref data, misalign) = input;

    use std::io::Write;
    let mut buf = vec![];
    let _ = writeln!(buf, "#[test]");
    let _ = writeln!(buf, "fn repro() {{");
    let _ = writeln!(buf, "    let pat = Pattern::<{alignment}, {bytes}>::from_str({:?}).unwrap();", patstr);
    let _ = writeln!(buf, "    with_misaligned(&{}, {}, |data| {{", hex_list(data), misalign);
    let _ = writeln!(buf, "        assert_eq!(pat.matches(data).collect::<Vec<_>>(), &{:?})", reference);
    let _ = writeln!(buf, "    }});");
    let _ = writeln!(buf, "}}");
    String::from_utf8_lossy(&buf).into_owned()
}

fuzz_target!(|input: FuzzData| -> Corpus {
    let FuzzData(ref patstr, ref data, misalign) = input;

    let pattern_length = reference::Pattern::from_str(patstr, 1)
        .expect("reference impl can parse pattern")
        .pattern
        .len();

    with_misaligned(data, misalign as usize, |data| {
        macro_rules! fuzz_alignment {
            ($align:literal, $bytes:literal) => {{
                if pattern_length <= $bytes {
                    let rpat = reference::Pattern::from_str(&patstr, $align)
                        .expect("reference impl can parse pattern");
                    let spat = patterns::Pattern::<$align, $bytes>::from_str(&patstr)
                        .expect("simd impl can parse pattern");

                    let reference = rpat.matches(&data).collect::<Vec<_>>();
                    let simd = spat.matches(&data).collect::<Vec<_>>();
                    assert!(
                        simd == reference,
                        "SIMD-vs-reference mismatch at alignment={}\n simd: {:?}\n  ref: \
                         {:?}\n\n{}\n",
                        $align,
                        &simd,
                        &reference,
                        repro(&input, $align, $bytes, &reference)
                    );
                }
            }};
        }

        fuzz_alignment!(1, 64);
        fuzz_alignment!(2, 64);
        fuzz_alignment!(4, 64);
        fuzz_alignment!(8, 64);
        fuzz_alignment!(16, 64);
        fuzz_alignment!(32, 64);
        fuzz_alignment!(64, 64);

        fuzz_alignment!(1, 32);
        fuzz_alignment!(2, 32);
        fuzz_alignment!(4, 32);
        fuzz_alignment!(8, 32);
        fuzz_alignment!(16, 32);
        fuzz_alignment!(32, 32);

        fuzz_alignment!(1, 16);
        fuzz_alignment!(2, 16);
        fuzz_alignment!(4, 16);
        fuzz_alignment!(8, 16);
        fuzz_alignment!(16, 16);
    });

    Corpus::Keep
});
