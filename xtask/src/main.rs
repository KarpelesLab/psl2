//! Build-time code generator for the `psl2` crate.
//!
//! Reads the vendored `list/public_suffix_list.dat`, normalizes every rule to
//! ASCII/punycode, and emits a flattened **reversed-label trie** that the
//! library walks from the TLD inward with no allocation:
//!
//! * `src/trie_nodes.bin` — 8 bytes/node: `edge_start: u32 LE`,
//!   `edge_count: u16 LE`, `flags: u8`, pad. Node 0 is the root.
//! * `src/trie_edges.bin` — 9 bytes/edge: `label_off: u32 LE`, `label_len: u8`,
//!   `child: u32 LE`. A node's edges are contiguous and sorted by label.
//! * `src/trie_labels.txt` — every edge label concatenated; an edge's label is
//!   `labels[off .. off + len]`.
//!
//! `flags` bits: `RULE=1, RULE_PRIV=2, WILD=4, WILD_PRIV=8, EXC=16, EXC_PRIV=32`
//! (a `_PRIV` bit means the rule came from the PRIVATE section).
//!
//! For human inspection and the benchmarks it also writes the sorted
//! `src/{rules,wildcards,exceptions}.txt` (`rule\tS` lines; `S` = `I`/`P`) and
//! `src/psl_version.txt`. Wildcard rules `*.Y` are stored as `Y`; exception
//! rules `!X` as `X`.
//!
//! Runs at *publish* time (in CI), not on every consumer build:
//!
//! ```text
//! cargo run -p xtask
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::process::ExitCode;

const SRC: &str = "list/public_suffix_list.dat";

// Node flag bits (kept in sync with `src/lib.rs`).
const F_RULE: u8 = 1;
const F_RULE_PRIV: u8 = 2;
const F_WILD: u8 = 4;
const F_WILD_PRIV: u8 = 8;
const F_EXC: u8 = 16;
const F_EXC_PRIV: u8 = 32;

#[derive(Default)]
struct Node {
    children: BTreeMap<String, usize>, // label -> node index (sorted by label)
    flags: u8,
}

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

    // Build the reversed-label trie in an arena (node 0 = root).
    let mut nodes: Vec<Node> = vec![Node::default()];
    let priv_bit = |sec: char, bit: u8| if sec == 'P' { bit } else { 0 };
    for (rule, &sec) in &rules {
        let n = insert(&mut nodes, rule);
        nodes[n].flags |= F_RULE | priv_bit(sec, F_RULE_PRIV);
    }
    for (y, &sec) in &wildcards {
        let n = insert(&mut nodes, y);
        nodes[n].flags |= F_WILD | priv_bit(sec, F_WILD_PRIV);
    }
    for (x, &sec) in &exceptions {
        let n = insert(&mut nodes, x);
        nodes[n].flags |= F_EXC | priv_bit(sec, F_EXC_PRIV);
    }

    // Flatten: node records, edge records, and a label blob.
    let mut node_blob = Vec::with_capacity(nodes.len() * 8);
    let mut edge_blob = Vec::new();
    let mut labels = String::new();
    let mut edge_index = 0u32;
    for node in &nodes {
        let edge_start = edge_index;
        let edge_count = u16::try_from(node.children.len()).expect("too many edges");
        node_blob.extend_from_slice(&edge_start.to_le_bytes());
        node_blob.extend_from_slice(&edge_count.to_le_bytes());
        node_blob.push(node.flags);
        node_blob.push(0); // pad to 8 bytes
        edge_index += u32::from(edge_count);
    }
    for node in &nodes {
        for (label, &child) in &node.children {
            let label_off = u32::try_from(labels.len()).expect("labels exceed 4 GiB");
            let label_len = u8::try_from(label.len()).expect("label longer than 255 bytes");
            labels.push_str(label);
            edge_blob.extend_from_slice(&label_off.to_le_bytes());
            edge_blob.push(label_len);
            edge_blob.extend_from_slice(&(child as u32).to_le_bytes());
        }
    }

    let result = write_block("src/rules", &rules)
        .and_then(|_| write_block("src/wildcards", &wildcards))
        .and_then(|_| write_block("src/exceptions", &exceptions))
        .and_then(|_| write_file("src/psl_version.txt", version.as_bytes()))
        .and_then(|_| write_file("src/trie_nodes.bin", &node_blob))
        .and_then(|_| write_file("src/trie_edges.bin", &edge_blob))
        .and_then(|_| write_file("src/trie_labels.txt", labels.as_bytes()));
    if let Err(code) = result {
        return code;
    }

    eprintln!(
        "wrote {} rules, {} wildcards, {} exceptions (PSL {version}); \
         trie: {} nodes, {} edges, {} label bytes; {skipped} skipped",
        rules.len(),
        wildcards.len(),
        exceptions.len(),
        nodes.len(),
        edge_index,
        labels.len(),
    );
    ExitCode::SUCCESS
}

/// Insert a rule's labels into the trie in reversed order (so `co.uk` becomes
/// root → `uk` → `co`), returning the index of the terminal node.
fn insert(nodes: &mut Vec<Node>, rule: &str) -> usize {
    let mut node = 0usize;
    for label in rule.split('.').rev() {
        node = match nodes[node].children.get(label) {
            Some(&c) => c,
            None => {
                let c = nodes.len();
                nodes.push(Node::default());
                nodes[node].children.insert(label.to_string(), c);
                c
            }
        };
    }
    node
}

/// Write one sorted block as `<stem>.txt` (`rule\tS` lines) for human
/// inspection and the benchmarks. Not shipped in the published crate.
fn write_block(stem: &str, entries: &BTreeMap<String, char>) -> Result<(), ExitCode> {
    let mut text = String::new();
    for (rule, section) in entries {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(rule);
        text.push('\t');
        text.push(*section);
    }
    write_file(&format!("{stem}.txt"), text.as_bytes())
}

fn write_file(path: &str, bytes: &[u8]) -> Result<(), ExitCode> {
    fs::write(path, bytes).map_err(|e| {
        eprintln!("error: cannot write {path}: {e}");
        ExitCode::FAILURE
    })
}
