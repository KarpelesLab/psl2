# compare-psl

A standalone, **workspace-excluded** crate that benchmarks `psl2` against the
original [`psl`](https://crates.io/crates/psl) crate.

`psl` is kept out of `psl2`'s own dependency graph on purpose: it embeds the
Public Suffix List as ~2.5 MB / ~100k lines of generated `match` code, which is
exactly the compile cost `psl2` avoids. Pulling it in even as a dev-dependency
would tax every `cargo test`/`cargo bench` of the library, so it lives here and
only builds when you explicitly run the comparison:

```sh
cargo bench --manifest-path compare-psl/Cargo.toml
```

## What is measured

The shared operation: an allocation-free, zero-copy lookup of an
already-lowercased ASCII/punycode host. Two operations (`suffix`, `domain`)
across three front-ends:

- `psl2_lookup` — `psl2::lookup` (the native `&str` API; validates input).
- `psl2_compat` — `psl2::compat::*` (the `psl`-shaped `&[u8]` API).
- `psl` — `psl::suffix_str` / `psl::domain_str`.

## Results (indicative)

Absolute numbers vary by machine; ratios are the point. On one x86-64 dev
machine, median per-lookup times (psl2 with the default `fast-lookup` index):

| case (domain) | `psl2::lookup` | `psl` | `psl` faster by |
| --- | --- | --- | --- |
| tld (`com`) | ~23 ns | ~3.7 ns | ~6× |
| typical (`www.example.co.uk`) | ~44 ns | ~16 ns | ~2.8× |
| deep (`a.b.c.d.e.f.example.co.uk`) | ~50 ns | ~16 ns | ~3.1× |

**`psl` is still faster per lookup** — roughly 3–6× — because its list is
compiled straight to branch/jump-table code with no memory indirection,
whereas `psl2` walks a binary trie. But `psl2`'s `fast-lookup` prefix index
(default) closed most of the original ~7× gap. Without `fast-lookup`
(`default-features = false`), `psl2` is roughly 2× slower again, trading lookup
speed for ~42 KB less rodata.

In absolute terms `psl2` is ~20–50 ns/lookup (~20M+ lookups/sec on one core),
negligible for typical cookie/URL work. The tradeoff `psl2` makes is
deliberate: **much cheaper compiles, built-in IDNA, and a clean `&str` API,
for a few × on raw lookup latency.** If single-lookup latency is your
bottleneck and you can absorb the compile cost, `psl` is faster.
