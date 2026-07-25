// Decodes a compiled .mobi's INDX/TAGX/IDXT records via kindling's own
// `mobi_dump` and checks that a target set of labels (1) are present as
// decoded entry labels and (2) sit in binary-search-consistent (monotonic
// raw UTF-16BE) order within their containing leaf record. kindling sorts
// Greek/Latin INDX labels by raw UTF-16BE byte value rather than proper
// collation, so this is the structural precondition for on-device lookup to
// find a given label at all - independent of whether the device's own
// folding/search behavior actually reaches it.
//
// Usage: cargo run --bin verify_indx_order -- <path.mobi> [target words...]

use std::env;
use std::path::PathBuf;

fn utf16be_bytes(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn cmp_utf16be(a: &str, b: &str) -> std::cmp::Ordering {
    utf16be_bytes(a).cmp(&utf16be_bytes(b))
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: verify_indx_order <path.mobi> [target words...]");
        std::process::exit(2);
    }
    let path = PathBuf::from(&args[1]);
    let targets: Vec<String> = if args.len() > 2 {
        args[2..].to_vec()
    } else {
        vec![]
    };

    let dump = match kindling::mobi_dump::dump_mobi(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("dump_mobi failed: {}", e);
            std::process::exit(1);
        }
    };

    // Parse lines like: indx[3].entries[42] = { label = "Έδειχναν", ... }
    // Group decoded labels by record index, preserving on-disk entry order.
    let mut records: std::collections::BTreeMap<usize, Vec<(usize, String)>> =
        std::collections::BTreeMap::new();

    for line in dump.lines() {
        let Some(rest) = line.strip_prefix("indx[") else { continue };
        let Some(close) = rest.find(']') else { continue };
        let rec_idx: usize = match rest[..close].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let rest2 = &rest[close + 1..];
        let Some(rest2) = rest2.strip_prefix(".entries[") else { continue };
        let Some(close2) = rest2.find(']') else { continue };
        let entry_idx: usize = match rest2[..close2].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        // Extract label = "..." (first occurrence; routing_label for gen=0,
        // label for gen=1 data entries).
        let label = extract_quoted_after(line, "label = ")
            .or_else(|| extract_quoted_after(line, "routing_label = "));
        if let Some(label) = label {
            records.entry(rec_idx).or_default().push((entry_idx, label));
        }
    }

    let mut total_entries = 0usize;
    let mut monotonic_violations = 0usize;
    let mut found: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut found_positions: Vec<(String, usize, usize)> = Vec::new(); // (label, record, entry)

    for (rec_idx, entries) in &records {
        total_entries += entries.len();
        let mut prev: Option<&str> = None;
        for (entry_idx, label) in entries {
            if targets.iter().any(|t| t == label) {
                found.insert(label.clone());
                found_positions.push((label.clone(), *rec_idx, *entry_idx));
            }
            if let Some(p) = prev
                && cmp_utf16be(p, label) == std::cmp::Ordering::Greater
            {
                monotonic_violations += 1;
                println!(
                    "NON-MONOTONIC in record {}: {:?} (entry {}) > {:?} (entry {})",
                    rec_idx, p, entry_idx.saturating_sub(1), label, entry_idx
                );
            }
            prev = Some(label.as_str());
        }
    }

    println!("Parsed {} INDX records, {} total entries", records.len(), total_entries);
    println!("Monotonic (raw UTF-16BE) violations: {}", monotonic_violations);

    if !targets.is_empty() {
        println!("\nTarget word lookup:");
        for t in &targets {
            if found.contains(t) {
                let positions: Vec<String> = found_positions
                    .iter()
                    .filter(|(l, _, _)| l == t)
                    .map(|(_, r, e)| format!("record {} entry {}", r, e))
                    .collect();
                println!("  FOUND {:?} at [{}]", t, positions.join(", "));
            } else {
                println!("  MISSING {:?}", t);
            }
        }
    }

    if monotonic_violations > 0 {
        std::process::exit(1);
    }
}

fn extract_quoted_after(line: &str, marker: &str) -> Option<String> {
    let idx = line.find(marker)?;
    let rest = &line[idx + marker.len()..];
    if !rest.starts_with('"') {
        return None;
    }
    let mut out = String::new();
    let mut chars = rest[1..].chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else if c == '"' {
            return Some(out);
        } else {
            out.push(c);
        }
    }
    None
}
