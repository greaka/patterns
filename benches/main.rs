use criterion::{criterion_group, criterion_main, Bencher, Criterion};
use patterns::Pattern;

const PLAIN_PATTERN: &str = "01 01 01 01 01 01 01 01";
const WILDCARD_PATTERN: &str = "01 01 ?? 01 . 01 01 01";
const WILDCARD_PREFIX_PATTERN: &str = "? ? ?. 01 01 01 01 01";

fn avx(b: &mut Bencher, pattern: &Pattern, data: &[u8]) {
    b.iter(move || {
        let mut buffer = [0; 128];
        pattern.matches(data, &mut buffer).next().unwrap();
    });
}

fn criterion_benchmark(c: &mut Criterion) {
    let mut data = vec![0; 1_000_000];
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

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
