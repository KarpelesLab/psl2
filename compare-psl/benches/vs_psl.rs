//! Head-to-head lookup latency: `psl2` vs the original `psl` crate.
//!
//! Run with `cargo bench --manifest-path compare-psl/Cargo.toml`.
//!
//! Both crates are compared on the operation they share: an allocation-free,
//! zero-copy lookup of an already-lowercased ASCII/punycode host. We measure
//! two equivalent operations across both:
//!
//! * **suffix** — the public suffix (eTLD).
//! * **domain** — the registrable domain (eTLD+1).
//!
//! For `psl2` two front-ends are timed: `compat` (the byte-oriented,
//! `psl`-shaped API) and the native `lookup` (`&str`). `psl` is driven through
//! its `suffix_str` / `domain_str` helpers. This isolates the data-structure
//! cost — `psl2`'s flattened binary trie vs `psl`'s compiled `match` tree — not
//! parsing or allocation, since neither does either here.

use std::hint::black_box;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

/// Representative hosts spanning shapes that stress the algorithms differently:
/// bare TLD, shallow/typical/deep names, a wildcard rule (`*.kobe.jp`), an
/// exception rule (`!city.kobe.jp` -> via `www.ck`), an unknown TLD (implicit
/// `*`), and a punycode IDN.
const CASES: &[(&str, &str)] = &[
    ("tld", "com"),
    ("shallow", "example.com"),
    ("typical", "www.example.co.uk"),
    ("deep", "a.b.c.d.e.f.example.co.uk"),
    ("wildcard", "city.kobe.jp"),
    ("exception", "www.ck"),
    ("unknown", "foo.someunknownnonexistenttld"),
    ("punycode", "www.xn--85x722f.xn--55qx5d.cn"),
];

fn suffix(c: &mut Criterion) {
    let mut g = c.benchmark_group("suffix");
    for &(name, host) in CASES {
        g.bench_with_input(BenchmarkId::new("psl2_compat", name), &host, |b, &h| {
            b.iter(|| black_box(psl2::compat::suffix_str(black_box(h))))
        });
        g.bench_with_input(BenchmarkId::new("psl2_lookup", name), &host, |b, &h| {
            b.iter(|| black_box(psl2::lookup(black_box(h)).map(|d| d.suffix())))
        });
        g.bench_with_input(BenchmarkId::new("psl", name), &host, |b, &h| {
            b.iter(|| black_box(psl::suffix_str(black_box(h))))
        });
    }
    g.finish();
}

fn domain(c: &mut Criterion) {
    let mut g = c.benchmark_group("domain");
    for &(name, host) in CASES {
        g.bench_with_input(BenchmarkId::new("psl2_compat", name), &host, |b, &h| {
            b.iter(|| black_box(psl2::compat::domain_str(black_box(h))))
        });
        g.bench_with_input(BenchmarkId::new("psl2_lookup", name), &host, |b, &h| {
            b.iter(|| black_box(psl2::lookup(black_box(h)).and_then(|d| d.registrable_domain())))
        });
        g.bench_with_input(BenchmarkId::new("psl", name), &host, |b, &h| {
            b.iter(|| black_box(psl::domain_str(black_box(h))))
        });
    }
    g.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(2));
    targets = suffix, domain
}
criterion_main!(benches);
