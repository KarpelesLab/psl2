//! Runs the canonical `checkPublicSuffix` test vectors published alongside the
//! Public Suffix List (vendored in `tests/tests.txt`).
//!
//! The upstream file expresses expectations in Unicode; `psl2` returns
//! ASCII/punycode, so we compare both sides after normalizing the expected
//! value through the same `idna` path.

#![cfg(feature = "idna")]

const VECTORS: &str = include_str!("tests.txt");

/// Normalize an expected registrable-domain string the same way `psl2`
/// normalizes its output, so Unicode and punycode expectations both compare
/// equal to the crate's ASCII result.
fn expected_ascii(s: &str) -> String {
    idna::domain_to_ascii(s).expect("expected value should be a valid domain")
}

#[test]
fn official_vectors() {
    let mut checked = 0usize;
    let mut failures = Vec::new();

    for (lineno, line) in VECTORS.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }

        // Each case is `input expected`, where either token may be `null`.
        let mut parts = line.splitn(2, char::is_whitespace);
        let input = parts.next().unwrap_or("");
        let expected = parts.next().unwrap_or("").trim();

        let got = psl2::registrable_domain(if input == "null" { "" } else { input });

        let ok = if expected == "null" {
            got.is_none()
        } else {
            got.as_deref() == Some(expected_ascii(expected).as_str())
        };

        if !ok {
            failures.push(format!(
                "  line {}: input={:?} expected={:?} got={:?}",
                lineno + 1,
                input,
                expected,
                got,
            ));
        }
        checked += 1;
    }

    assert!(
        failures.is_empty(),
        "{} of {} official vectors failed:\n{}",
        failures.len(),
        checked,
        failures.join("\n"),
    );

    // Sanity: make sure we actually exercised the file.
    assert!(checked > 50, "only {checked} vectors parsed");
}
