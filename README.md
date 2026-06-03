# psl2

[![crates.io](https://img.shields.io/crates/v/psl2.svg)](https://crates.io/crates/psl2)
[![docs.rs](https://img.shields.io/docsrs/psl2)](https://docs.rs/psl2)
[![CI](https://github.com/KarpelesLab/psl2/actions/workflows/ci.yml/badge.svg)](https://github.com/KarpelesLab/psl2/actions/workflows/ci.yml)

A modern alternative to the [`psl`] crate for working with Mozilla's
[Public Suffix List].

`psl2` tells you, reliably, whether a hostname is a **registrable domain** (one
that can own cookies — e.g. `example.co.uk`) or a **public suffix** (an
"extension" under which names are registered — e.g. `co.uk`).

```rust
// The public suffix ("effective TLD"):
assert_eq!(psl2::suffix("www.example.co.uk").as_deref(), Some("co.uk"));

// The registrable domain (eTLD + 1) — the cookie domain:
assert_eq!(
    psl2::registrable_domain("www.example.co.uk").as_deref(),
    Some("example.co.uk"),
);

// A bare public suffix has no registrable domain:
assert_eq!(psl2::registrable_domain("co.uk"), None);
assert!(psl2::is_public_suffix("co.uk"));
```

Internationalized domains work out of the box (inputs and outputs are
normalized to ASCII/punycode):

```rust
assert_eq!(
    psl2::registrable_domain("食狮.公司.cn").as_deref(),
    Some("xn--85x722f.xn--55qx5d.cn"),
);
```

## `no_std` / `no_alloc`

The crate is `#![no_std]`, and its core lookup **allocates nothing**. Pass an
already-lowercased ASCII/punycode hostname to `lookup` and read back borrowed
slices — this works on bare-metal targets with no allocator:

```rust
let d = psl2::lookup("www.example.co.uk").unwrap();
assert_eq!(d.suffix(), "co.uk");
assert_eq!(d.registrable_domain(), Some("example.co.uk"));
assert_eq!(d.subdomain(), Some("www"));
```

For embedded / allocator-free builds, turn the default features off:

```toml
[dependencies]
psl2 = { version = "0.1", default-features = false }
```

With no features, you still get a borrowing [`Domain`] object via `lookup` —
plus the [`compat`](#migrating-from-the-psl-crate) module's `domain`/`suffix`
helpers over `&[u8]`. This is the same allocator-free, zero-copy capability the
[`psl`] crate offers by default (and a *lighter compile*, since `psl2` embeds
the list as a data trie rather than ~100k lines of generated `match` code), so
the bare core is a near drop-in for `psl` on `no_std` targets. The only
`alloc`-gated items are the ones that must own or normalize: the `Info` struct
and the `String`-returning `analyze`/`suffix`/… convenience functions.

[`Domain`]: https://docs.rs/psl2/latest/psl2/struct.Domain.html

## Why another crate?

The existing `psl` and `publicsuffix` crates work, but have rough edges. `psl2`
is designed around a few principles:

- **Fast builds.** The list is normalized to ASCII **at publish time** and
  embedded as a flattened **reversed-label trie** via `include_bytes!`, walked
  from the TLD inward with no allocation. There is no `build.rs`, no
  procedural-macro codegen, and no per-build list processing, so adding `psl2`
  costs almost nothing in compile time — a few hundred lines of code plus an
  opaque data blob, versus the ~100k lines of generated `match` code the
  [`psl`] crate makes every consumer compile.
- **Lookups in the ~45–95 ns range**, allocation-free and stable across
  hostname depth. The embedded blobs are decoded into native node/edge arrays
  **at compile time**, so a lookup reads native fields with no byte assembly or
  endian conversion — performance is identical on little- and big-endian
  targets. This is plenty for typical cookie/URL work (~15M lookups/s per
  core), but note `psl2` does *not* aim to beat `psl` on raw latency: `psl`
  compiles the list straight to branch code and is still a few times faster per
  lookup. `psl2` trades that for far cheaper compiles, built-in IDNA, and a
  clean API. (See `compare-psl/` for the head-to-head benchmark.)
- **`no_std` + `no_alloc` core**, usable on embedded targets.
- **Built-in IDNA.** You pass a `&str` hostname — Unicode or not — and `psl2`
  normalizes it for you. No need to punycode-encode input yourself.
- **Clean, explicit API** over `&str`, with ICANN / private / unknown
  classification exposed.
- **Always current.** A scheduled GitHub Action republishes the crate whenever
  the upstream list changes. The bundled list version is available at runtime
  via [`psl2::psl_version()`](https://docs.rs/psl2/latest/psl2/fn.psl_version.html).

## API

Allocation-free core (always available):

| Function | Returns |
| --- | --- |
| `lookup(host) -> Option<Domain>` | Borrowing analysis of a **pre-normalized** host |
| `psl_version() -> &'static str` | The bundled PSL version |

[`Domain`](https://docs.rs/psl2/latest/psl2/struct.Domain.html) exposes
`suffix()`, `registrable_domain()`, `subdomain()`, `is_public_suffix()`,
`typ()`, `is_icann()`, `is_private()`, `is_known()`, and `as_str()` — all
returning borrowed `&str`.

Allocating convenience (requires `alloc`, on by default; normalizes any input):

| Function | Returns |
| --- | --- |
| `analyze(host) -> Option<Info>` | Full analysis with owned normalized form |
| `suffix(host) -> Option<String>` | The public suffix |
| `registrable_domain(host) -> Option<String>` | The eTLD+1 (cookie domain) |
| `subdomain(host) -> Option<String>` | The labels left of the registrable domain |
| `is_public_suffix(host) -> bool` | Whether `host` is itself a public suffix |

`lookup` is the zero-allocation path; prefer it on hot paths when your input is
already lowercase ASCII/punycode.

## Features

- `std` *(default)* — currently just implies `alloc`.
- `alloc` *(default, via `std`)* — the allocating convenience API above.
- `idna` *(default)* — accept Unicode/IDN input via the [`idna`] crate (implies
  `alloc`). Without it, `alloc` input must be ASCII/punycode (lowercased for
  you); non-ASCII returns `None`.

With **no features**, only the allocation-free core (`lookup`, `Domain`,
`Type`, `psl_version`) is compiled.

## Migrating from the `psl` crate

The `compat` module mirrors the [`psl`] crate's shape (operates on `&[u8]`,
returns a `Domain` that *is* the registrable domain with a nested `Suffix`):

```rust
let d = psl2::compat::domain_str("www.example.co.uk").unwrap();
assert_eq!(d.as_bytes(), b"example.co.uk");
assert_eq!(d.suffix().as_bytes(), b"co.uk");
assert!(d.suffix().is_known());
```

| `psl` | `psl2::compat` |
| --- | --- |
| `psl::domain_str(s)` | `psl2::compat::domain_str(s)` |
| `psl::suffix_str(s)` | `psl2::compat::suffix_str(s)` |
| `psl::domain(b)` / `psl::suffix(b)` | `psl2::compat::domain(b)` / `suffix(b)` |

Like `psl`, this path is allocation-free, borrows from its input, and is
case-sensitive (expects lowercased ASCII/punycode). For Unicode/auto-normalized
input, use the main `analyze`/`registrable_domain` API.

## ICANN vs. PRIVATE

The list has two sections: ICANN (real registry suffixes) and PRIVATE
(suffixes delegated by organizations, e.g. `github.io`, `s3.amazonaws.com`,
`blogspot.com`). Both are honored by default, matching browser cookie behavior.

This means some names you might not expect are public suffixes — e.g.
`registrable_domain("blogspot.com")` is `None`, and the registrable domain of
`foo.blogspot.com` is `foo.blogspot.com` itself. This is intentional (diverging
from the PSL would be worse); use `is_icann()` / `is_private()` on `Domain` or
`Info` to tell the sections apart when it matters.

## MSRV

Rust **1.86**, set by the `idna` dependency tree (the `icu` crates). The
allocation-free core (`default-features = false`) builds on much older Rust.

## License

The `psl2` source code is dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

The **bundled Public Suffix List data** (`src/rules.txt`, `src/wildcards.txt`,
`src/exceptions.txt`, derived from `public_suffix_list.dat`) is © the Mozilla
Foundation and distributed under the [Mozilla Public License v2.0][MPL]. It is
included unmodified in substance, only re-encoded for efficient lookup.

[`psl`]: https://crates.io/crates/psl
[`idna`]: https://crates.io/crates/idna
[Public Suffix List]: https://publicsuffix.org/
[MPL]: https://mozilla.org/MPL/2.0/
