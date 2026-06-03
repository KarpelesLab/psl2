//! `psl2` — a modern alternative to the [`psl`] crate for working with
//! Mozilla's [Public Suffix List].
//!
//! It answers the practical question *"is this hostname a registrable domain
//! (one that can own cookies), or is it a public suffix?"* — e.g. `example.jp`
//! is registrable, while `co.jp` is a public suffix.
//!
//! ```
//! # #[cfg(feature = "alloc")] {
//! // The public suffix ("effective TLD"):
//! assert_eq!(psl2::suffix("www.example.co.uk").as_deref(), Some("co.uk"));
//!
//! // The registrable domain (eTLD + 1) — the cookie domain:
//! assert_eq!(
//!     psl2::registrable_domain("www.example.co.uk").as_deref(),
//!     Some("example.co.uk")
//! );
//!
//! // A bare public suffix has no registrable domain:
//! assert_eq!(psl2::registrable_domain("co.uk"), None);
//! assert!(psl2::is_public_suffix("co.uk"));
//! # }
//! ```
//!
//! Unicode / internationalized domains are handled transparently (the `idna`
//! feature, enabled by default); inputs and outputs are normalized to
//! ASCII/punycode:
//!
//! ```
//! # #[cfg(feature = "idna")] {
//! assert_eq!(psl2::registrable_domain("食狮.公司.cn").as_deref(), Some("xn--85x722f.xn--55qx5d.cn"));
//! # }
//! ```
//!
//! # `no_std` and `no_alloc`
//!
//! The crate is `#![no_std]`. Its **core lookup is allocation-free**: pass an
//! already-lowercased ASCII/punycode hostname to [`lookup`] and read back
//! borrowed slices via [`Domain`]:
//!
//! ```
//! let d = psl2::lookup("www.example.co.uk").unwrap();
//! assert_eq!(d.suffix(), "co.uk");
//! assert_eq!(d.registrable_domain(), Some("example.co.uk"));
//! assert_eq!(d.subdomain(), Some("www"));
//! ```
//!
//! Cargo features:
//!
//! * **`std`** *(default)* — currently just implies `alloc`.
//! * **`alloc`** *(default, via `std`)* — the owned/normalizing convenience API
//!   ([`analyze`], [`suffix`], [`registrable_domain`], [`subdomain`],
//!   [`is_public_suffix`]).
//! * **`idna`** *(default)* — Unicode/IDN input (implies `alloc`).
//!
//! With *no* features the crate compiles on bare-metal targets with no
//! allocator, exposing only [`lookup`], [`Domain`], [`Type`], and
//! [`psl_version`].
//!
//! # How it differs from `psl`
//!
//! * **Fast builds.** The list is pre-normalized to ASCII at *publish* time and
//!   embedded as plain, sorted data ([`include_str!`]), queried with a no-alloc
//!   binary search. There is no `build.rs` and no procedural-macro codegen.
//! * **Built-in IDNA** and a clean `&str` API with ICANN / private / unknown
//!   classification.
//! * **`no_std` + `no_alloc`** core, and **always current** (CI republishes
//!   when the upstream list changes — see [`psl_version`]).
//!
//! # A note on "surprising" suffixes
//!
//! The list's PRIVATE section contains organizationally-delegated suffixes such
//! as `blogspot.com`, `github.io`, and `s3.amazonaws.com`. These *are* public
//! suffixes, so `registrable_domain("blogspot.com")` is `None` and
//! `registrable_domain("foo.blogspot.com")` is `Some("foo.blogspot.com")`.
//! This matches browser cookie behavior, but can be surprising. Use
//! [`Domain::is_private`] / [`Domain::is_icann`] if you need to tell the two
//! sections apart.
//!
//! [`psl`]: https://crates.io/crates/psl
//! [Public Suffix List]: https://publicsuffix.org/

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "alloc")]
use alloc::string::String;

use core::cmp::Ordering;

/// Ordinary rules (`com`, `co.uk`), one `rule\tS` per line, sorted by rule.
static RULES: &str = include_str!("rules.txt");
/// Wildcard rules `*.Y`, keyed by `Y` (e.g. `ck` for `*.ck`).
static WILDCARDS: &str = include_str!("wildcards.txt");
/// Exception rules `!X`, keyed by `X` (e.g. `www.ck`).
static EXCEPTIONS: &str = include_str!("exceptions.txt");

/// Upper bound on labels we analyze (a 253-char hostname has at most ~127).
const MAX_LABELS: usize = 128;

/// Which section of the Public Suffix List a rule comes from.
///
/// The ICANN section covers real registry TLDs; the PRIVATE section covers
/// suffixes delegated by private organizations (e.g. `github.io`,
/// `s3.amazonaws.com`). Browsers honor both for cookie scoping, but some
/// callers only care about the ICANN boundary — this lets you choose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    /// A rule from the ICANN section of the list.
    Icann,
    /// A rule from the PRIVATE section of the list.
    Private,
}

/// Internal, `Copy` result of the matching algorithm: byte offsets into the
/// analyzed hostname plus the matched section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Parts {
    suffix_off: usize,
    domain_off: Option<usize>,
    typ: Option<Type>,
}

/// Binary-search one sorted data block for `key`, returning its [`Type`] if a
/// line's rule equals `key`. Allocation-free.
fn lookup_block(block: &str, key: &str) -> Option<Type> {
    let b = block.as_bytes();
    let mut lo = 0usize;
    let mut hi = b.len();
    // Invariant: `lo` and `hi` are line starts (or the very end of the block).
    while lo < hi {
        // Midpoint, snapped back to the start of the line it falls in.
        let mut mid = lo + (hi - lo) / 2;
        while mid > lo && b[mid - 1] != b'\n' {
            mid -= 1;
        }
        // End of that line.
        let mut end = mid;
        while end < hi && b[end] != b'\n' {
            end += 1;
        }
        let (rule, sec) = split_line(&block[mid..end]);
        match key.as_bytes().cmp(rule.as_bytes()) {
            Ordering::Equal => return Some(sec),
            Ordering::Less => hi = mid,
            Ordering::Greater => lo = end + 1,
        }
    }
    None
}

/// Split a `rule\tS` line into its rule and section.
fn split_line(line: &str) -> (&str, Type) {
    let bytes = line.as_bytes();
    match bytes.iter().position(|&c| c == b'\t') {
        Some(t) => {
            let sec = if bytes.get(t + 1) == Some(&b'P') {
                Type::Private
            } else {
                Type::Icann
            };
            (&line[..t], sec)
        }
        None => (line, Type::Icann),
    }
}

/// Run the Public Suffix List algorithm over an already-normalized
/// (lowercase ASCII/punycode) hostname. Allocation-free.
fn compute(ascii: &str) -> Option<Parts> {
    // Byte offsets of each label start.
    let mut offs = [0usize; MAX_LABELS];
    let mut n = 1usize;
    for (i, &b) in ascii.as_bytes().iter().enumerate() {
        if b == b'.' {
            if n >= MAX_LABELS {
                return None;
            }
            offs[n] = i + 1;
            n += 1;
        }
    }

    // Prevailing rule: an exception always wins; else the most-labels match;
    // else the implicit `*` default (a single-label suffix).
    let mut best: Option<(usize, Type)> = None;
    let mut exception: Option<(usize, Type)> = None;

    for i in 0..n {
        let cand = &ascii[offs[i]..]; // labels i..n => (n - i) labels
        let rule_labels = n - i;

        if let Some(ty) = lookup_block(EXCEPTIONS, cand) {
            if exception.is_none_or(|(c, _)| rule_labels > c) {
                exception = Some((rule_labels, ty));
            }
        }
        if let Some(ty) = lookup_block(RULES, cand) {
            if best.is_none_or(|(c, _)| rule_labels > c) {
                best = Some((rule_labels, ty));
            }
        }
        // Wildcard `*.Y`: the `*` consumes label `i`, `Y` is labels i+1..n.
        if i + 1 < n {
            let y = &ascii[offs[i + 1]..];
            if let Some(ty) = lookup_block(WILDCARDS, y) {
                if best.is_none_or(|(c, _)| rule_labels > c) {
                    best = Some((rule_labels, ty));
                }
            }
        }
    }

    let (suffix_labels, typ) = if let Some((c, ty)) = exception {
        (c - 1, Some(ty)) // exception suffix = rule minus its leftmost label
    } else if let Some((c, ty)) = best {
        (c, Some(ty))
    } else {
        (1, None) // implicit `*`
    };

    let suffix_idx = n - suffix_labels;
    Some(Parts {
        suffix_off: offs[suffix_idx],
        domain_off: if suffix_idx >= 1 {
            Some(offs[suffix_idx - 1])
        } else {
            None
        },
        typ,
    })
}

/// `true` if `host` is a non-empty, lowercase, ASCII hostname with no empty
/// labels (the precondition for [`lookup`]).
fn is_normalized_ascii(host: &str) -> bool {
    let b = host.as_bytes();
    if b.is_empty() || b[0] == b'.' || b[b.len() - 1] == b'.' {
        return false;
    }
    let mut prev_dot = false;
    for &c in b {
        if !c.is_ascii() {
            return false;
        }
        if c == b'.' {
            if prev_dot {
                return false; // empty label
            }
            prev_dot = true;
        } else {
            if c.is_ascii_uppercase() {
                return false; // must be pre-lowercased
            }
            prev_dot = false;
        }
    }
    true
}

/// The result of an allocation-free [`lookup`], borrowing the input hostname.
///
/// All accessors return subslices of the hostname you passed to [`lookup`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Domain<'a> {
    input: &'a str,
    parts: Parts,
}

impl<'a> Domain<'a> {
    /// The hostname this was derived from (the input minus any trailing dot).
    #[inline]
    pub fn as_str(&self) -> &'a str {
        self.input
    }

    /// The public suffix ("effective TLD"), e.g. `co.uk` for
    /// `www.example.co.uk`.
    #[inline]
    pub fn suffix(&self) -> &'a str {
        &self.input[self.parts.suffix_off..]
    }

    /// The registrable domain (public suffix plus one more label), e.g.
    /// `example.co.uk`. This is the domain that can set cookies.
    ///
    /// `None` when the input is itself a public suffix.
    #[inline]
    pub fn registrable_domain(&self) -> Option<&'a str> {
        self.parts.domain_off.map(|o| &self.input[o..])
    }

    /// The subdomain: the labels left of the registrable domain, e.g. `www`
    /// for `www.example.co.uk`. `None` if there is none.
    #[inline]
    pub fn subdomain(&self) -> Option<&'a str> {
        match self.parts.domain_off {
            Some(d) if d > 0 => Some(&self.input[..d - 1]),
            _ => None,
        }
    }

    /// `true` if the input is itself a public suffix (no registrable domain).
    #[inline]
    pub fn is_public_suffix(&self) -> bool {
        self.parts.domain_off.is_none()
    }

    /// The section of the matched rule, or `None` for the implicit default
    /// rule (an unknown TLD).
    #[inline]
    pub fn typ(&self) -> Option<Type> {
        self.parts.typ
    }

    /// `true` if the suffix matched a rule in the ICANN section.
    #[inline]
    pub fn is_icann(&self) -> bool {
        self.parts.typ == Some(Type::Icann)
    }

    /// `true` if the suffix matched a rule in the PRIVATE section.
    #[inline]
    pub fn is_private(&self) -> bool {
        self.parts.typ == Some(Type::Private)
    }

    /// `true` if a real rule matched (not the implicit `*` default).
    #[inline]
    pub fn is_known(&self) -> bool {
        self.parts.typ.is_some()
    }
}

/// Look up an **already-normalized** hostname against the Public Suffix List,
/// without allocating.
///
/// `host` must be lowercase ASCII/punycode with no empty labels (a single
/// trailing dot is accepted and ignored). Returns `None` otherwise. For
/// arbitrary or Unicode input, use [`analyze`] (requires the `alloc` / `idna`
/// features), which normalizes for you.
///
/// ```
/// let d = psl2::lookup("a.b.example.co.uk").unwrap();
/// assert_eq!(d.suffix(), "co.uk");
/// assert_eq!(d.registrable_domain(), Some("example.co.uk"));
/// assert!(psl2::lookup("WWW.EXAMPLE.COM").is_none()); // not pre-lowercased
/// ```
#[inline]
pub fn lookup(host: &str) -> Option<Domain<'_>> {
    let host = host.strip_suffix('.').unwrap_or(host);
    if !is_normalized_ascii(host) {
        return None;
    }
    Some(Domain {
        input: host,
        parts: compute(host)?,
    })
}

/// The upstream Public Suffix List version (a UTC timestamp string) this build
/// of `psl2` was generated from.
#[inline]
pub fn psl_version() -> &'static str {
    include_str!("psl_version.txt")
}

// ---------------------------------------------------------------------------
// Allocating convenience API (feature = "alloc", on by default via "std").
// ---------------------------------------------------------------------------

/// The result of [`analyze`], owning the normalized hostname.
///
/// Accessors return slices of the normalized (lowercased, ASCII/punycode) form,
/// retrievable via [`Info::as_ascii`].
#[cfg(feature = "alloc")]
#[derive(Clone, Debug)]
pub struct Info {
    ascii: String,
    parts: Parts,
}

#[cfg(feature = "alloc")]
impl Info {
    #[inline]
    fn borrow(&self) -> Domain<'_> {
        Domain {
            input: &self.ascii,
            parts: self.parts,
        }
    }

    /// The fully normalized (lowercased, ASCII/punycode) form of the input.
    #[inline]
    pub fn as_ascii(&self) -> &str {
        &self.ascii
    }

    /// The public suffix ("effective TLD").
    #[inline]
    pub fn suffix(&self) -> &str {
        self.borrow().suffix()
    }

    /// The registrable domain (eTLD + 1), or `None` if the input is itself a
    /// public suffix.
    #[inline]
    pub fn registrable_domain(&self) -> Option<&str> {
        self.borrow().registrable_domain()
    }

    /// The subdomain, or `None` if there is none.
    #[inline]
    pub fn subdomain(&self) -> Option<&str> {
        self.borrow().subdomain()
    }

    /// `true` if the input is itself a public suffix.
    #[inline]
    pub fn is_public_suffix(&self) -> bool {
        self.borrow().is_public_suffix()
    }

    /// The section of the matched rule, or `None` for the default rule.
    #[inline]
    pub fn typ(&self) -> Option<Type> {
        self.parts.typ
    }

    /// `true` if the suffix matched a rule in the ICANN section.
    #[inline]
    pub fn is_icann(&self) -> bool {
        self.borrow().is_icann()
    }

    /// `true` if the suffix matched a rule in the PRIVATE section.
    #[inline]
    pub fn is_private(&self) -> bool {
        self.borrow().is_private()
    }

    /// `true` if a real rule matched (not the implicit `*` default).
    #[inline]
    pub fn is_known(&self) -> bool {
        self.borrow().is_known()
    }
}

/// Normalize an input hostname to lowercase ASCII/punycode, or `None` if it is
/// not a usable hostname.
#[cfg(feature = "alloc")]
fn normalize(domain: &str) -> Option<String> {
    let d = domain.trim();
    let d = d.strip_suffix('.').unwrap_or(d);
    if d.is_empty() {
        return None;
    }
    let ascii = to_ascii(d)?;
    if ascii.is_empty() || ascii.starts_with('.') || ascii.ends_with('.') || ascii.contains("..") {
        return None;
    }
    Some(ascii)
}

#[cfg(all(feature = "alloc", feature = "idna"))]
#[inline]
fn to_ascii(domain: &str) -> Option<String> {
    idna::domain_to_ascii(domain).ok()
}

#[cfg(all(feature = "alloc", not(feature = "idna")))]
#[inline]
fn to_ascii(domain: &str) -> Option<String> {
    // Without the `idna` feature we only accept already-ASCII hostnames; we
    // still lowercase them so matching is case-insensitive.
    if domain.is_ascii() {
        Some(domain.to_ascii_lowercase())
    } else {
        None
    }
}

/// Analyze any hostname against the Public Suffix List, normalizing it first.
///
/// Returns `None` only if the input cannot be normalized into a hostname
/// (empty, malformed, or — without the `idna` feature — non-ASCII).
///
/// ```
/// # #[cfg(feature = "alloc")] {
/// let info = psl2::analyze("WwW.Example.CO.UK").unwrap();
/// assert_eq!(info.as_ascii(), "www.example.co.uk");
/// assert_eq!(info.suffix(), "co.uk");
/// assert_eq!(info.registrable_domain(), Some("example.co.uk"));
/// # }
/// ```
#[cfg(feature = "alloc")]
pub fn analyze(domain: &str) -> Option<Info> {
    let ascii = normalize(domain)?;
    let parts = compute(&ascii)?;
    Some(Info { ascii, parts })
}

/// The public suffix of a hostname, e.g. `co.uk` for `www.example.co.uk`.
#[cfg(feature = "alloc")]
#[inline]
pub fn suffix(domain: &str) -> Option<String> {
    analyze(domain).map(|i| String::from(i.suffix()))
}

/// The registrable domain (eTLD + 1) of a hostname, e.g. `example.co.uk`.
///
/// `None` if the input is not a usable hostname, or is itself a public suffix.
#[cfg(feature = "alloc")]
#[inline]
pub fn registrable_domain(domain: &str) -> Option<String> {
    analyze(domain).and_then(|i| i.registrable_domain().map(String::from))
}

/// The subdomain (the labels left of the registrable domain) of a hostname,
/// e.g. `www` for `www.example.co.uk`.
#[cfg(feature = "alloc")]
#[inline]
pub fn subdomain(domain: &str) -> Option<String> {
    analyze(domain).and_then(|i| i.subdomain().map(String::from))
}

/// `true` if `domain` is itself a public suffix (and so cannot own cookies).
///
/// Returns `false` for inputs that are not usable hostnames.
#[cfg(feature = "alloc")]
#[inline]
pub fn is_public_suffix(domain: &str) -> bool {
    analyze(domain).is_some_and(|i| i.is_public_suffix())
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::string::String;

    #[test]
    fn core_lookup_basic() {
        let d = lookup("www.example.co.uk").unwrap();
        assert_eq!(d.suffix(), "co.uk");
        assert_eq!(d.registrable_domain(), Some("example.co.uk"));
        assert_eq!(d.subdomain(), Some("www"));
        assert!(d.is_icann());
    }

    #[test]
    fn core_lookup_requires_normalized_input() {
        assert!(lookup("WWW.EXAMPLE.COM").is_none());
        assert!(lookup("食狮.com.cn").is_none());
        assert!(lookup("").is_none());
        assert!(lookup(".com").is_none());
        assert!(lookup("a..b").is_none());
        // A single trailing dot is accepted.
        assert_eq!(lookup("example.com.").unwrap().suffix(), "com");
    }

    #[test]
    fn core_wildcard_and_exception() {
        assert!(lookup("foo.ck").unwrap().is_public_suffix());
        assert_eq!(
            lookup("a.b.test.ck").unwrap().registrable_domain(),
            Some("b.test.ck")
        );
        assert_eq!(
            lookup("www.ck").unwrap().registrable_domain(),
            Some("www.ck")
        );
        assert_eq!(lookup("www.ck").unwrap().suffix(), "ck");
    }

    #[test]
    fn core_unknown_tld_default_rule() {
        let d = lookup("foo.nonexistenttld").unwrap();
        assert_eq!(d.suffix(), "nonexistenttld");
        assert_eq!(d.registrable_domain(), Some("foo.nonexistenttld"));
        assert!(!d.is_known());
    }

    /// Cross-check the binary search against a brute-force scan for every rule.
    #[test]
    fn binary_search_matches_linear() {
        for block in [RULES, WILDCARDS, EXCEPTIONS] {
            for line in block.lines() {
                let (rule, sec) = split_line(line);
                assert_eq!(lookup_block(block, rule), Some(sec), "missing {rule:?}");
            }
            assert_eq!(lookup_block(block, "\u{0}definitely-not-present"), None);
            assert_eq!(lookup_block(block, "zzzzzzzz-not-present"), None);
        }
    }

    #[test]
    fn version_present() {
        assert!(!psl_version().is_empty());
        assert!(!psl_version().contains('\n'));
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn alloc_api() {
        assert_eq!(suffix("example.com").as_deref(), Some("com"));
        assert_eq!(
            registrable_domain("www.example.co.uk").as_deref(),
            Some("example.co.uk")
        );
        assert_eq!(subdomain("a.b.example.co.uk").as_deref(), Some("a.b"));
        assert_eq!(registrable_domain("co.uk"), None);
        assert!(is_public_suffix("co.uk"));
        assert_ne!(suffix("."), Some(String::new()));
        assert!(analyze("com..").is_none());
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn alloc_normalizes_case_and_trailing_dot() {
        assert_eq!(
            registrable_domain("WwW.Example.COM.").as_deref(),
            Some("example.com")
        );
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn alloc_private_suffix() {
        let info = analyze("foo.blogspot.com").unwrap();
        assert_eq!(info.suffix(), "blogspot.com");
        assert!(info.is_private());
        assert_eq!(registrable_domain("blogspot.com"), None);
    }
}
