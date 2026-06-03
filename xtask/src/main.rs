//! Build-time code generator for the `psl2` crate.
//!
//! Reads the vendored `list/public_suffix_list.dat`, normalizes every rule to
//! ASCII/punycode (so the runtime never has to), and writes the compact,
//! **sorted** data files that the library embeds via `include_str!` and queries
//! with a no-alloc binary search:
//!
//! * `src/rules.txt`      — ordinary rules (`com`, `co.uk`)
//! * `src/wildcards.txt`  — the `Y` of each `*.Y` wildcard rule (`ck` for `*.ck`)
//! * `src/exceptions.txt` — the `X` of each `!X` exception rule (`www.ck`)
//! * `src/psl_version.txt` — the upstream list version string
//!
//! Each rule line is `rule\tS` where `S` is the section (`I` = ICANN,
//! `P` = PRIVATE). Lines are sorted by `rule` so the runtime can binary-search.
//!
//! This runs at *publish* time (in CI), not on every consumer build. Run it
//! locally with:
//!
//! ```text
//! cargo run -p xtask
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::process::ExitCode;

const SRC: &str = "list/public_suffix_list.dat";

fn main() -> ExitCode {
    let raw = match fs::read_to_string(SRC) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read {SRC}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let version = raw
        .lines()
        .find_map(|l| l.trim().strip_prefix("// VERSION:"))
        .map(str::trim)
        .unwrap_or("unknown")
        .to_string();

    // BTreeMap keeps each section sorted by rule and de-duplicates.
    let mut rules: BTreeMap<String, char> = BTreeMap::new();
    let mut wildcards: BTreeMap<String, char> = BTreeMap::new();
    let mut exceptions: BTreeMap<String, char> = BTreeMap::new();

    let mut section = 'I';
    let mut skipped = 0usize;

    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(comment) = t.strip_prefix("//") {
            if comment.contains("BEGIN PRIVATE DOMAINS") {
                section = 'P';
            } else if comment.contains("BEGIN ICANN DOMAINS") {
                section = 'I';
            }
            continue;
        }

        let rule = t.split_whitespace().next().unwrap_or("");
        if rule.is_empty() {
            continue;
        }

        let (table, domain_part): (&mut BTreeMap<String, char>, &str) =
            if let Some(r) = rule.strip_prefix('!') {
                (&mut exceptions, r)
            } else if let Some(r) = rule.strip_prefix("*.") {
                (&mut wildcards, r)
            } else if rule == "*" {
                (&mut wildcards, "")
            } else {
                (&mut rules, rule)
            };

        let ascii = match idna::domain_to_ascii(domain_part) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("warning: skipping rule {rule:?}: idna error: {e:?}");
                skipped += 1;
                continue;
            }
        };
        if ascii.contains('\t') || ascii.contains('\n') {
            eprintln!("warning: skipping rule {rule:?}: illegal character");
            skipped += 1;
            continue;
        }

        table.insert(ascii, section);
    }

    if let Err(code) = write_block("src/rules.txt", &rules)
        .and_then(|_| write_block("src/wildcards.txt", &wildcards))
        .and_then(|_| write_block("src/exceptions.txt", &exceptions))
        .and_then(|_| {
            // No trailing newline: the runtime returns this verbatim.
            fs::write("src/psl_version.txt", version.as_bytes()).map_err(|e| {
                eprintln!("error: cannot write src/psl_version.txt: {e}");
                ExitCode::FAILURE
            })
        })
    {
        return code;
    }

    eprintln!(
        "wrote {} rules, {} wildcards, {} exceptions (PSL {version}); {skipped} skipped",
        rules.len(),
        wildcards.len(),
        exceptions.len(),
    );
    ExitCode::SUCCESS
}

/// Write one sorted block as `rule\tS` lines joined by `\n` (no trailing
/// newline, so the runtime's binary search never sees an empty final line).
fn write_block(path: &str, entries: &BTreeMap<String, char>) -> Result<(), ExitCode> {
    let mut out = String::new();
    for (rule, section) in entries {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(rule);
        out.push('\t');
        out.push(*section);
    }
    fs::write(path, out.as_bytes()).map_err(|e| {
        eprintln!("error: cannot write {path}: {e}");
        ExitCode::FAILURE
    })
}
