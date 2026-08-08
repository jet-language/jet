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

use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn every_what_why_fix_table_row_is_complete() {
    let root = root();
    let text = fs::read_to_string(root.join("docs/spec/diagnostics.md"))
        .expect("docs/spec/diagnostics.md missing");
    let violations = diagnostic_body_violations(&text);
    assert!(
        violations.is_empty(),
        "docs/spec/diagnostics.md has malformed or incomplete What/Why/Fix table rows \
         (I4 — every diagnostic needs what/why/fix, not just a code):\n{}",
        violations.join("\n")
    );
}

#[test]
fn diagnostic_body_validator_rejects_malformed_rows() {
    let malformed = "| Code | What | Why | Fix |\n\
                     | --- | --- | --- | --- |\n\
                     | E0001 | what | why | |\n\
                     | E0002 | what | why |\n\
                     | `E0003` | what | why | |\n";
    let violations = diagnostic_body_violations(malformed);
    assert_eq!(violations.len(), 3, "{violations:#?}");
    assert!(violations[0].contains("E0001") && violations[0].contains("non-empty"));
    assert!(violations[1].contains("E0002") && violations[1].contains("malformed"));
    assert!(violations[2].contains("E0003") && violations[2].contains("non-empty"));
}

fn diagnostic_body_violations(text: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let mut in_wwf_table = false;

    for (idx, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if !line.starts_with('|') {
            in_wwf_table = false;
            continue;
        }
        let cells = markdown_cells(line);
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
        let Some(code) = cells.get(1).and_then(|code| diagnostic_code(code)) else {
            continue;
        };
        if cells.len() != 6 {
            violations.push(format!(
                "line {}: {} — malformed What/Why/Fix row has {} pipe-delimited cells, expected 6",
                idx + 1,
                code,
                cells.len()
            ));
            continue;
        }
        let what = &cells[2];
        let why = &cells[3];
        let fix = &cells[4];
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
    violations
}

fn markdown_cells(line: &str) -> Vec<String> {
    let mut cells = vec![String::new()];
    let mut escaped = false;
    for ch in line.chars() {
        if ch == '|' && !escaped {
            cells.push(String::new());
        } else {
            cells.last_mut().unwrap().push(ch);
        }
        escaped = ch == '\\' && !escaped;
    }
    cells
        .into_iter()
        .map(|cell| cell.trim().to_string())
        .collect()
}

fn is_wwf_header(cells: &[String]) -> bool {
    cells.len() >= 5
        && cells[1].eq_ignore_ascii_case("code")
        && cells[2].eq_ignore_ascii_case("what")
        && cells[3].eq_ignore_ascii_case("why")
        && cells[4].eq_ignore_ascii_case("fix")
}

fn is_code_like(code: &str) -> bool {
    jet::Explain::is_code(code)
}

fn diagnostic_code(cell: &str) -> Option<&str> {
    let code = cell
        .strip_prefix('`')
        .and_then(|cell| cell.strip_suffix('`'))
        .unwrap_or(cell);
    is_code_like(code).then_some(code)
}
