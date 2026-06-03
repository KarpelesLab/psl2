//! Benchmarks for psl2.
//!
//! Run with `cargo bench`. The `lookup/*` cases measure the allocation-free
//! core; the `registrable_domain/*` cases measure the allocating + IDNA path.

use std::hint::black_box;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion};

fn core_lookup(c: &mut Criterion) {
    let mut g = c.benchmark_group("lookup");
    let cases = [
        ("tld", "com"),
        ("shallow", "example.com"),
        ("typical", "www.example.co.uk"),
        ("deep", "a.b.c.d.e.f.example.co.uk"),
        ("wildcard", "city.kobe.jp"),
        ("exception", "www.ck"),
        ("unknown", "foo.someunknownnonexistenttld"),
        ("punycode", "www.xn--85x722f.xn--55qx5d.cn"),
    ];
    for (name, host) in cases {
        g.bench_function(name, |b| {
            b.iter(|| black_box(psl2::lookup(black_box(host))))
        });
    }
    g.finish();
}

#[cfg(feature = "alloc")]
fn alloc_path(c: &mut Criterion) {
    let mut g = c.benchmark_group("registrable_domain");
    g.bench_function("ascii", |b| {
        b.iter(|| black_box(psl2::registrable_domain(black_box("www.example.co.uk"))))
    });
    g.bench_function("mixed_case", |b| {
        b.iter(|| black_box(psl2::registrable_domain(black_box("WwW.Example.CO.UK"))))
    });
    #[cfg(feature = "idna")]
    g.bench_function("unicode_idna", |b| {
        b.iter(|| black_box(psl2::registrable_domain(black_box("www.食狮.公司.cn"))))
    });
    g.finish();
}

#[cfg(not(feature = "alloc"))]
fn alloc_path(_c: &mut Criterion) {}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(2));
    targets = core_lookup, alloc_path
}
criterion_main!(benches);
