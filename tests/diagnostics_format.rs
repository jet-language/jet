//! Diagnostics registry per-entry format validator (card #447 / durability W2).
//!
//! D-REPORT-HOME1=A requires every typed diagnostic row to carry What/Why/Fix
//! prose, not just a code and severity. The coverage test
//! (tests/diagnostics_coverage.rs) checks reachability; this test checks the
//! row payload itself.
//!
//! Run: `cargo test --test diagnostics_format`

mod common;

use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn diagnostic_row_reference_is_generated() {
    let expected = jet::Explain::diagnostics_reference_markdown();
    let path = root().join("docs/spec/diagnostic-rows.md");
    if std::env::var_os("UPDATE_DIAGNOSTICS").is_some() {
        fs::write(&path, &expected).expect("write generated diagnostic-row reference");
    }
    let actual = fs::read_to_string(&path)
        .expect("docs/spec/diagnostic-rows.md missing; run with UPDATE_DIAGNOSTICS=1");
    assert_eq!(actual, expected, "typed diagnostic reference is stale");
}

#[test]
fn every_typed_diagnostic_row_is_complete() {
    let rows = jet_foundation::Registry::diagnostic_rows();
    let violations: Vec<String> = rows
        .iter()
        .flat_map(|row| {
            [
                ("What", row.what),
                ("Why", row.why),
                ("Fix", row.fix),
            ]
            .into_iter()
            .filter_map(move |(part, value)| {
                value
                    .trim()
                    .is_empty()
                    .then(|| format!("{}: {} is empty", row.code, part))
            })
        })
        .collect();
    assert!(
        violations.is_empty(),
        "typed diagnostic rows have malformed or incomplete What/Why/Fix templates \
         (I4 — every diagnostic needs what/why/fix, not just a code):\n{}",
        violations.join("\n")
    );
}

#[test]
fn typed_row_holes_and_structured_fixes_have_one_projection() {
    let source_markers = jet_foundation::Registry::DIAGNOSTIC_SOURCE
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
        .filter(|line| line.split('\t').nth(11).is_some_and(|value| value != "-"))
        .count();
    let typed_markers = jet_foundation::Registry::diagnostic_rows()
        .iter()
        .filter(|row| row.structured_fix.is_some())
        .count();
    assert_eq!(source_markers, typed_markers, "every source fix marker needs a typed row projection");

    for row in jet_foundation::Registry::diagnostic_rows() {
        let values: Vec<(&str, String)> = row
            .template_holes
            .iter()
            .map(|hole| (*hole, format!("<{hole}>")))
            .collect();
        let holes: Vec<(&str, &str)> = values
            .iter()
            .map(|(hole, value)| (*hole, value.as_str()))
            .collect();
        let rendered = row.render(&holes);
        assert!(!rendered.what.trim().is_empty(), "{} rendered empty What", row.code);
        assert!(!rendered.why.trim().is_empty(), "{} rendered empty Why", row.code);
        assert!(!rendered.fix.trim().is_empty(), "{} rendered empty Fix", row.code);
        if row.detail {
            assert!(!row.what.trim().is_empty(), "{} detailed row has empty What", row.code);
            assert!(!row.why.trim().is_empty(), "{} detailed row has empty Why", row.code);
            assert!(!row.fix.trim().is_empty(), "{} detailed row has empty Fix", row.code);
        }
        if let Some(fix) = row.structured_fix {
            assert!(!fix.source_marker().is_empty(), "{} has an empty typed structured fix", row.code);
        }
    }
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
