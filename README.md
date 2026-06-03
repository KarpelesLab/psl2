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

## Why another crate?

The existing `psl` and `publicsuffix` crates work, but have rough edges. `psl2`
is designed around a few principles:

- **Fast builds, no `build.rs`.** The list is normalized to ASCII **at publish
  time** and embedded as plain data via `include_str!`. There is no
  procedural-macro codegen and no per-build list processing, so adding `psl2`
  to your dependency tree costs almost nothing in compile time.
- **Built-in IDNA.** You pass a `&str` hostname — Unicode or not — and `psl2`
  normalizes it for you. No need to punycode-encode input yourself.
- **Clean, explicit API** over `&str`, with ICANN / private / unknown
  classification exposed.
- **Always current.** A scheduled GitHub Action republishes the crate whenever
  the upstream list changes. The bundled list version is available at runtime
  via [`psl2::psl_version()`](https://docs.rs/psl2/latest/psl2/fn.psl_version.html).

## API

| Function | Returns |
| --- | --- |
| `analyze(host) -> Option<Info>` | Full analysis (zero-copy accessors) |
| `suffix(host) -> Option<String>` | The public suffix |
| `registrable_domain(host) -> Option<String>` | The eTLD+1 (cookie domain) |
| `subdomain(host) -> Option<String>` | The labels left of the registrable domain |
| `is_public_suffix(host) -> bool` | Whether `host` is itself a public suffix |
| `psl_version() -> &'static str` | The bundled PSL version |

[`Info`](https://docs.rs/psl2/latest/psl2/struct.Info.html) additionally
exposes `subdomain()`, `is_icann()`, `is_private()`, `is_known()`, and
`as_ascii()`.

`analyze` is the zero-allocation path: its accessors return slices into the
normalized hostname, so prefer it on hot paths.

## Features

- `idna` *(default)* — accept Unicode/IDN input via the [`idna`] crate. Disable
  it (`default-features = false`) if you only ever pass ASCII/punycode
  hostnames and want a leaner build; non-ASCII input then returns `None`.

## ICANN vs. PRIVATE

The list has two sections: ICANN (real registry suffixes) and PRIVATE
(suffixes delegated by organizations, e.g. `github.io`, `s3.amazonaws.com`,
`blogspot.com`). Both are honored by default, matching browser cookie behavior.

This means some names you might not expect are public suffixes — e.g.
`registrable_domain("blogspot.com")` is `None`, and the registrable domain of
`foo.blogspot.com` is `foo.blogspot.com` itself. This is intentional (diverging
from the PSL would be worse); use `Info::is_icann()` / `Info::is_private()` to
tell the sections apart when it matters.

## MSRV

Rust 1.70 (for `std::sync::OnceLock`).

## License

The `psl2` source code is dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

The **bundled Public Suffix List data** (`src/list.txt`, derived from
`public_suffix_list.dat`) is © the Mozilla Foundation and distributed under the
[Mozilla Public License v2.0][MPL]. It is included unmodified in substance,
only re-encoded for efficient lookup.

[`psl`]: https://crates.io/crates/psl
[`idna`]: https://crates.io/crates/idna
[Public Suffix List]: https://publicsuffix.org/
[MPL]: https://mozilla.org/MPL/2.0/
