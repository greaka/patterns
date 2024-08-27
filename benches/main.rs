#![feature(portable_simd)]

use std::{
    cmp::max,
    simd::{LaneCount, SupportedLaneCount},
};

use aligned_vec::{AVec, ConstAlign};
use criterion::{criterion_group, criterion_main, Bencher, Criterion};
use patterns::{Pattern, Scanner};

const PLAIN_PATTERN: &str = "01 01 01 01 01 01 01 01";
const WILDCARD_PATTERN: &str = "01 01 ?? 01 . 01 01 01";
const WILDCARD_PREFIX_PATTERN: &str = "? ? ?. 01 01 01 01 01";

fn avx<const ALIGN: usize, const BYTES: usize>(
    b: &mut Bencher,
    pattern: &Pattern<ALIGN, BYTES>,
    data: &[u8],
) where
    LaneCount<ALIGN>: SupportedLaneCount,
    LaneCount<BYTES>: SupportedLaneCount,
{
    b.iter(move || {
        pattern.matches(data).next().unwrap();
    });
}

fn scanner<const ALIGN: usize, const BYTES: usize>(b: &mut Bencher, scanner: &Scanner<ALIGN, BYTES>)
where
    LaneCount<ALIGN>: SupportedLaneCount,
    LaneCount<BYTES>: SupportedLaneCount,
{
    b.iter(move || {
        let mut scanner = scanner.clone();
        scanner.next().unwrap();
    });
}

fn criterion_benchmark(c: &mut Criterion) {
    const ALIGN: usize = 1;
    const BYTES: usize = 64;
    let mut data: AVec<u8, ConstAlign<ALIGN>> = AVec::new(ALIGN);
    data.resize(1_000_000, 0);
    let len = data.len();
    let offset = max(ALIGN, 8);
    data[len - offset..].fill(1);
    let plain_pattern: Pattern<ALIGN, BYTES> = Pattern::new(PLAIN_PATTERN);
    let wildcard_pattern: Pattern<ALIGN, BYTES> = Pattern::new(WILDCARD_PATTERN);
    let wildcard_prefix_pattern: Pattern<ALIGN, BYTES> = Pattern::new(WILDCARD_PREFIX_PATTERN);
    let plain_scanner = plain_pattern.matches(&data);
    let wildcard_scanner = wildcard_pattern.matches(&data);
    let wildcard_prefix_scanner = wildcard_prefix_pattern.matches(&data);

    c.bench_function("avx_plain", |b| avx(b, &plain_pattern, &data));
    c.bench_function("avx_wildcard", |b| avx(b, &wildcard_pattern, &data));
    c.bench_function("avx_wildcard_prefix", |b| {
        avx(b, &wildcard_prefix_pattern, &data)
    });
    c.bench_function("scanner_plain", |b| scanner(b, &plain_scanner));
    c.bench_function("scanner_wildcard", |b| scanner(b, &wildcard_scanner));
    c.bench_function("scanner_wildcard_prefix", |b| {
        scanner(b, &wildcard_prefix_scanner)
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
