//! D-CANON-SOURCE1 / D-RECONCILE-SCOPE1: live examples, reference surface,
//! and agent memory must not reintroduce retired syntax spellings.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn identifier_prefix_contract_is_canonical() {
    use jet::Syntax::{classify_identifier, IdentifierClass};

    assert_eq!(classify_identifier("name"), IdentifierClass::Ordinary);
    assert_eq!(classify_identifier("_name"), IdentifierClass::SoftPublic);
    assert_eq!(classify_identifier("_name_"), IdentifierClass::SoftPublic);
    assert_eq!(classify_identifier("__name"), IdentifierClass::Reserved);
    assert_eq!(classify_identifier("__name__"), IdentifierClass::Reserved);
    assert_eq!(classify_identifier("__"), IdentifierClass::Reserved);
    assert_eq!(classify_identifier("_"), IdentifierClass::Ordinary);
}

#[test]
fn every_double_underscore_source_identifier_is_rejected() {
    let source = r#"
module __module
use thing as __alias
extern c { fn __ffi(__ffi_arg: Int) }
fn __call(__arg: Int) { __local :: __arg; value.__field }
trait Contract { fn __method(self) }
"#;
    let (_, diagnostics) = jet::Lexer::lex(source);
    let reserved = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "E0067")
        .count();
    assert_eq!(reserved, 10, "{diagnostics:#?}");
}

#[test]
fn compiler_generated_identifiers_use_the_reserved_lane() {
    let source = "fn __jet_generated() { print(\"{__jet_value}\") }";
    let (_, user_diagnostics) = jet::Lexer::lex(source);
    let (tokens, generated_diagnostics) = jet::Lexer::lex_generated(source);
    assert!(user_diagnostics.iter().any(|diagnostic| diagnostic.code == "E0067"));
    assert!(generated_diagnostics.is_empty(), "{generated_diagnostics:#?}");
    assert!(jet::Parser::parse(&tokens).is_ok());
}

#[test]
fn generated_symbol_ladder_has_one_canonical_prefix() {
    use jet::Syntax::{generated_name, generated_path, generated_suffix};

    assert_eq!(generated_name("lambda_7"), "__jet_lambda_7");
    assert_eq!(generated_path("scoring.letter"), "__jet_scoring__letter");
    assert_eq!(generated_suffix("__jet_lambda_7"), "lambda_7");
}

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
    "pure fn",
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
const DATATREE_NORMATIVE_SURFACES: &[&str] = &[
    "docs/spec/syntax-decisions.md",
    "docs/spec/encoding-decisions.md",
    "docs/reference/core-library.md",
    "examples/features/serde/datatree_accessors.jet",
    "examples/features/serde/encoding_breadth.jet",
    "examples/features/serde/encoding_base.jet",
    "examples/features/serde/encoding_base_expert/main.jet",
];
const ACTIVE_MATURITY_DOCS: &[&str] = &["docs/reference/maturity-tags.md"];
const MARKER_CENSUS_DOC: &str = "docs/spec/syntax-decisions.md";
const ENVIRONMENT_REFERENCE: &str = "docs/reference/environment.md";

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
fn pipe_family_has_no_stale_flow_reservation() {
    let decision = fs::read_to_string("docs/spec/syntax-decisions.md").unwrap();
    assert!(decision.contains("D-SHAPE-PIPE1=C — Bars mean alternatives, not general flow"));
    assert!(decision.contains("single `|` is legal only in alternative-list grammar"));

    let syntax = fs::read_to_string("crates/jet-foundation/src/Syntax/math_layout.rs").unwrap();
    assert!(syntax.contains("D-PATO / D-SHAPE-PIPE1=C"));
    assert!(syntax.contains("`|=` remains bitwise-or-assign under S17"));
}

#[test]
fn dynamic_encoding_surface_uses_datatree_name() {
    let mut failures = Vec::new();
    for relative in DATATREE_NORMATIVE_SURFACES {
        let text = fs::read_to_string(relative).unwrap_or_else(|error| {
            panic!("cannot read DataTree inventory surface {relative}: {error}")
        });
        let markdown = Path::new(relative).extension().is_some_and(|ext| ext == "md");
        if markdown {
            for (line_no, line) in markdown_data_name_lines(&text) {
                if !historical_data_reference(line) {
                    failures.push(format!("{relative}:{line_no}: {}", line.trim()));
                }
            }
        } else {
            for offset in source_identifier_offsets(&text, "Data") {
                let line_no = text[..offset].bytes().filter(|byte| *byte == b'\n').count() + 1;
                let line = text.lines().nth(line_no - 1).unwrap_or("");
                failures.push(format!("{relative}:{line_no}: {}", line.trim()));
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
fn datatree_name_check_distinguishes_prose_and_type_names() {
    for lawful in [
        "| `conn.send_text(text)` | Data frames; client frames are masked |",
        "| Data ownership | Official UCD inputs are checked in. |",
    ] {
        assert!(markdown_data_name_lines(lawful).is_empty());
    }
    for prohibited in ["`Data.Object`", "`[Data]`", "`Data?`"] {
        assert_eq!(markdown_data_name_lines(prohibited).len(), 1);
    }

    let tilde_fence = "~~~jet\nvalue: Data\n~~~";
    assert_eq!(markdown_data_name_lines(tilde_fence)[0].0, 2);
    let four_tick_fence = "````jet\nvalue: Data\n```\nother: Data\n````";
    assert_eq!(
        markdown_data_name_lines(four_tick_fence)
            .iter()
            .map(|(line_no, _)| *line_no)
            .collect::<Vec<_>>(),
        [2, 4]
    );
    let multiline_fence =
        "```jet\n/*\nData in a comment\n*/\ntext :: \"\"\"\nData in text\n\"\"\"\nvalue: Data\n```";
    assert_eq!(
        markdown_data_name_lines(multiline_fence)
            .iter()
            .map(|(line_no, _)| *line_no)
            .collect::<Vec<_>>(),
        [8]
    );
    assert_eq!(markdown_data_name_lines("```jet\nvalue: Data")[0].0, 2);

    let lawful_source = "// Data is retired\ntext :: \"Data\"\n/* Data */\nvalue: DataTree";
    assert!(source_identifier_offsets(lawful_source, "Data").is_empty());
    assert_eq!(source_identifier_offsets("value: Data", "Data").len(), 1);
}

#[test]
fn active_maturity_docs_use_meta_field_only() {
    let retired = [
        "#Experimental",
        "#Tested",
        "#Hardened",
        "#Experimental",
        "#Tested",
        "#Hardened",
    ];
    let mut failures = Vec::new();
    for relative in ACTIVE_MATURITY_DOCS {
        let text = fs::read_to_string(relative)
            .unwrap_or_else(|error| panic!("cannot read maturity surface {relative}: {error}"));
        for required in [
            "#Meta(maturity:",
            ".Experimental",
            ".Tested",
            ".Hardened",
        ] {
            if !text.contains(required) {
                failures.push(format!("{relative} does not teach `{required}`"));
            }
        }
        for line in text.lines() {
            for spelling in retired {
                if line.contains(spelling) && !line.contains("not grammar") {
                    failures.push(format!("{relative} teaches standalone `{spelling}`"));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "active maturity docs drifted from the sole `#Meta(maturity: ...)` surface:\n{}",
        failures.join("\n")
    );
}

#[test]
fn proposal_marker_census_matches_syntax_registry() {
    let rules = jet::Policy::APPLIED_RULES
        .iter()
        .filter(|row| matches!(row.status, jet::Policy::RuleStatus::Active))
        .count();
    assert!(rules > 0, "applied-rule registry must be non-empty");
    assert_eq!(
        jet::Syntax::RULE_PREFIX,
        "#",
        "registered applied rules must use the canonical `#` plane"
    );

    let text = fs::read_to_string(MARKER_CENSUS_DOC).unwrap_or_else(|error| {
        panic!("cannot read marker census doc {MARKER_CENSUS_DOC}: {error}")
    });
    assert!(
        text.contains("`#` is the sole prefix for attributes, instructions, and\nproperties.")
            && text.contains("`@` is reserved for locations, addresses, and sources.")
            && text.contains("A leading `@Rule` produces E0063\nwith the canonical `#Rule` fix."),
        "{MARKER_CENSUS_DOC} must teach the Syntax registry's canonical `#` applied-rule plane"
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
    assert!(spec.contains("A ratified entry may sit unbuilt **only** when gated on"));

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
        if !(context.contains("card #") || context.contains("cards #")) {
            uncovered.push(format!("{}: {}", idx + 1, line.trim()));
        }
    }

    assert!(
        uncovered.is_empty(),
        "unbuilt syntax note lacks a live card gate:\n{}",
        uncovered.join("\n")
    );
}

#[test]
fn environment_reference_is_canonical_and_navigable() {
    let reference = fs::read_to_string(ENVIRONMENT_REFERENCE)
        .expect("read canonical environment-variable reference");
    let docs_index = fs::read_to_string("docs/README.md").expect("read docs index");

    assert!(
        docs_index.contains("reference/environment.md"),
        "docs index must link the canonical environment-variable reference"
    );
    for required in [
        "JET_PROP_SEED",
        "JET_RAYLIB_DISPLAY",
        "JET_UI_HEADLESS",
        "JET_ENV_DISABLE",
        "JET_TEST_JOBS",
        "JET_CANVAS_PREREQUISITES",
        "## Naming convention",
    ] {
        assert!(
            reference.contains(required),
            "environment-variable reference missing `{required}`"
        );
    }
}

#[test]
fn documentation_consistency_sweep_stays_current() {
    let spec = fs::read_to_string("docs/spec/spec.md").expect("read language spec");
    let roadmap = fs::read_to_string("docs/spec/roadmap.md").expect("read roadmap");
    let release =
        fs::read_to_string("docs/spec/release-policy.md").expect("read release policy");

    assert!(!spec.contains("[ \"~\" | \"^\" | \"&\" ]"));
    assert!(!spec.contains("Result(String, IOError)"));
    assert!(spec.contains("String ? IOError"));
    assert!(roadmap.contains("Self-hosting → **Epoch 9** (Bootstrapping)"));
    for current_path in [
        "crates/jet-pkg-model/src/Manifest.rs",
        "crates/jet-foundation/src/Syntax.rs",
        "crates/jet-driver/src/Loader.rs",
    ] {
        assert!(
            release.contains(current_path),
            "release policy missing current path `{current_path}`"
        );
    }
}

#[test]
fn marker_plane_matrix_covers_current_marker_families() {
    let syntax = fs::read_to_string("crates/jet-foundation/src/Syntax.rs").expect("read Syntax.rs");
    let decisions =
        fs::read_to_string("docs/spec/syntax-decisions.md").expect("read syntax decisions");

    assert_eq!(
        jet::Syntax::RULE_PREFIX,
        "#",
        "the applied-rule registry must stay on the canonical `#` plane"
    );
    assert_eq!(jet::Syntax::EFFECT_ARROW_OPEN, "=[");
    assert_eq!(jet::Syntax::EFFECT_ARROW_CLOSE, "]=>");
    assert!(
        decisions.contains("**D-SHAPE8=A — Effects inside the arrow** *(ratified 2026-07-14,")
            && decisions.contains("owner-amended by D-ARROW-CONTROL1 on 2026-07-26; card #543)*:"),
        "syntax decisions must keep D-SHAPE8=A ratified, amended, and implemented"
    );

    let mut unique = std::collections::BTreeSet::new();
    for row in jet::Policy::APPLIED_RULES
        .iter()
        .filter(|row| matches!(row.status, jet::Policy::RuleStatus::Active))
    {
        let rule = row.name;
        assert!(
            !rule.starts_with(['#', '@']),
            "registered applied-rule names are bare; RULE_PREFIX owns the `#` plane: {rule}"
        );
        assert!(unique.insert(rule), "duplicate applied rule `{rule}`");
    }
    for rule in [
        "PubFile",
        "NoPrelude",
        "Target",
        "HTML",
        "WasmExport",
        "Layout",
        "SingleUse",
        "UnitFamily",
        "Rename",
        "Skip",
        "Default",
        "RenameAll",
        "Discriminant",
        "Untagged",
        "Codable",
        "Encode",
        "Decode",
        "Comparable",
        "MustUse",
        "Pre",
        "Post",
        "Inline",
        "Persist",
        "CLI",
        "Numeric",
        "Printable",
        "CodableAsBase",
        "Caps",
        "Grant",
        "Unsafe",
        "Impure",
        "Test",
        "Bench",
        "Scrub",
        "Job",
        "Replayable",
        "State",
        "Transition",
        "Track",
        "Meta",
    ] {
        assert!(
            unique.contains(rule),
            "Syntax registry missing applied-rule family member `{rule}`"
        );
    }

    for syntax_anchor in [
        "MARKER_PUB_FILE",
        "MARKER_TARGET",
        "MARKER_LAYOUT",
        "MARKER_CODABLE",
        "APPLIED_RULES",
        "KW_CAPS",
        "KW_GRANT",
        "KW_COMPTIME",
        "KW_DERIVE",
        "MARKER_TRACK",
        "MARKER_LOCAL",
        "MARKER_SHARED",
    ] {
        assert!(
            syntax.contains(syntax_anchor),
            "Syntax.rs must retain live marker anchor `{syntax_anchor}`"
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
            decisions.contains(decision_anchor),
            "live syntax decisions must retain `{decision_anchor}`"
        );
    }
}

#[test]
fn value_dispatch_accepts_range_arm_heads() {
    // #1487 / D-IFDIST1: expression-position value dispatch must parse the same
    // `lo..hi ->` range arm heads statement dispatch already accepts.
    let source = r#"
fn ordered(n: Int) => Int {
    return if n == {
        0..9 -> 1
        10..99 -> 2
        else -> 3
    }
}
fn run() {
    print("{ordered(40)}")
}
"#;
    let (tokens, lex_diags) = jet::Lexer::lex(source);
    assert!(lex_diags.is_empty(), "{lex_diags:#?}");
    let program = jet::Parser::parse(&tokens).expect("value-dispatch range arms must parse");
    assert!(
        !program.items.is_empty(),
        "parsed program should retain the ordered fn"
    );
}

#[test]
fn card_511_census_matches_current_law() {
    let core = fs::read_to_string("crates/jet-foundation/src/Syntax/core_surface.rs")
        .expect("read core surface registry");
    let package = fs::read_to_string("crates/jet-foundation/src/Syntax/package_files.rs")
        .expect("read package surface registry");
    let markers = fs::read_to_string("crates/jet-foundation/src/Syntax/markers.rs")
        .expect("read marker registry");
    let decisions = fs::read_to_string("docs/spec/syntax-decisions.md")
        .expect("read syntax decisions");

    assert!(
        !core.contains("pub const KW_VIEW"),
        "D-MEM1 retired `view` as a keyword"
    );
    assert!(
        package.contains("pub const METHOD_VIEW"),
        "D-SHAPE-PLACE1 keeps retired `.view(a..b)` registered for E0214 teaching"
    );
    for retired in ["MARKER_WASM,", "MARKER_JS,", "MARKER_SUPPRESS,"] {
        assert!(
            !markers.contains(retired),
            "retired marker remains registered: {retired}"
        );
    }
    for value in ["MARKER_EXPERIMENTAL", "MARKER_TESTED", "MARKER_HARDENED"] {
        assert!(
            package.contains(value) && !markers.contains(value),
            "maturity value `{value}` must remain a #Meta value, not a standalone marker"
        );
    }
    for law in ["D-MARKER-FAMILY1", "D-DET1", "D-REPLAY1", "D-MARK-META1"] {
        assert!(decisions.contains(law), "current syntax law omits `{law}`");
    }
}

#[test]
fn module_internal_is_discovery_not_access_control() {
    let syntax = fs::read_to_string("crates/jet-foundation/src/Syntax.rs")
        .expect("read syntax registry");
    let config = fs::read_to_string("crates/jet-foundation/src/Syntax/jetpack_config.rs")
        .expect("read module syntax registry");
    let decisions = fs::read_to_string("docs/spec/syntax-decisions.md")
        .expect("read syntax decisions");

    assert!(syntax.contains("D-SHAPE-MODULEINTERNAL1=A"));
    assert!(config.contains("D-SHAPE-MODULEINTERNAL1=A"));
    assert!(config.contains("pub const MODULE_INTERNAL_PREFIX: &str = \"_\";"));
    assert!(config.contains("pub const PROJECT_IMPORT_PREFIX: &str = \"project.\";"));
    assert!(decisions.contains("`use project._name` remains allowed"));
    assert!(decisions.contains("underscore changes discovery, not access"));
    assert!(!fs::read_to_string("crates/jet-foundation/src/AST/items.rs")
        .expect("read module AST")
        .contains("pub disabled: bool"));
    assert!(!decisions.contains("| D-SHAPE-MODULEINTERNAL1 |"));
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

fn historical_data_reference(line: &str) -> bool {
    [
        "retired `Data`",
        "old `Data`",
        "former `Data`",
        "historical `Data`",
        "pre-`DataTree`",
        "edition 2026",
    ]
    .iter()
    .any(|label| line.contains(label))
}

fn source_identifier_offsets(source: &str, identifier: &str) -> Vec<usize> {
    fn collect(tokens: &[jet::Lexer::Token], identifier: &str, offsets: &mut Vec<usize>) {
        for token in tokens {
            match &token.kind {
                jet::Lexer::TokKind::Ident(name) if name == identifier => {
                    offsets.push(token.span.start)
                }
                jet::Lexer::TokKind::Str(parts) => {
                    for part in parts {
                        if let jet::Lexer::StrTokPart::Interp(tokens) = part {
                            collect(tokens, identifier, offsets);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let (tokens, _) = jet::Lexer::lex(source);
    let mut offsets = Vec::new();
    collect(&tokens, identifier, &mut offsets);
    offsets
}

fn markdown_data_name_lines(text: &str) -> Vec<(usize, &str)> {
    let mut fence: Option<(u8, usize, usize, String)> = None;
    let mut lines = Vec::new();
    let source_lines: Vec<_> = text.lines().collect();
    for (index, line) in source_lines.iter().copied().enumerate() {
        if let Some((open_marker, open_len, _, _)) = fence.as_ref() {
            if markdown_fence(line).is_some_and(|(marker, len, rest)| {
                marker == *open_marker && len >= *open_len && rest.trim().is_empty()
            }) {
                let (_, _, start_line, source) = fence.take().unwrap();
                append_fence_data_lines(&source, start_line, &source_lines, &mut lines);
            } else {
                let (_, _, _, source) = fence.as_mut().unwrap();
                source.push_str(line);
                source.push('\n');
            }
        } else if let Some((marker, len, _)) = markdown_fence(line) {
            fence = Some((marker, len, index + 2, String::new()));
        } else if inline_code_has_identifier(line, "Data") {
            lines.push((index + 1, line));
        }
    }
    if let Some((_, _, start_line, source)) = fence {
        append_fence_data_lines(&source, start_line, &source_lines, &mut lines);
    }
    lines
}

fn append_fence_data_lines<'a>(
    source: &str,
    start_line: usize,
    source_lines: &[&'a str],
    lines: &mut Vec<(usize, &'a str)>,
) {
    for offset in source_identifier_offsets(source, "Data") {
        let line_no =
            start_line + source[..offset].bytes().filter(|byte| *byte == b'\n').count();
        if !lines.iter().any(|(existing, _)| *existing == line_no) {
            lines.push((line_no, source_lines[line_no - 1]));
        }
    }
}

fn markdown_fence(line: &str) -> Option<(u8, usize, &str)> {
    let trimmed = line.trim_start();
    let marker = *trimmed.as_bytes().first()?;
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let len = trimmed.bytes().take_while(|byte| *byte == marker).count();
    (len >= 3).then_some((marker, len, &trimmed[len..]))
}

fn inline_code_has_identifier(line: &str, identifier: &str) -> bool {
    let bytes = line.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'`' {
            at += 1;
            continue;
        }
        let delimiter = bytes[at..].iter().take_while(|byte| **byte == b'`').count();
        let content_start = at + delimiter;
        let mut close = content_start;
        let mut matched = false;
        while close < bytes.len() {
            if bytes[close] != b'`' {
                close += 1;
                continue;
            }
            let run = bytes[close..].iter().take_while(|byte| **byte == b'`').count();
            if run == delimiter {
                if !source_identifier_offsets(&line[content_start..close], identifier).is_empty() {
                    return true;
                }
                at = close + run;
                matched = true;
                break;
            }
            close += run;
        }
        if !matched {
            break;
        }
    }
    false
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

#[test]
fn artifact_extensions_are_one_closed_kind_specific_family() {
    use jet_foundation::Syntax::{self, ArtifactKind};
    assert_eq!(Syntax::ARTIFACT_KINDS, &[
        (ArtifactKind::SourceMap, ".jetmap"),
        (ArtifactKind::Notebook, ".jetnb"),
        (ArtifactKind::Proof, ".jetproof"),
        (ArtifactKind::Trace, ".jettrace"),
        (ArtifactKind::GameReplay, ".jetreplay"),
        (ArtifactKind::ProofReplay, ".jetproof-replay"),
    ]);
    for (kind, suffix) in Syntax::ARTIFACT_KINDS {
        assert_eq!(Syntax::artifact_kind(&format!("artifact{suffix}")), Some(*kind));
    }
    assert_eq!(Syntax::artifact_kind("run.jetproof-replay"), Some(ArtifactKind::ProofReplay));
    assert_ne!(Syntax::artifact_kind("run.jetproof-replay"), Some(ArtifactKind::GameReplay));

    for root in ["Source", "crates", "examples", "tests", "docs"] {
        for path in files(Path::new(root)) {
            if path.ends_with("tests/syntax_reconciliation.rs") { continue; }
            let Ok(text) = fs::read_to_string(&path) else { continue };
            for (line_no, line) in text.lines().enumerate() {
                for retired in [".jproof", ".jtrace", ".jreplay"] {
                    if line.contains(retired) && !line.contains("jet.jproof") {
                        panic!("{}:{} retains retired artifact suffix `{retired}`", path.display(), line_no + 1);
                    }
                }
            }
        }
    }
}

fn forbidden_for_path(path: &Path) -> Vec<&'static str> {
    FORBIDDEN
        .iter()
        .copied()
        .filter(|needle| {
            if *needle == "pure fn" {
                return path.starts_with("docs");
            }
            if path.extension().and_then(|x| x.to_str()) != Some("rs") {
                return true;
            }
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
