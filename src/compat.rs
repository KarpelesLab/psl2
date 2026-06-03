//! A thin, [`psl`]-crate-compatible API to ease migration.
//!
//! The shipping [`crate`] API is `&str`-first and exposes a single [`Domain`]
//! that describes the whole host. The [`psl`] crate instead operates on
//! `&[u8]` and returns a `Domain` that *is* the registrable domain (with a
//! nested `Suffix`). This module mirrors that shape so migrating is closer to a
//! find-and-replace:
//!
//! ```text
//! psl::domain_str(name)  ->  psl2::compat::domain_str(name)
//! psl::suffix_str(name)  ->  psl2::compat::suffix_str(name)
//! psl::domain(bytes)     ->  psl2::compat::domain(bytes)
//! psl::suffix(bytes)     ->  psl2::compat::suffix(bytes)
//! ```
//!
//! ```
//! let d = psl2::compat::domain_str("www.example.co.uk").unwrap();
//! assert_eq!(d.as_bytes(), b"example.co.uk");
//! assert_eq!(d.suffix().as_bytes(), b"co.uk");
//! assert!(d.suffix().is_known());
//! ```
//!
//! Like `psl`, this API is allocation-free, borrows from its input, and is
//! **case-sensitive** — it expects already-lowercased ASCII/punycode and falls
//! back to the implicit `*` rule for anything it does not match (rather than
//! normalizing). For Unicode input or automatic normalization, use the main
//! [`crate::analyze`] API instead.
//!
//! [`psl`]: https://crates.io/crates/psl

use super::{compute, Parts};

pub use super::Type;

/// The public suffix of a domain name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Suffix<'a> {
    bytes: &'a [u8],
    typ: Option<Type>,
    fqdn: bool,
}

impl<'a> Suffix<'a> {
    /// The public suffix as bytes (without any fully-qualifying trailing dot).
    #[inline]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// The section the suffix matched, or `None` for an unknown TLD (the
    /// implicit `*` rule).
    #[inline]
    pub fn typ(&self) -> Option<Type> {
        self.typ
    }

    /// `true` if the suffix matched a real rule rather than the implicit `*`.
    #[inline]
    pub fn is_known(&self) -> bool {
        self.typ.is_some()
    }

    /// `true` if the original input ended with a fully-qualifying dot.
    #[inline]
    pub fn is_fqdn(&self) -> bool {
        self.fqdn
    }
}

/// A registrable domain name (its bytes are the eTLD + 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Domain<'a> {
    bytes: &'a [u8],
    suffix: Suffix<'a>,
}

impl<'a> Domain<'a> {
    /// The registrable domain as bytes.
    #[inline]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// The public suffix of this registrable domain.
    #[inline]
    pub fn suffix(&self) -> Suffix<'a> {
        self.suffix
    }
}

/// Parse `name` into the matching host string, its [`Parts`], and whether it
/// was fully qualified.
fn parse(name: &[u8]) -> Option<(&str, Parts, bool)> {
    let s = core::str::from_utf8(name).ok()?;
    if s.is_empty() {
        return None;
    }
    let fqdn = s.as_bytes().last() == Some(&b'.');
    let host = if fqdn { &s[..s.len() - 1] } else { s };
    if host.is_empty() {
        return None;
    }
    let parts = compute(host)?;
    Some((host, parts, fqdn))
}

/// Get the public suffix of a domain name (`psl`-compatible).
#[inline]
pub fn suffix(name: &[u8]) -> Option<Suffix<'_>> {
    let (host, parts, fqdn) = parse(name)?;
    Some(Suffix {
        bytes: &host.as_bytes()[parts.suffix_off..],
        typ: parts.typ,
        fqdn,
    })
}

/// Get the public suffix of a domain name from a `&str` (`psl`-compatible).
#[inline]
pub fn suffix_str(name: &str) -> Option<Suffix<'_>> {
    suffix(name.as_bytes())
}

/// Get the registrable domain of a domain name (`psl`-compatible).
///
/// Returns `None` if `name` is invalid or is itself a public suffix (no
/// registrable domain).
#[inline]
pub fn domain(name: &[u8]) -> Option<Domain<'_>> {
    let (host, parts, fqdn) = parse(name)?;
    let domain_off = parts.domain_off?;
    Some(Domain {
        bytes: &host.as_bytes()[domain_off..],
        suffix: Suffix {
            bytes: &host.as_bytes()[parts.suffix_off..],
            typ: parts.typ,
            fqdn,
        },
    })
}

/// Get the registrable domain of a domain name from a `&str`
/// (`psl`-compatible).
#[inline]
pub fn domain_str(name: &str) -> Option<Domain<'_>> {
    domain(name.as_bytes())
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    #[test]
    fn domain_and_suffix() {
        let d = domain_str("www.example.co.uk").unwrap();
        assert_eq!(d.as_bytes(), b"example.co.uk");
        assert_eq!(d.suffix().as_bytes(), b"co.uk");
        assert!(d.suffix().is_known());
        assert_eq!(d.suffix().typ(), Some(Type::Icann));

        assert_eq!(suffix_str("com").unwrap().as_bytes(), b"com");
        assert_eq!(domain_str("com"), None); // a bare suffix has no domain
    }

    #[test]
    fn unknown_tld_uses_default_rule() {
        let s = suffix_str("foo.example").unwrap();
        assert_eq!(s.as_bytes(), b"example");
        assert!(!s.is_known());
        assert_eq!(
            domain_str("foo.example").unwrap().as_bytes(),
            b"foo.example"
        );
    }

    #[test]
    fn fully_qualified() {
        let s = suffix_str("example.com.").unwrap();
        assert_eq!(s.as_bytes(), b"com");
        assert!(s.is_fqdn());
        assert!(!suffix_str("example.com").unwrap().is_fqdn());
    }

    #[test]
    fn byte_and_str_agree() {
        assert_eq!(domain(b"a.b.example.com"), domain_str("a.b.example.com"));
        assert_eq!(
            domain_str("a.b.example.com").unwrap().as_bytes(),
            b"example.com"
        );
    }

    #[test]
    fn invalid() {
        assert_eq!(suffix(b""), None);
        assert_eq!(domain(b""), None);
        assert_eq!(suffix(&[0xff, 0xfe]), None); // not UTF-8
    }
}
