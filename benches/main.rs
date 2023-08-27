use aligned_vec::{avec, AVec};
use criterion::{criterion_group, criterion_main, Bencher, Criterion};
use patterns::Pattern;
use xxhash_rust::xxh3;

// duplicated in src/tests.rs
pub fn xxh3_data(length: usize) -> AVec<u8> {
    AVec::<u8>::from_iter(
        64,
        (0..length.div_ceil(8))
            .flat_map(|i| xxh3::xxh3_64(&i.to_be_bytes()).to_be_bytes())
            .take(length),
    )
}

const PLAIN_PATTERN: &str = "01 01 01 01 01 01 01 01";
const WILDCARD_PATTERN: &str = "01 01 ?? 01 . 01 01 01";
const WILDCARD_PREFIX_PATTERN: &str = "? ? ?. 01 01 01 01 01";

fn avx(b: &mut Bencher, pattern: &Pattern, data: &[u8]) {
    b.iter(move || {
        pattern.matches(data).next().unwrap();
    });
}

#[allow(unused)]
fn xxh_alignment(c: &mut Criterion) {
    let data = xxh3_data(1_000_032);
    // at position 500_000
    #[rustfmt::skip]
    let mid_1: Pattern = r#"e8 22 77 4d 4b 54 96 10 08 b7 61 e5 d6 54 94 5d e0 b0 c0 32 90 ec 85 c0 78 f3 43 2b"#.parse().unwrap();

    let mut group = c.benchmark_group("xxh_align");
    for offset in 0..16usize {
        group.bench_with_input(
            criterion::BenchmarkId::from_parameter(offset),
            &offset,
            |b, i: &usize| {
                let data = &data[offset..(data.len() - (64 - offset))];
                assert_eq!(data.len(), 1_000_032 - 64);
                avx(b, &mid_1, data);
            },
        );
    }

    group.finish();
}

#[rustfmt::skip]
fn xxh_benchmark(c: &mut Criterion) {
    let data = xxh3_data(1_000_032);
    // at position 500_000
    let mid_1: Pattern = r#"e8 22 77 4d 4b 54 96 10 08 b7 61 e5 d6 54 94 5d e0 b0 c0 32 90 ec 85 c0 78 f3 43 2b"#.parse().unwrap();
    let mid_2: Pattern = r#"e8 ?? ?? 4d 4b 54 96 10 ?? ?? ?? ?? d6 54 94 5d e0 b0 c0 32 90 ec ?? ?? ?? f3 43 2b"#.parse().unwrap();
    // at position 999_950
    let late_1: Pattern = r#"19 4a 69 d9 bf 6a 04 76 5d 06 4f cc 40 2d f3 9b b1 3b 70 53 87 91 39 e0 85 b1 a7 92"#.parse().unwrap();
    // starts inside last 32 bytes
    let tail_1: Pattern = r#"e2 f4 b7 0f eb 75 06 cf e0 54 92 0e e9 20 cb cc 89 39 e7 a9 1f 8e 0a 39 0d 71 d4 68"#.parse().unwrap();
    let tail_2: Pattern = r#"e2 ?? ?? 0f eb ?? ?? ?? e0 54 92 0e e9 20 ?? ?? 89 39 e7 a9 1f 8e ?? 39 0d 71 d4 68"#.parse().unwrap();

    c.bench_function("xxh_mid", |b| avx(b, &mid_1, &data));
    c.bench_function("xxh_mid_wildcard", |b| avx(b, &mid_2, &data));
    c.bench_function("xxh_late", |b| avx(b, &late_1, &data));
    c.bench_function("xxh_tail", |b| avx(b, &tail_1, &data));
    c.bench_function("xxh_tail_wildcard", |b| avx(b, &tail_2, &data));
}

fn trivial_benchmark(c: &mut Criterion) {
    let mut data = avec![0; 1_000_000];
    let len = data.len();
    data[len - 8..].fill(1);
    let plain_pattern: Pattern = PLAIN_PATTERN.parse().unwrap();
    let wildcard_pattern: Pattern = WILDCARD_PATTERN.parse().unwrap();
    let wildcard_prefix_pattern: Pattern = WILDCARD_PREFIX_PATTERN.parse().unwrap();

    c.bench_function("avx_plain", |b| avx(b, &plain_pattern, &data));
    c.bench_function("avx_wildcard", |b| avx(b, &wildcard_pattern, &data));
    c.bench_function("avx_wildcard_prefix", |b| {
        avx(b, &wildcard_prefix_pattern, &data)
    });
}

criterion_group!(align, xxh_alignment);
criterion_group!(benches, xxh_benchmark, trivial_benchmark);
criterion_main!(benches, align);
