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
const SYNTAX_STATUS_MATRIX: &str =
    "docs/plans/epoch-3/syntax-law-source-status-matrix-2026-07-07.md";
const MARKER_PLANE_MATRIX: &str =
    "docs/plans/epoch-3/marker-plane-source-of-truth-matrix-2026-07-07.md";
const DATATREE_NORMATIVE_SURFACES: &[&str] = &[
    "docs/spec/syntax-decisions.md",
    "docs/spec/encoding-decisions.md",
    "docs/reference/core-library.md",
    "examples/features/serde/datatree_accessors.jet",
    "examples/features/serde/encoding_breadth.jet",
];
const MATRIX_UNBUILT_MARKERS: &[&str] = &[
    "S74-D-DESTRUCT1-ARM",
    "D-IGNORERET1",
    "D-SMELLLINT1",
    "D-NOSTD1",
    "D-PATHFS1",
    "D-TIMEDEPTH1",
    "D-MATHLIB1",
    "D-HTTPLIB1",
    "D-HTTPLIB2",
    "D-HTTPLIB3",
    "D-ROUTE1",
    "D-HONESTNUM1",
    "D-OPTGC1",
];
const MARKER_PLANE_ROWS: &[(&str, &[&str])] = &[
    (
        "file-target-directives",
        &[
            "#PubFile",
            "#NoPrelude",
            "#Target(Web)",
            "#Html",
            "#Js",
            "#Wasm",
            "#WasmExport",
        ],
    ),
    (
        "type-layout-directives",
        &[
            "#Layout(c)",
            "#Layout(columnar)",
            "#SingleUse",
            "#UnitFamily",
        ],
    ),
    (
        "serde-directive-attributes",
        &[
            "#[Rename",
            "#[Skip]",
            "#[Default]",
            "#RenameAll",
            "#Tag",
            "#Untagged",
        ],
    ),
    (
        "derive-contract-markers",
        &[
            "@Codable",
            "@[Encode, Decode]",
            "@Debug",
            "@Summarize",
            "@Comparable",
        ],
    ),
    (
        "general-contract-markers",
        &[
            "@Pure", "@MustUse", "@Pre", "@Post", "@Inline", "@Persist", "@Cli",
        ],
    ),
    (
        "distinct-capability-bundles",
        &["@Numeric", "@Printable", "@CodableAsBase"],
    ),
    (
        "effect-capability-directives",
        &["#(Fs)", "#(via f)", "#Caps", "#Grant"],
    ),
    (
        "unsafe-impure-gates",
        &["#Unsafe", "#Impure", "D-UNSAFE-REASON1"],
    ),
    (
        "test-bench-directives",
        &["#Test(\"name\")", "#Test fn", "#Bench(\"name\")"],
    ),
    (
        "typing-fact-directives",
        &[
            "#Tainted",
            "#Sanitizer",
            "#Replayable",
            "#State",
            "#Transition",
            "#Suppress",
            "#Track",
        ],
    ),
    (
        "comptime-metaprogramming",
        &["comptime", "$name", "derive T.Trait"],
    ),
    (
        "capability-sigils",
        &["^T", "&T", "copy x", "p.*", "edit", "share"],
    ),
    (
        "maturity-markers",
        &["#Meta(maturity: .Experimental", ".Tested", ".Hardened"],
    ),
    (
        "retired-paused-marker-spellings",
        &["@unsafe", "#extern", "#layout", "#Bench \"", "#[Serialize"],
    ),
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

#[test]
fn dynamic_encoding_surface_uses_datatree_name() {
    let mut failures = Vec::new();
    for relative in DATATREE_NORMATIVE_SURFACES {
        let text = fs::read_to_string(relative).unwrap_or_else(|error| {
            panic!("cannot read DataTree inventory surface {relative}: {error}")
        });
        for (index, line) in text.lines().enumerate() {
            let historical = [
                "retired `Data`",
                "old `Data`",
                "former `Data`",
                "historical `Data`",
                "pre-`DataTree`",
                "edition 2026",
            ]
            .iter()
            .any(|label| line.contains(label));
            let bare_dynamic_name = line.match_indices("Data").any(|(at, _)| {
                let before = line[..at].chars().next_back();
                let after = line[at + "Data".len()..].chars().next();
                !before.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                    && !after.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            });
            if bare_dynamic_name && !historical {
                failures.push(format!("{relative}:{}: {}", index + 1, line.trim()));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "current dynamic-encoding surfaces must use `DataTree`; label historical `Data` references explicitly:\n{}",
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

#[test]
fn syntax_status_matrix_covers_unbuilt_notes() {
    // CAPABILITY_CLAIM: claim.syntax-law / syntax-matrix
    let spec = fs::read_to_string("docs/spec/syntax-decisions.md").expect("read syntax decisions");
    let matrix = fs::read_to_string(SYNTAX_STATUS_MATRIX).expect("read syntax status matrix");

    for marker in MATRIX_UNBUILT_MARKERS {
        let row = format!("| `{}` |", marker);
        assert!(
            matrix.contains(&row),
            "syntax status matrix missing row for {marker}"
        );
    }

    let lines: Vec<&str> = spec.lines().collect();
    let mut uncovered = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let lower = line.to_ascii_lowercase();
        if !(lower.contains("unbuilt") || lower.contains("not yet implemented")) {
            continue;
        }
        if idx < 20 || lower.contains("have/have-not ledger") {
            continue;
        }
        let start = idx.saturating_sub(4);
        let end = (idx + 2).min(lines.len());
        let context = lines[start..end].join("\n");
        let covered_by_marker = MATRIX_UNBUILT_MARKERS
            .iter()
            .any(|marker| context.contains(marker));
        let covered_by_named_prose = context.contains("dispatch-arm struct-pattern head")
            && matrix.contains("S74-D-DESTRUCT1-ARM");
        if !(covered_by_marker || covered_by_named_prose) {
            uncovered.push(format!("{}: {}", idx + 1, line.trim()));
        }
    }

    assert!(
        uncovered.is_empty(),
        "unbuilt syntax note lacks matrix coverage:\n{}",
        uncovered.join("\n")
    );
}

#[test]
fn marker_plane_matrix_covers_current_marker_families() {
    let matrix = fs::read_to_string(MARKER_PLANE_MATRIX).expect("read marker-plane matrix");
    let syntax = fs::read_to_string("crates/jet-foundation/src/Syntax.rs").expect("read Syntax.rs");
    let decisions =
        fs::read_to_string("docs/spec/syntax-decisions.md").expect("read syntax decisions");

    for (row, spellings) in MARKER_PLANE_ROWS {
        let row_token = format!("| `{}` |", row);
        assert!(
            matrix.contains(&row_token),
            "marker-plane matrix missing row `{row}`"
        );
        for spelling in *spellings {
            assert!(
                matrix.contains(spelling),
                "marker-plane matrix row `{row}` missing spelling `{spelling}`"
            );
        }
    }

    for syntax_anchor in [
        "MARKER_PUB_FILE",
        "ATTR_TARGET",
        "ATTR_LAYOUT",
        "ATTR_CODABLE",
        "CONTRACT_MARKERS",
        "KW_CAPS",
        "KW_GRANT",
        "KW_COMPTIME",
        "KW_DERIVE",
        "ATTR_TRACK",
    ] {
        assert!(
            matrix.contains(syntax_anchor) && syntax.contains(syntax_anchor),
            "marker-plane matrix must cite live Syntax.rs anchor `{syntax_anchor}`"
        );
    }

    for decision_anchor in [
        "D-MARKER-FAMILY1",
        "D-MARKERMOVE1",
        "D-UNSAFE2",
        "D-TESTPAREN1",
        "D-BENCH1",
        "D-CAPBUNDLE1",
        "D-CTMARKER1",
    ] {
        assert!(
            matrix.contains(decision_anchor) && decisions.contains(decision_anchor),
            "marker-plane matrix must cite syntax decision `{decision_anchor}`"
        );
    }
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
