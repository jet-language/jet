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

const OLD_BINDING_SCAN_ROOTS: &[&str] = &[
    "crates/jet-foundation/src/Syntax.rs",
    "crates/jet-parser/src/Parser",
    "Source/FixEngine.rs",
    "Source/LSP",
    "docs/reference/syntax-surface.jet",
    "editors/vscode/README.md",
    "editors/zed/README.md",
    "tests/cli",
    "tests/lsp",
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

const OLD_BINDING_CODES: &[&str] = &["E0009", "E0010", "E0985"];
const OLD_BINDING_WORDS: &[&str] = &["let", "val", "var", "set"];

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

#[test]
fn old_binding_migration_paths_stay_removed() {
    let mut failures = Vec::new();
    for root in OLD_BINDING_SCAN_ROOTS {
        for path in files(Path::new(root)) {
            if should_skip(&path) {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            scan_old_binding_codes(&path, &text, &mut failures);
            scan_old_binding_examples(&path, &text, &mut failures);
        }
    }
    assert!(
        failures.is_empty(),
        "old binding migration path found:\n{}",
        failures.join("\n")
    );
}

fn scan_old_binding_codes(path: &Path, text: &str, failures: &mut Vec<String>) {
    for (idx, line) in text.lines().enumerate() {
        for code in OLD_BINDING_CODES {
            if line.contains(code) && !allowed_old_binding_reference(path, text, line) {
                failures.push(format!(
                    "{}:{} contains retired binding diagnostic `{}`",
                    path.display(),
                    idx + 1,
                    code
                ));
            }
        }
    }
}

fn scan_old_binding_examples(path: &Path, text: &str, failures: &mut Vec<String>) {
    if path.extension().and_then(|x| x.to_str()) == Some("rs") {
        for (line, literal) in rust_string_literals(text) {
            if literal_has_old_binding_example(&literal)
                && !allowed_old_binding_reference(path, text, &literal)
            {
                failures.push(format!(
                    "{}:{} contains retired binding spelling in a Rust string literal",
                    path.display(),
                    line
                ));
            }
        }
        return;
    }

    for (idx, line) in text.lines().enumerate() {
        if line_has_old_binding_start(line) && !allowed_old_binding_reference(path, text, line) {
            failures.push(format!(
                "{}:{} contains retired binding spelling `{}`",
                path.display(),
                idx + 1,
                line.trim()
            ));
        }
    }
}

fn rust_string_literals(text: &str) -> Vec<(usize, String)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                line += 1;
                i += 1;
            }
            b'r' => {
                let start_line = line;
                let mut j = i + 1;
                while j < bytes.len() && bytes[j] == b'#' {
                    j += 1;
                }
                if j >= bytes.len() || bytes[j] != b'"' {
                    i += 1;
                    continue;
                }
                let hashes = j - (i + 1);
                j += 1;
                let content_start = j;
                while j < bytes.len() {
                    if bytes[j] == b'\n' {
                        line += 1;
                    }
                    if bytes[j] == b'"' && bytes[j + 1..].starts_with(&vec![b'#'; hashes]) {
                        let content = String::from_utf8_lossy(&bytes[content_start..j]).to_string();
                        out.push((start_line, content));
                        j += 1 + hashes;
                        break;
                    }
                    j += 1;
                }
                i = j;
            }
            b'"' => {
                if i > 0 && i + 1 < bytes.len() && bytes[i - 1] == b'\'' && bytes[i + 1] == b'\'' {
                    i += 1;
                    continue;
                }
                let start_line = line;
                let mut j = i + 1;
                let mut content = String::new();
                while j < bytes.len() {
                    match bytes[j] {
                        b'\\' if j + 1 < bytes.len() => {
                            let next = bytes[j + 1] as char;
                            if next == 'n' {
                                content.push('\n');
                            } else {
                                content.push(next);
                            }
                            j += 2;
                        }
                        b'"' => {
                            j += 1;
                            break;
                        }
                        b'\n' => {
                            line += 1;
                            content.push('\n');
                            j += 1;
                        }
                        b => {
                            content.push(b as char);
                            j += 1;
                        }
                    }
                }
                out.push((start_line, content));
                i = j;
            }
            _ => {
                i += 1;
            }
        }
    }
    out
}

fn literal_has_old_binding_example(literal: &str) -> bool {
    literal.lines().any(line_has_old_binding_start)
}

fn line_has_old_binding_start(line: &str) -> bool {
    for segment in line.split(['{', ';']) {
        let trimmed = segment.trim_start();
        for word in OLD_BINDING_WORDS {
            if let Some(rest) = trimmed.strip_prefix(word) {
                if rest.starts_with(' ') || rest.starts_with('\t') {
                    return true;
                }
            }
        }
    }
    false
}

fn allowed_old_binding_reference(path: &Path, text: &str, line: &str) -> bool {
    let s = path.to_string_lossy();
    s.ends_with("Source/LSP/mod.rs")
        && text.contains("fn old_binding_keyword_has_no_teaching_edit")
        && (line.contains("let x = 1") || line.contains("E0009") || line.contains("E0985"))
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
