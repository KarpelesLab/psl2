//! Compares psl2's no-alloc, index-driven binary search against the same
//! lookup algorithm backed by Rust's native `HashMap` / `BTreeMap`.
//!
//! This isolates the *data-structure* cost. Note the native structures must be
//! built at startup (allocating, `std`-only); psl2 needs no construction at all
//! and works in `no_std` + `no_alloc`. Run with `cargo bench --bench vs_native`.

use std::collections::{BTreeMap, HashMap};
use std::hint::black_box;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion};

const RULES: &str = include_str!("../src/rules.txt");
const WILDCARDS: &str = include_str!("../src/wildcards.txt");
const EXCEPTIONS: &str = include_str!("../src/exceptions.txt");
const MAX_RULE_LABELS: usize = 7;

const HOSTS: &[&str] = &[
    "com",
    "example.com",
    "www.example.co.uk",
    "a.b.c.d.e.f.example.co.uk",
    "city.kobe.jp",
    "www.ck",
    "foo.someunknownnonexistenttld",
    "www.xn--85x722f.xn--55qx5d.cn",
];

fn rule(line: &'static str) -> &'static str {
    &line[..line.find('\t').unwrap()]
}

/// The same prevailing-rule algorithm psl2 uses, parameterized over a
/// membership predicate, so HashMap and BTreeMap share one implementation.
fn registrable(host: &str, contains: impl Fn(&str, Kind) -> bool) -> Option<&str> {
    let bytes = host.as_bytes();
    let mut offs = [0usize; 128];
    let mut n = 1usize;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'.' {
            offs[n] = i + 1;
            n += 1;
        }
    }
    let (mut best, mut exception): (Option<usize>, Option<usize>) = (None, None);
    let start = n.saturating_sub(MAX_RULE_LABELS);
    for i in start..n {
        let cand = &host[offs[i]..];
        let rl = n - i;
        if contains(cand, Kind::Exception) && exception.is_none_or(|c| rl > c) {
            exception = Some(rl);
        }
        if contains(cand, Kind::Rule) && best.is_none_or(|c| rl > c) {
            best = Some(rl);
        }
        if i + 1 < n
            && contains(&host[offs[i + 1]..], Kind::Wildcard)
            && best.is_none_or(|c| rl > c)
        {
            best = Some(rl);
        }
    }
    let suffix_labels = exception.map(|c| c - 1).or(best).unwrap_or(1);
    let suffix_idx = n - suffix_labels;
    (suffix_idx >= 1).then(|| &host[offs[suffix_idx - 1]..])
}

#[derive(Clone, Copy)]
enum Kind {
    Rule,
    Wildcard,
    Exception,
}

fn bench(c: &mut Criterion) {
    // Build the native structures once (this construction cost is *not* timed,
    // but psl2 pays none of it).
    let h_rules: HashMap<&str, ()> = RULES.lines().map(|l| (rule(l), ())).collect();
    let h_wild: HashMap<&str, ()> = WILDCARDS.lines().map(|l| (rule(l), ())).collect();
    let h_exc: HashMap<&str, ()> = EXCEPTIONS.lines().map(|l| (rule(l), ())).collect();
    let b_rules: BTreeMap<&str, ()> = RULES.lines().map(|l| (rule(l), ())).collect();
    let b_wild: BTreeMap<&str, ()> = WILDCARDS.lines().map(|l| (rule(l), ())).collect();
    let b_exc: BTreeMap<&str, ()> = EXCEPTIONS.lines().map(|l| (rule(l), ())).collect();

    // One-time construction cost the native structures pay at startup (psl2
    // pays none — it is ready at compile time).
    let mut cg = c.benchmark_group("construct_rules_map");
    cg.bench_function("hashmap", |b| {
        b.iter(|| {
            black_box(
                RULES
                    .lines()
                    .map(|l| (rule(l), ()))
                    .collect::<HashMap<_, _>>(),
            )
        })
    });
    cg.bench_function("btreemap", |b| {
        b.iter(|| {
            black_box(
                RULES
                    .lines()
                    .map(|l| (rule(l), ()))
                    .collect::<BTreeMap<_, _>>(),
            )
        })
    });
    cg.finish();

    let mut g = c.benchmark_group("vs_native");
    for &host in HOSTS {
        g.bench_with_input(format!("psl2/{host}"), &host, |b, &h| {
            b.iter(|| black_box(psl2::lookup(black_box(h)).and_then(|d| d.registrable_domain())))
        });
        g.bench_with_input(format!("hashmap/{host}"), &host, |b, &h| {
            b.iter(|| {
                black_box(registrable(black_box(h), |k, kind| match kind {
                    Kind::Rule => h_rules.contains_key(k),
                    Kind::Wildcard => h_wild.contains_key(k),
                    Kind::Exception => h_exc.contains_key(k),
                }))
            })
        });
        g.bench_with_input(format!("btreemap/{host}"), &host, |b, &h| {
            b.iter(|| {
                black_box(registrable(black_box(h), |k, kind| match kind {
                    Kind::Rule => b_rules.contains_key(k),
                    Kind::Wildcard => b_wild.contains_key(k),
                    Kind::Exception => b_exc.contains_key(k),
                }))
            })
        });
    }
    g.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_millis(400))
        .measurement_time(Duration::from_secs(2));
    targets = bench
}
criterion_main!(benches);
