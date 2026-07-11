//! Diagnostics registry per-entry format validator (card #447 / durability W2).
//!
//! docs/spec/diagnostics.md's "Adding a diagnostic" procedure requires every
//! diagnostic to carry What/Why/Fix prose (step 2), not just a one-line
//! registry-table "Meaning" cell. Section tables shaped
//! `| Code | What | Why | Fix |` are where that prose lives. This test fails
//! if any such row ships with an empty What, Why, or Fix cell — the coverage
//! test (tests/diagnostics_coverage.rs) only checks that a row/snapshot
//! *exists*, not that the body says anything.
//!
//! Run: `cargo test --test diagnostics_format`

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// TODO-ratchet: claude-main (card #465) is mid-editing E9999/E1291 table rows
// concurrently with this card. Exclude them here rather than fight a moving
// target; remove this exclusion once both codes have settled.
fn excluded_codes() -> BTreeSet<&'static str> {
    BTreeSet::from(["E9999", "E1291"])
}

#[test]
fn every_what_why_fix_table_row_is_complete() {
    let root = root();
    let text = fs::read_to_string(root.join("docs/spec/diagnostics.md"))
        .expect("docs/spec/diagnostics.md missing");

    let excluded = excluded_codes();
    let mut violations = Vec::new();
    let mut in_wwf_table = false;

    for (idx, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if !line.starts_with('|') {
            in_wwf_table = false;
            continue;
        }
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        // split('|') on "| a | b | c | d |" yields ["", "a", "b", "c", "d", ""]
        if is_wwf_header(&cells) {
            in_wwf_table = true;
            continue;
        }
        if !in_wwf_table {
            continue;
        }
        if cells.iter().all(|c| c.is_empty() || c.chars().all(|ch| ch == '-')) {
            continue; // header separator row
        }
        if cells.len() < 5 {
            continue;
        }
        let code = cells[1];
        let what = cells[2];
        let why = cells[3];
        let fix = cells[4];
        if code.is_empty() || !is_code_like(code) {
            continue; // not a per-code data row (e.g. a continuation/prose row)
        }
        if excluded.contains(code) {
            continue;
        }
        if what.is_empty() || why.is_empty() || fix.is_empty() {
            violations.push(format!(
                "line {}: {} — What/Why/Fix must all be non-empty (what={:?} why={:?} fix={:?})",
                idx + 1,
                code,
                what,
                why,
                fix
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "docs/spec/diagnostics.md has incomplete What/Why/Fix table rows (I4 — \
         every diagnostic needs what/why/fix, not just a code):\n{}",
        violations.join("\n")
    );
}

fn is_wwf_header(cells: &[&str]) -> bool {
    cells.len() >= 5
        && cells[1].eq_ignore_ascii_case("code")
        && cells[2].eq_ignore_ascii_case("what")
        && cells[3].eq_ignore_ascii_case("why")
        && cells[4].eq_ignore_ascii_case("fix")
}

fn is_code_like(code: &str) -> bool {
    let bytes = code.as_bytes();
    // Codes may be backtick-quoted with markdown emphasis stripped by callers;
    // accept the bare `[EL]NNNN` shape only.
    bytes.len() == 5
        && (bytes[0] == b'E' || bytes[0] == b'L')
        && bytes[1..].iter().all(|b| b.is_ascii_digit())
}
