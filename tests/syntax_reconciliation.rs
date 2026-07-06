//! D-CANON-SOURCE1 / D-RECONCILE-SCOPE1: live examples, reference surface,
//! and agent memory must not reintroduce retired syntax spellings.

use std::fs;
use std::path::{Path, PathBuf};

const ROOTS: &[&str] = &[
    "AGENTS.md",
    "CLAUDE.md",
    "docs/spec",
    "docs/reference/syntax-surface.jet",
    "Source",
    "crates",
    "examples",
    "tests/ui",
];

const FORBIDDEN: &[&str] = &[
    "@unsafe",
    "@audit",
    "@extern",
    "@bindgen",
    "#extern",
    "#bindgen",
    "#layout",
    "#grant",
    "#context",
    "#test",
    "#pure",
    "#todo",
    // D-CAP9 retired bare `Ptr<T>` as a standalone TYPE ANNOTATION (`x: Ptr<Int>`,
    // teaches E0210). It did NOT retire `mem.Ptr<T>.from_addr(addr)` — the
    // module-qualified generic static-call form for building a typed pointer from
    // a raw address, which is the current E2-M13 low-level-tier spelling (the
    // sema's own diagnostic text recommends writing it, `CheckerCoreLib.rs`
    // `infer_ptr_from_addr`). A blunt substring check can't tell "type position"
    // from "call position," so this list intentionally omits both `mem.Ptr<` and
    // bare `Ptr<` rather than false-flag the still-shipped call form.
    "List<",
    "List[",
    "Map<",
    "#Bench \"",
    "#[Serialize",
    "Serialize]",
    "#[Deserialize",
    "Deserialize]",
    "core.json",
    "use jet.",
    "use std.",
    "?continue",
    "?break",
    "?return",
    "comptime val ",
];

#[test]
fn live_surface_has_no_retired_spellings() {
    let mut failures = Vec::new();
    for root in ROOTS {
        for path in files(Path::new(root)) {
            if should_skip(&path) {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            for (line_no, line) in scan_lines(&path, &text) {
                for needle in forbidden_for_path(&path) {
                    if line.contains(needle) && !allowed_retired_reference(&path, line) {
                        failures.push(format!(
                            "{}:{} contains `{}`",
                            path.display(),
                            line_no,
                            needle
                        ));
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "retired syntax found:\n{}",
        failures.join("\n")
    );
}

fn files(path: &Path) -> Vec<PathBuf> {
    if path.is_file() {
        return vec![path.to_path_buf()];
    }
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(path) else {
        return out;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            out.extend(files(&p));
        } else {
            out.push(p);
        }
    }
    out
}

fn should_skip(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.contains("/target/")
        || s.contains("_retired_")
        || s.ends_with(".published.snapshot")
        || s.ends_with("tests/syntax_reconciliation.rs")
        || s.ends_with("docs/spec/syntax-decisions.md")
}

fn scan_lines<'a>(path: &Path, text: &'a str) -> Vec<(usize, &'a str)> {
    if path.extension().and_then(|x| x.to_str()) != Some("rs") {
        return text.lines().enumerate().map(|(i, l)| (i + 1, l)).collect();
    }
    text.lines()
        .enumerate()
        .filter_map(|(i, line)| {
            if path.ends_with("crates/jet-parser/src/Parser/Items.rs")
                && line.contains("retired_c_module_marker_diag")
            {
                return None;
            }
            let trimmed = line.trim_start();
            if trimmed.starts_with("//")
                || trimmed.starts_with("///")
                || trimmed.starts_with("//!")
                || line.contains('"')
            {
                Some((i + 1, line))
            } else {
                None
            }
        })
        .collect()
}

fn forbidden_for_path(path: &Path) -> Vec<&'static str> {
    if path.extension().and_then(|x| x.to_str()) != Some("rs") {
        return FORBIDDEN.to_vec();
    }
    FORBIDDEN
        .iter()
        .copied()
        .filter(|needle| {
            !matches!(
                *needle,
                "#layout"
                    | "#grant"
                    | "#context"
                    | "List<"
                    | "List["
                    | "Map<"
                    | "core.json"
                    | "use jet."
            )
        })
        .collect()
}

fn allowed_retired_reference(path: &Path, line: &str) -> bool {
    let s = path.to_string_lossy();
    if s.ends_with("docs/spec/diagnostics.md")
        && (line.contains("retired") || line.contains("teaching:"))
    {
        return true;
    }
    if s.ends_with(".stderr") && line.contains("retired") {
        return true;
    }
    false
}
